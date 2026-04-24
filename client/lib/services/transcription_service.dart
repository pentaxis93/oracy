import 'package:dio/dio.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:oracy/db/database.dart';
import 'package:oracy/services/api_client.dart';
import 'package:oracy/services/upload_retry_policy.dart';

// Conditional imports for platform-specific file handling
import 'transcription_service_stub.dart'
    if (dart.library.io) 'transcription_service_io.dart'
    if (dart.library.html) 'transcription_service_web.dart'
    as platform;

/// Response from the transcription API.
class TranscriptResponse {
  final String id;
  final String transcript;
  final double audioDurationSeconds;
  final String? audioFormat;
  final int? audioSizeBytes;
  final String? transcriptLanguage;
  final String? whisperModel;
  final int? processingTimeMs;
  final int costCents;
  final DateTime createdAt;

  const TranscriptResponse({
    required this.id,
    required this.transcript,
    required this.audioDurationSeconds,
    this.audioFormat,
    this.audioSizeBytes,
    this.transcriptLanguage,
    this.whisperModel,
    this.processingTimeMs,
    required this.costCents,
    required this.createdAt,
  });

  factory TranscriptResponse.fromJson(Map<String, dynamic> json) {
    return TranscriptResponse(
      id: json['id'] as String,
      transcript: json['transcript'] as String,
      audioDurationSeconds: (json['audio_duration_seconds'] as num).toDouble(),
      audioFormat: json['audio_format'] as String?,
      audioSizeBytes: json['audio_size_bytes'] as int?,
      transcriptLanguage: json['transcript_language'] as String?,
      whisperModel: json['whisper_model'] as String?,
      processingTimeMs: json['processing_time_ms'] as int?,
      costCents: json['cost_cents'] as int,
      createdAt: DateTime.parse(json['created_at'] as String),
    );
  }
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
  final TranscriptResponse transcript;
  const TranscriptionSuccess(this.transcript);
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

  const UploadFailureClassification({
    required this.message,
    required this.errorType,
    required this.isRetryable,
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
      message: 'Audio file is too large (max 25MB).',
      errorType: TranscriptionErrorType.fileValidation,
      isRetryable: false,
    );
  }

  if (statusCode == 415) {
    return const UploadFailureClassification(
      message:
          'Unsupported audio format. Supported: mp3, mp4, m4a, wav, webm, opus.',
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

/// Service for uploading audio and getting transcriptions.
class TranscriptionService {
  final Dio _dio;

  TranscriptionService(this._dio);

  /// Upload an audio file for transcription.
  /// On native platforms, filePath is a file system path.
  /// On web, filePath is a blob URL (blob:https://...).
  Future<TranscriptResponse> transcribe(
    String filePath, {
    String? language,
    String? idempotencyKey,
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

    final formFields = <String, dynamic>{
      'file': multipartFileFromFileData(fileData),
    };
    if (language != null) {
      formFields['language'] = language;
    }

    final formData = FormData.fromMap(formFields);

    final response = await _dio.post(
      '/api/v1/transcribe',
      data: formData,
      options: Options(
        contentType: 'multipart/form-data',
        receiveTimeout: const Duration(minutes: 5),
        sendTimeout: const Duration(minutes: 2),
        headers: idempotencyKey == null
            ? null
            : {'Idempotency-Key': idempotencyKey},
      ),
      onSendProgress: (sent, total) {
        if (total > 0 && onProgress != null) {
          onProgress(sent / total);
        }
      },
    );

    return TranscriptResponse.fromJson(response.data as Map<String, dynamic>);
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

      final transcript = await service.transcribe(
        filePath,
        language: language,
        idempotencyKey: queuedUpload?.idempotencyKey,
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

      state = TranscriptionSuccess(transcript);
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
