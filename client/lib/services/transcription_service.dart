import 'dart:async';

import 'package:crypto/crypto.dart';
import 'package:dio/dio.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:oracy/db/database.dart';
import 'package:oracy/models/voice_note.dart';
import 'package:oracy/services/api_client.dart';
import 'package:oracy/services/upload_retry_policy.dart';
import 'package:uuid/uuid.dart';

// Conditional imports for platform-specific file handling
import 'transcription_service_stub.dart'
    if (dart.library.io) 'transcription_service_io.dart'
    if (dart.library.html) 'transcription_service_web.dart'
    as platform;

const int maxTranscriptionChunkBytes = 26214400;
const int maxTranscriptionChunkCount = 256;
const Duration defaultTranscriptionPollInterval = Duration(seconds: 5);

final _uuid = const Uuid();

typedef TranscriptionSleep = Future<void> Function(Duration duration);
typedef TranscriptionNow = DateTime Function();

Future<void> _defaultSleep(Duration duration) => Future.delayed(duration);

class TranscriptionClientException implements Exception {
  final UploadFailureClassification classification;

  const TranscriptionClientException(this.classification);

  @override
  String toString() => classification.message;
}

class TranscriptionJob {
  final String id;
  final String status;
  final int chunkCount;
  final int chunksReceived;
  final DateTime? nextAttemptAt;
  final String? failureMessage;
  final bool? retryableByClient;
  final String? voiceNoteId;

  const TranscriptionJob({
    required this.id,
    required this.status,
    required this.chunkCount,
    required this.chunksReceived,
    this.nextAttemptAt,
    this.failureMessage,
    this.retryableByClient,
    this.voiceNoteId,
  });

  factory TranscriptionJob.fromJson(Map<String, dynamic> json) {
    final nextAttemptAtValue = json['next_attempt_at'];
    return TranscriptionJob(
      id: json['id'] as String,
      status: json['status'] as String,
      chunkCount: json['chunk_count'] as int,
      chunksReceived: json['chunks_received'] as int,
      nextAttemptAt: nextAttemptAtValue is String
          ? DateTime.parse(nextAttemptAtValue)
          : null,
      failureMessage: json['failure_message'] as String?,
      retryableByClient: json['retryable_by_client'] as bool?,
      voiceNoteId: json['voice_note_id'] as String?,
    );
  }

  bool get isTerminal => status == 'succeeded' || status == 'failed';
}

/// State for transcription operations.
sealed class TranscriptionState {
  const TranscriptionState();
}

class TranscriptionIdle extends TranscriptionState {
  const TranscriptionIdle();
}

class TranscriptionUploading extends TranscriptionState {
  final double progress;
  const TranscriptionUploading({this.progress = 0.0});
}

class TranscriptionProcessing extends TranscriptionState {
  const TranscriptionProcessing();
}

class TranscriptionSuccess extends TranscriptionState {
  final VoiceNote voiceNote;
  const TranscriptionSuccess(this.voiceNote);
}

/// Types of transcription errors for better UI differentiation.
enum TranscriptionErrorType {
  /// Authentication/API key issues.
  auth,

  /// Network connectivity issues.
  network,

  /// Server timeout issues.
  timeout,

  /// File validation issues (too large, wrong format).
  fileValidation,

  /// Server-side transcription failure.
  transcription,

  /// Unknown/generic errors.
  unknown,
}

class UploadFailureClassification {
  final String message;
  final TranscriptionErrorType errorType;
  final bool isRetryable;
  final bool requiresFreshIdempotencyKey;

  const UploadFailureClassification({
    required this.message,
    required this.errorType,
    required this.isRetryable,
    this.requiresFreshIdempotencyKey = false,
  });
}

typedef LocalFileDeleter = Future<void> Function(String filePath);

Future<void> defaultLocalFileDeleter(String filePath) {
  return platform.deleteLocalFileIfExists(filePath);
}

final localFileDeleterProvider = Provider<LocalFileDeleter>((ref) {
  return defaultLocalFileDeleter;
});

class TranscriptionError extends TranscriptionState {
  final String message;
  final TranscriptionErrorType errorType;
  final String? filePath;
  final bool isRetryable;

  const TranscriptionError(
    this.message, {
    this.errorType = TranscriptionErrorType.unknown,
    this.filePath,
    bool? isRetryable,
  }) : isRetryable =
           isRetryable ??
           (errorType == TranscriptionErrorType.network ||
               errorType == TranscriptionErrorType.timeout ||
               errorType == TranscriptionErrorType.transcription ||
               errorType == TranscriptionErrorType.unknown);

  bool get isAuthError => errorType == TranscriptionErrorType.auth;
}

String? _extractResponseDetail(Object? responseData) {
  if (responseData is! Map) {
    return null;
  }

  final detail = responseData['detail'];
  return detail is String ? detail : null;
}

UploadFailureClassification classifyUploadFailure(Object error) {
  if (error is TranscriptionClientException) {
    return error.classification;
  }

  if (error is! DioException) {
    return UploadFailureClassification(
      message: 'Transcription failed: $error',
      errorType: TranscriptionErrorType.unknown,
      isRetryable: false,
    );
  }

  final statusCode = error.response?.statusCode;
  final detail = _extractResponseDetail(error.response?.data);

  if (statusCode == 401) {
    return const UploadFailureClassification(
      message: 'Authentication failed. Please check your API key in Settings.',
      errorType: TranscriptionErrorType.auth,
      isRetryable: true,
    );
  }

  if (statusCode == 413) {
    return const UploadFailureClassification(
      message: 'Audio chunk is too large (max 25 MiB).',
      errorType: TranscriptionErrorType.fileValidation,
      isRetryable: false,
    );
  }

  if (statusCode == 415) {
    return const UploadFailureClassification(
      message: 'Unsupported audio format. Supported: m4a, mp3, wav, webm.',
      errorType: TranscriptionErrorType.fileValidation,
      isRetryable: false,
    );
  }

  if (statusCode == 429) {
    return UploadFailureClassification(
      message: detail ?? 'Rate limit exceeded. Please try again shortly.',
      errorType: TranscriptionErrorType.transcription,
      isRetryable: true,
    );
  }

  if (statusCode != null && statusCode >= 500) {
    return UploadFailureClassification(
      message: detail ?? 'Transcription service error. Please try again later.',
      errorType: TranscriptionErrorType.transcription,
      isRetryable: true,
    );
  }

  if (statusCode != null && statusCode >= 400) {
    return UploadFailureClassification(
      message: detail ?? 'Transcription failed: ${error.message}',
      errorType: TranscriptionErrorType.transcription,
      isRetryable: false,
    );
  }

  if (error.type == DioExceptionType.connectionTimeout ||
      error.type == DioExceptionType.sendTimeout) {
    return const UploadFailureClassification(
      message: 'Upload timed out. Please check your connection and try again.',
      errorType: TranscriptionErrorType.timeout,
      isRetryable: true,
    );
  }

  if (error.type == DioExceptionType.receiveTimeout) {
    return const UploadFailureClassification(
      message: 'Server took too long to respond. Please try again.',
      errorType: TranscriptionErrorType.timeout,
      isRetryable: true,
    );
  }

  if (error.type == DioExceptionType.connectionError) {
    return const UploadFailureClassification(
      message:
          'Unable to connect to server. Please check your internet connection.',
      errorType: TranscriptionErrorType.network,
      isRetryable: true,
    );
  }

  return UploadFailureClassification(
    message: detail ?? 'Transcription failed: ${error.message}',
    errorType: TranscriptionErrorType.unknown,
    isRetryable: false,
  );
}

Future<void> recordUploadFailure(
  AppDatabase db,
  int uploadId,
  UploadFailureClassification classification,
) async {
  if (classification.isRetryable) {
    if (classification.requiresFreshIdempotencyKey) {
      await db.replaceUploadIdempotencyKey(uploadId);
    }

    final upload = await db.getUploadById(uploadId);
    if (upload != null && upload.retryCount + 1 >= maxRetryAttempts) {
      await db.incrementRetryCount(
        uploadId,
        errorMessage: 'Automatic retries exhausted. ${classification.message}',
      );
      await db.markAsTerminalFailure(
        uploadId,
        errorMessage: 'Automatic retries exhausted. ${classification.message}',
      );
      return;
    }

    await db.incrementRetryCount(
      uploadId,
      errorMessage: classification.message,
    );
    return;
  }

  await db.markAsTerminalFailure(
    uploadId,
    errorMessage: classification.message,
  );
}

Future<void> finalizeSuccessfulUpload(
  AppDatabase db, {
  required int uploadId,
  required String filePath,
  LocalFileDeleter deleteLocalFile = defaultLocalFileDeleter,
}) async {
  try {
    await deleteLocalFile(filePath);
    await db.markAsCompleted(uploadId);
  } catch (e) {
    await db.markAsCleanupPending(
      uploadId,
      errorMessage: 'Local cleanup failed: $e',
    );

    if (kDebugMode) {
      debugPrint('[TRANSCRIPTION] Could not delete transcribed file: $e');
    }
  }
}

Future<void> retryPendingUploadCleanup(
  AppDatabase db, {
  LocalFileDeleter deleteLocalFile = defaultLocalFileDeleter,
}) async {
  final cleanupPendingUploads = await db.getCleanupPendingUploads();

  for (final upload in cleanupPendingUploads) {
    await finalizeSuccessfulUpload(
      db,
      uploadId: upload.id,
      filePath: upload.audioPath,
      deleteLocalFile: deleteLocalFile,
    );
  }
}

/// Service for uploading audio and getting voice notes.
class TranscriptionService {
  final Dio _dio;
  final Duration pollInterval;
  final TranscriptionSleep _sleep;
  final TranscriptionNow _now;

  TranscriptionService(
    this._dio, {
    this.pollInterval = defaultTranscriptionPollInterval,
    TranscriptionSleep sleep = _defaultSleep,
    TranscriptionNow? now,
  }) : _sleep = sleep,
       _now = now ?? DateTime.now;

  /// Upload an audio file through the v0.1.0 transcription-job protocol.
  /// On native platforms, filePath is a file system path.
  /// On web, filePath is a blob URL (blob:https://...).
  Future<VoiceNote> transcribe(
    String filePath, {
    String? language,
    String? idempotencyKey,
    DateTime? recordedAt,
    void Function(double progress)? onProgress,
  }) async {
    if (kDebugMode) {
      debugPrint(
        '[TRANSCRIPTION_SERVICE] transcribe() called with filePath: $filePath',
      );
    }

    // Get file data using platform-specific implementation
    if (kDebugMode) {
      debugPrint('[TRANSCRIPTION_SERVICE] Calling platform.getFileData...');
    }
    final fileData = await platform.getFileData(filePath);
    if (kDebugMode) {
      debugPrint(
        '[TRANSCRIPTION_SERVICE] Got ${fileData.bytes.length} bytes, filename: ${fileData.filename}',
      );
    }

    final audioFormat = audioFormatFromFilename(fileData.filename);
    if (audioFormat == null) {
      throw const TranscriptionClientException(
        UploadFailureClassification(
          message: 'Unsupported audio format. Supported: m4a, mp3, wav, webm.',
          errorType: TranscriptionErrorType.fileValidation,
          isRetryable: false,
        ),
      );
    }

    final chunks = chunkAudio(fileData.bytes);
    final stableIdempotencyKey = idempotencyKey ?? _uuid.v4();
    final openBody = <String, dynamic>{
      'recorded_at': (recordedAt ?? _now()).toUtc().toIso8601String(),
      'chunk_count': chunks.length,
      'audio_format': audioFormat,
    };
    if (language != null) {
      openBody['language'] = language;
    }
    final openResponse = await _dio.post(
      '/api/v1/transcription-jobs',
      data: openBody,
      options: Options(
        contentType: Headers.jsonContentType,
        headers: {'Idempotency-Key': stableIdempotencyKey},
      ),
    );

    var job = TranscriptionJob.fromJson(
      openResponse.data as Map<String, dynamic>,
    );

    if (job.status == 'accepting_chunks') {
      for (var index = 0; index < chunks.length; index++) {
        final chunk = chunks[index];
        await _pushChunk(job.id, index, chunk, fileData);
        onProgress?.call((index + 1) / chunks.length);
      }

      final finalizeResponse = await _dio.post(
        '/api/v1/transcription-jobs/${job.id}/finalize',
        options: Options(receiveTimeout: const Duration(minutes: 5)),
      );
      job = TranscriptionJob.fromJson(
        finalizeResponse.data as Map<String, dynamic>,
      );
    } else {
      onProgress?.call(1.0);
    }

    final succeeded = await _pollUntilTerminal(job);
    final voiceNoteId = succeeded.voiceNoteId;
    if (voiceNoteId == null || voiceNoteId.isEmpty) {
      throw const TranscriptionClientException(
        UploadFailureClassification(
          message: 'Transcription succeeded but no voice note was returned.',
          errorType: TranscriptionErrorType.transcription,
          isRetryable: false,
        ),
      );
    }

    final voiceNoteResponse = await _dio.get(
      '/api/v1/voice-notes/$voiceNoteId',
    );
    return VoiceNote.fromJson(voiceNoteResponse.data as Map<String, dynamic>);
  }

  Future<void> _pushChunk(
    String jobId,
    int chunkIndex,
    Uint8List chunk,
    FileData fileData,
  ) async {
    final formData = FormData.fromMap({
      'chunk_index': chunkIndex.toString(),
      'chunk_sha256': sha256Hex(chunk),
      'file': MultipartFile.fromBytes(
        chunk,
        filename:
            'chunk-$chunkIndex.${audioFormatFromFilename(fileData.filename) ?? 'bin'}',
        contentType: fileData.contentType,
      ),
    });

    await _dio.post(
      '/api/v1/transcription-jobs/$jobId/chunks',
      data: formData,
      options: Options(
        contentType: 'multipart/form-data',
        sendTimeout: const Duration(minutes: 2),
      ),
    );
  }

  Future<TranscriptionJob> _pollUntilTerminal(TranscriptionJob job) async {
    var current = job;
    while (!current.isTerminal) {
      await _sleep(_pollDelay(current));
      final response = await _dio.get('/api/v1/transcription-jobs/${job.id}');
      current = TranscriptionJob.fromJson(
        response.data as Map<String, dynamic>,
      );
    }

    if (current.status == 'succeeded') {
      return current;
    }

    final retryable = current.retryableByClient ?? false;
    throw TranscriptionClientException(
      UploadFailureClassification(
        message: current.failureMessage ?? 'Transcription failed.',
        errorType: TranscriptionErrorType.transcription,
        isRetryable: retryable,
        requiresFreshIdempotencyKey: retryable,
      ),
    );
  }

  Duration _pollDelay(TranscriptionJob job) {
    if (job.status != 'retry_waiting' || job.nextAttemptAt == null) {
      return pollInterval;
    }

    final delay = job.nextAttemptAt!.difference(_now().toUtc());
    return delay.isNegative ? Duration.zero : delay;
  }
}

/// Data class for file information.
class FileData {
  final Uint8List bytes;
  final String filename;
  final DioMediaType? contentType;

  const FileData({
    required this.bytes,
    required this.filename,
    this.contentType,
  });
}

MultipartFile multipartFileFromFileData(FileData fileData) {
  return MultipartFile.fromBytes(
    fileData.bytes,
    filename: fileData.filename,
    contentType: fileData.contentType,
  );
}

String? audioFormatFromFilename(String filename) {
  final extension = filename.split('.').last.toLowerCase();
  return switch (extension) {
    'm4a' || 'mp3' || 'wav' || 'webm' => extension,
    _ => null,
  };
}

List<Uint8List> chunkAudio(Uint8List bytes) {
  if (bytes.isEmpty) {
    throw const TranscriptionClientException(
      UploadFailureClassification(
        message: 'Audio file is empty.',
        errorType: TranscriptionErrorType.fileValidation,
        isRetryable: false,
      ),
    );
  }

  final chunkCount = (bytes.length / maxTranscriptionChunkBytes).ceil();
  if (chunkCount > maxTranscriptionChunkCount) {
    throw const TranscriptionClientException(
      UploadFailureClassification(
        message: 'Audio file is too large for v0.1.0 chunked submission.',
        errorType: TranscriptionErrorType.fileValidation,
        isRetryable: false,
      ),
    );
  }

  return [
    for (
      var offset = 0;
      offset < bytes.length;
      offset += maxTranscriptionChunkBytes
    )
      Uint8List.sublistView(
        bytes,
        offset,
        (offset + maxTranscriptionChunkBytes).clamp(0, bytes.length),
      ),
  ];
}

String sha256Hex(Uint8List bytes) => sha256.convert(bytes).toString();

DateTime recordedAtForQueuedUpload(PendingUpload upload) {
  final timestamp = _recordingTimestampFromPath(upload.audioPath);
  if (timestamp != null) {
    return DateTime.fromMillisecondsSinceEpoch(timestamp, isUtc: true);
  }
  return upload.createdAt.toUtc();
}

int? _recordingTimestampFromPath(String audioPath) {
  final match = RegExp(r'oracy_recording_(\d+)\.').firstMatch(audioPath);
  if (match == null) {
    return null;
  }
  return int.tryParse(match.group(1)!);
}

/// Provider for transcription service.
final transcriptionServiceProvider = Provider<TranscriptionService>((ref) {
  final dio = ref.watch(apiClientProvider);
  return TranscriptionService(dio);
});

/// Notifier for managing transcription state.
class TranscriptionNotifier extends Notifier<TranscriptionState> {
  @override
  TranscriptionState build() => const TranscriptionIdle();

  /// Transcribe an audio file.
  Future<void> transcribe(String filePath, {String? language}) async {
    if (kDebugMode) {
      debugPrint(
        '[TRANSCRIPTION] transcribe() called with filePath: $filePath',
      );
    }

    final queuedUploadId = await _ensureQueuedForRetry(
      filePath,
      language: language,
    );
    final db = ref.read(appDatabaseProvider);

    if (queuedUploadId != null) {
      await db.markAsUploading(queuedUploadId);
    }

    final queuedUpload = queuedUploadId == null
        ? null
        : await db.getUploadById(queuedUploadId);

    // Check for API key first
    final storage = ref.read(secureStorageProvider);
    final hasKey = await storage.hasApiKey();
    if (kDebugMode) {
      debugPrint('[TRANSCRIPTION] hasApiKey: $hasKey');
    }
    if (!hasKey) {
      if (queuedUploadId != null) {
        await db.markAsPending(queuedUploadId);
      }

      state = TranscriptionError(
        'No API key configured. Please add your API key in Settings.',
        errorType: TranscriptionErrorType.auth,
        filePath: filePath,
      );
      return;
    }

    state = const TranscriptionUploading();
    if (kDebugMode) {
      debugPrint('[TRANSCRIPTION] State set to TranscriptionUploading');
    }

    try {
      final service = ref.read(transcriptionServiceProvider);

      state = const TranscriptionUploading(progress: 0.0);

      final voiceNote = await service.transcribe(
        filePath,
        language: language,
        idempotencyKey: queuedUpload?.idempotencyKey,
        recordedAt: queuedUpload == null
            ? null
            : recordedAtForQueuedUpload(queuedUpload),
        onProgress: (progress) {
          // Only update if still in uploading state
          if (state is TranscriptionUploading) {
            if (progress >= 1.0) {
              state = const TranscriptionProcessing();
            } else {
              state = TranscriptionUploading(progress: progress);
            }
          }
        },
      );

      if (queuedUploadId != null) {
        await finalizeSuccessfulUpload(
          db,
          uploadId: queuedUploadId,
          filePath: filePath,
          deleteLocalFile: ref.read(localFileDeleterProvider),
        );
      }

      state = TranscriptionSuccess(voiceNote);
    } on DioException catch (e) {
      final classification = classifyUploadFailure(e);
      await _markQueuedUploadFailed(
        queuedUploadId,
        classification: classification,
      );
      state = TranscriptionError(
        classification.message,
        errorType: classification.errorType,
        filePath: filePath,
        isRetryable: classification.isRetryable,
      );
    } catch (e, stackTrace) {
      final classification = classifyUploadFailure(e);
      await _markQueuedUploadFailed(
        queuedUploadId,
        classification: classification,
      );

      if (kDebugMode) {
        debugPrint('[TRANSCRIPTION] Caught exception: $e');
        debugPrint('[TRANSCRIPTION] Stack trace: $stackTrace');
      }
      state = TranscriptionError(
        classification.message,
        errorType: classification.errorType,
        filePath: filePath,
        isRetryable: classification.isRetryable,
      );
    }
  }

  Future<int?> _ensureQueuedForRetry(
    String filePath, {
    String? language,
  }) async {
    if (kIsWeb || filePath.isEmpty) {
      return null;
    }

    return ref
        .read(appDatabaseProvider)
        .ensurePendingUpload(audioPath: filePath, language: language);
  }

  Future<void> _markQueuedUploadFailed(
    int? uploadId, {
    required UploadFailureClassification classification,
  }) async {
    if (uploadId == null) {
      return;
    }

    await recordUploadFailure(
      ref.read(appDatabaseProvider),
      uploadId,
      classification,
    );
  }

  /// Retry the last failed transcription.
  ///
  /// Returns `true` if retry was initiated, `false` if no file path was available.
  Future<bool> retry({String? language}) async {
    final currentState = state;
    if (currentState is! TranscriptionError || currentState.filePath == null) {
      return false;
    }
    await transcribe(currentState.filePath!, language: language);
    return true;
  }

  /// Reset to idle state.
  void reset() {
    state = const TranscriptionIdle();
  }
}

/// Provider for transcription state management.
final transcriptionProvider =
    NotifierProvider<TranscriptionNotifier, TranscriptionState>(
      TranscriptionNotifier.new,
    );
