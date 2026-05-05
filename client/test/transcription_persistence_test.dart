import 'dart:async';
import 'dart:io';

import 'package:dio/dio.dart';
import 'package:drift/drift.dart' show Value;
import 'package:drift/native.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:oracy/db/database.dart';
import 'package:oracy/services/api_client.dart';
import 'package:oracy/services/transcription_service.dart';
import 'package:oracy/services/upload_retry_policy.dart';

import 'helpers/test_utils.dart';

class _FailingTranscriptionService extends TranscriptionService {
  _FailingTranscriptionService() : super(Dio());

  @override
  Future<TranscriptionSubmissionResult> transcribe(
    String filePath, {
    String? language,
    required String idempotencyKey,
    required DateTime recordedAt,
    void Function(double progress)? onProgress,
  }) async {
    throw DioException(
      requestOptions: RequestOptions(path: '/api/v1/transcription-jobs'),
      type: DioExceptionType.connectionError,
    );
  }
}

class _SuccessfulTranscriptionService extends TranscriptionService {
  _SuccessfulTranscriptionService() : super(Dio());

  @override
  Future<TranscriptionSubmissionResult> transcribe(
    String filePath, {
    String? language,
    required String idempotencyKey,
    required DateTime recordedAt,
    void Function(double progress)? onProgress,
  }) async {
    return TranscriptionSubmissionVoiceNote(createMockVoiceNote());
  }
}

class _RecordingTimestampTranscriptionService extends TranscriptionService {
  _RecordingTimestampTranscriptionService() : super(Dio());

  final recordedAts = <DateTime>[];

  @override
  Future<TranscriptionSubmissionResult> transcribe(
    String filePath, {
    String? language,
    required String idempotencyKey,
    required DateTime recordedAt,
    void Function(double progress)? onProgress,
  }) async {
    recordedAts.add(recordedAt);
    return TranscriptionSubmissionVoiceNote(createMockVoiceNote());
  }
}

class _RetryableForegroundTranscriptionService extends TranscriptionService {
  _RetryableForegroundTranscriptionService() : super(Dio());

  int callCount = 0;
  final List<String> idempotencyKeys = [];
  final List<DateTime> recordedAts = [];

  @override
  Future<TranscriptionSubmissionResult> transcribe(
    String filePath, {
    String? language,
    required String idempotencyKey,
    required DateTime recordedAt,
    void Function(double progress)? onProgress,
  }) async {
    callCount++;
    idempotencyKeys.add(idempotencyKey);
    recordedAts.add(recordedAt);

    if (callCount == 1) {
      throw DioException(
        requestOptions: RequestOptions(path: '/api/v1/transcription-jobs'),
        type: DioExceptionType.receiveTimeout,
      );
    }

    return TranscriptionSubmissionVoiceNote(createMockVoiceNote());
  }
}

class _FreshAttemptForegroundTranscriptionService extends TranscriptionService {
  _FreshAttemptForegroundTranscriptionService() : super(Dio());

  int callCount = 0;
  final List<String> idempotencyKeys = [];
  final List<DateTime> recordedAts = [];

  @override
  Future<TranscriptionSubmissionResult> transcribe(
    String filePath, {
    String? language,
    required String idempotencyKey,
    required DateTime recordedAt,
    void Function(double progress)? onProgress,
  }) async {
    callCount++;
    idempotencyKeys.add(idempotencyKey);
    recordedAts.add(recordedAt);

    if (callCount == 1) {
      throw const TranscriptionClientException(
        UploadFailureClassification(
          message: 'Backend retries were exhausted.',
          errorType: TranscriptionErrorType.transcription,
          isRetryable: true,
          requiresFreshIdempotencyKey: true,
        ),
      );
    }

    return TranscriptionSubmissionVoiceNote(createMockVoiceNote());
  }
}

class _DeletedReplayTranscriptionService extends TranscriptionService {
  _DeletedReplayTranscriptionService() : super(Dio());

  @override
  Future<TranscriptionSubmissionResult> transcribe(
    String filePath, {
    String? language,
    required String idempotencyKey,
    required DateTime recordedAt,
    void Function(double progress)? onProgress,
  }) async {
    return const TranscriptionSubmissionAcceptedWithoutVoiceNote();
  }
}

class _ApiKeySensitiveTranscriptionService extends TranscriptionService {
  final SecureStorageService storage;
  final String validApiKey;

  _ApiKeySensitiveTranscriptionService({
    required this.storage,
    required this.validApiKey,
  }) : super(Dio());

  @override
  Future<TranscriptionSubmissionResult> transcribe(
    String filePath, {
    String? language,
    required String idempotencyKey,
    required DateTime recordedAt,
    void Function(double progress)? onProgress,
  }) async {
    final apiKey = await storage.getApiKey();
    if (apiKey != validApiKey) {
      throw DioException(
        requestOptions: RequestOptions(path: '/api/v1/transcription-jobs'),
        response: Response(
          requestOptions: RequestOptions(path: '/api/v1/transcription-jobs'),
          statusCode: 401,
          data: {'detail': 'invalid key'},
        ),
        type: DioExceptionType.badResponse,
      );
    }

    return TranscriptionSubmissionVoiceNote(createMockVoiceNote());
  }
}

class _StatusCodeFailingTranscriptionService extends TranscriptionService {
  final int statusCode;
  final String? detail;

  _StatusCodeFailingTranscriptionService(this.statusCode, {this.detail})
    : super(Dio());

  @override
  Future<TranscriptionSubmissionResult> transcribe(
    String filePath, {
    String? language,
    required String idempotencyKey,
    required DateTime recordedAt,
    void Function(double progress)? onProgress,
  }) async {
    throw DioException(
      requestOptions: RequestOptions(path: '/api/v1/transcription-jobs'),
      response: Response(
        requestOptions: RequestOptions(path: '/api/v1/transcription-jobs'),
        statusCode: statusCode,
        data: detail == null ? null : {'detail': detail},
      ),
      type: DioExceptionType.badResponse,
    );
  }
}

class _PlainTextResponseFailingTranscriptionService
    extends TranscriptionService {
  final int statusCode;
  final Object? responseData;

  _PlainTextResponseFailingTranscriptionService(
    this.statusCode, {
    this.responseData,
  }) : super(Dio());

  @override
  Future<TranscriptionSubmissionResult> transcribe(
    String filePath, {
    String? language,
    required String idempotencyKey,
    required DateTime recordedAt,
    void Function(double progress)? onProgress,
  }) async {
    throw DioException(
      requestOptions: RequestOptions(path: '/api/v1/transcription-jobs'),
      response: Response(
        requestOptions: RequestOptions(path: '/api/v1/transcription-jobs'),
        statusCode: statusCode,
        data: responseData,
      ),
      type: DioExceptionType.badResponse,
    );
  }
}

class _TimeoutTranscriptionService extends TranscriptionService {
  _TimeoutTranscriptionService() : super(Dio());

  @override
  Future<TranscriptionSubmissionResult> transcribe(
    String filePath, {
    String? language,
    required String idempotencyKey,
    required DateTime recordedAt,
    void Function(double progress)? onProgress,
  }) async {
    throw DioException(
      requestOptions: RequestOptions(path: '/api/v1/transcription-jobs'),
      type: DioExceptionType.connectionTimeout,
    );
  }
}

class _ThrowingTranscriptionService extends TranscriptionService {
  _ThrowingTranscriptionService() : super(Dio());

  @override
  Future<TranscriptionSubmissionResult> transcribe(
    String filePath, {
    String? language,
    required String idempotencyKey,
    required DateTime recordedAt,
    void Function(double progress)? onProgress,
  }) async {
    throw StateError('local file read failed');
  }
}

class _BlockingHasApiKeyStorage extends MockSecureStorage {
  _BlockingHasApiKeyStorage() : super(apiKey: 'oracy_sk_test');

  final Completer<void> started = Completer<void>();
  final Completer<void> unblock = Completer<void>();

  @override
  Future<bool> hasApiKey() async {
    if (!started.isCompleted) {
      started.complete();
    }

    await unblock.future;
    return super.hasApiKey();
  }
}

void main() {
  late Directory tempDir;
  late AppDatabase db;
  late ProviderContainer container;

  setUp(() async {
    tempDir = await Directory.systemTemp.createTemp('oracy_test_');
    db = AppDatabase(NativeDatabase.memory());
  });

  tearDown(() async {
    container.dispose();
    await db.close();
    if (await tempDir.exists()) {
      await tempDir.delete(recursive: true);
    }
  });

  Future<File> createAudioFile(String name) async {
    final file = File('${tempDir.path}/$name');
    await file.writeAsBytes(List<int>.filled(2048, 1));
    return file;
  }

  final testRecordingStartedAt = DateTime.utc(2026, 4, 21, 18, 29, 55);

  ProviderContainer createContainer(TranscriptionService service) {
    return ProviderContainer(
      overrides: [
        secureStorageProvider.overrideWith(
          (_) => MockSecureStorage(apiKey: 'oracy_sk_test'),
        ),
        appDatabaseProvider.overrideWithValue(db),
        transcriptionServiceProvider.overrideWith((_) => service),
      ],
    );
  }

  test(
    'Given foreground transcription starts, When the upload is claimed before network I/O, Then the row is uploading and excluded from the pending queue',
    () async {
      final audioFile = await createAudioFile('foreground_claim.wav');
      final storage = _BlockingHasApiKeyStorage();
      container = ProviderContainer(
        overrides: [
          secureStorageProvider.overrideWith((_) => storage),
          appDatabaseProvider.overrideWithValue(db),
          transcriptionServiceProvider.overrideWith(
            (_) => _SuccessfulTranscriptionService(),
          ),
        ],
      );

      final transcriptionFuture = container
          .read(transcriptionProvider.notifier)
          .transcribe(audioFile.path, recordedAt: testRecordingStartedAt);

      await storage.started.future;

      final pendingUploads = await db.getPendingUploads();
      final storedUpload = await db.getUploadByAudioPath(audioFile.path);

      expect(storedUpload, isNotNull);
      expect(storedUpload!.status, UploadStatus.uploading.index);
      expect(pendingUploads, isEmpty);

      storage.unblock.complete();
      await transcriptionFuture;

      expect(
        container.read(transcriptionProvider),
        isA<TranscriptionSuccess>(),
      );
    },
  );

  test(
    'Given no API key is configured, When foreground transcription claims the upload, Then the row is restored to pending and remains visible for retry',
    () async {
      final audioFile = await createAudioFile('missing_key.wav');
      container = ProviderContainer(
        overrides: [
          secureStorageProvider.overrideWith((_) => MockSecureStorage()),
          appDatabaseProvider.overrideWithValue(db),
          transcriptionServiceProvider.overrideWith(
            (_) => _SuccessfulTranscriptionService(),
          ),
        ],
      );

      await container
          .read(transcriptionProvider.notifier)
          .transcribe(audioFile.path, recordedAt: testRecordingStartedAt);

      final state = container.read(transcriptionProvider);
      final pendingUploads = await db.getPendingUploads();
      final storedUpload = await db.getUploadByAudioPath(audioFile.path);

      expect(state, isA<TranscriptionError>());
      expect(
        (state as TranscriptionError).errorType,
        TranscriptionErrorType.auth,
      );
      expect(pendingUploads, hasLength(1));
      expect(storedUpload, isNotNull);
      expect(storedUpload!.status, UploadStatus.pending.index);
      expect(storedUpload.retryCount, 0);
    },
  );

  test(
    'Given transcription fails, When audio was recorded, Then audio remains queued for retry',
    () async {
      final audioFile = await createAudioFile('failed.wav');
      container = createContainer(_FailingTranscriptionService());

      await container
          .read(transcriptionProvider.notifier)
          .transcribe(audioFile.path, recordedAt: testRecordingStartedAt);

      final state = container.read(transcriptionProvider);
      final pendingUploads = await db.getPendingUploads();

      expect(state, isA<TranscriptionError>());
      expect(pendingUploads, hasLength(1));
      expect(pendingUploads.single.audioPath, audioFile.path);
      expect(pendingUploads.single.status, UploadStatus.failed.index);
      expect(await audioFile.exists(), isTrue);
    },
  );

  test(
    'Given transcription returns unauthorized, When audio was recorded, Then audio remains failed and retryable',
    () async {
      final audioFile = await createAudioFile('unauthorized.wav');
      container = createContainer(_StatusCodeFailingTranscriptionService(401));

      await container
          .read(transcriptionProvider.notifier)
          .transcribe(audioFile.path, recordedAt: testRecordingStartedAt);

      final state = container.read(transcriptionProvider);
      final pendingUploads = await db.getPendingUploads();
      final storedUpload = await db.getUploadByAudioPath(audioFile.path);

      expect(state, isA<TranscriptionError>());
      expect((state as TranscriptionError).isRetryable, isTrue);
      expect(pendingUploads, hasLength(1));
      expect(storedUpload, isNotNull);
      expect(storedUpload!.status, UploadStatus.failed.index);
      expect(storedUpload.retryCount, 1);
      expect(pendingUploads.single.id, storedUpload.id);
      expect(await audioFile.exists(), isTrue);
    },
  );

  test(
    'Given transcription returns a plain-text server error, When audio was recorded, Then the failure is recorded without leaving the row uploading',
    () async {
      final audioFile = await createAudioFile('plain_text_502.wav');
      container = createContainer(
        _PlainTextResponseFailingTranscriptionService(
          502,
          responseData: 'bad gateway',
        ),
      );

      await expectLater(
        container
            .read(transcriptionProvider.notifier)
            .transcribe(audioFile.path, recordedAt: testRecordingStartedAt),
        completes,
      );

      final state = container.read(transcriptionProvider);
      final storedUpload = await db.getUploadByAudioPath(audioFile.path);

      expect(state, isA<TranscriptionError>());
      expect(storedUpload, isNotNull);
      expect(storedUpload!.status, UploadStatus.failed.index);
      expect(storedUpload.retryCount, 1);
      expect(
        storedUpload.errorMessage,
        'Transcription service error. Please try again later.',
      );
      expect(await audioFile.exists(), isTrue);
    },
  );

  test(
    'Given authentication fails first, When the user fixes the API key and retries, Then the preserved upload succeeds',
    () async {
      const validApiKey = 'oracy_sk_valid';
      final storage = MockSecureStorage(apiKey: 'oracy_sk_invalid');
      final audioFile = await createAudioFile('auth_retry.wav');
      container = ProviderContainer(
        overrides: [
          secureStorageProvider.overrideWith((_) => storage),
          appDatabaseProvider.overrideWithValue(db),
          transcriptionServiceProvider.overrideWith(
            (_) => _ApiKeySensitiveTranscriptionService(
              storage: storage,
              validApiKey: validApiKey,
            ),
          ),
        ],
      );

      await container
          .read(transcriptionProvider.notifier)
          .transcribe(audioFile.path, recordedAt: testRecordingStartedAt);

      final failedUpload = await db.getUploadByAudioPath(audioFile.path);
      final pendingAfterFailure = await db.getPendingUploads();

      expect(container.read(transcriptionProvider), isA<TranscriptionError>());
      expect(failedUpload, isNotNull);
      expect(failedUpload!.status, UploadStatus.failed.index);
      expect(failedUpload.retryCount, 1);
      expect(
        pendingAfterFailure.map((upload) => upload.id),
        contains(failedUpload.id),
      );
      expect(await audioFile.exists(), isTrue);

      await storage.setApiKey(validApiKey);

      final retried = await container
          .read(transcriptionProvider.notifier)
          .retry();

      expect(retried, isTrue);
      expect(
        container.read(transcriptionProvider),
        isA<TranscriptionSuccess>(),
      );
      expect(await db.getUploadByAudioPath(audioFile.path), isNull);
      expect(await db.getPendingUploads(), isEmpty);
      expect(await audioFile.exists(), isFalse);
    },
  );

  test(
    'Given foreground transcription is not queued, When a retryable failure is retried, Then the retry reuses the same idempotency key and recorded timestamp',
    () async {
      final audioFile = await createAudioFile('web_retry.wav');
      final service = _RetryableForegroundTranscriptionService();
      container = ProviderContainer(
        overrides: [
          secureStorageProvider.overrideWith(
            (_) => MockSecureStorage(apiKey: 'oracy_sk_test'),
          ),
          appDatabaseProvider.overrideWithValue(db),
          transcriptionServiceProvider.overrideWith((_) => service),
          foregroundTranscriptionsAreQueuedProvider.overrideWithValue(false),
        ],
      );

      await container
          .read(transcriptionProvider.notifier)
          .transcribe(
            audioFile.path,
            language: 'en',
            recordedAt: testRecordingStartedAt,
          );

      expect(container.read(transcriptionProvider), isA<TranscriptionError>());
      expect(service.idempotencyKeys, hasLength(1));
      expect(await db.getUploadByAudioPath(audioFile.path), isNull);

      final retried = await container
          .read(transcriptionProvider.notifier)
          .retry();

      expect(retried, isTrue);
      expect(
        container.read(transcriptionProvider),
        isA<TranscriptionSuccess>(),
      );
      expect(service.idempotencyKeys, hasLength(2));
      expect(service.idempotencyKeys[1], service.idempotencyKeys[0]);
      expect(service.recordedAts[1], service.recordedAts[0]);
    },
  );

  test(
    'Given foreground transcription is not queued, When a recording start timestamp is supplied, Then the open request uses the recording start time',
    () async {
      final audioFile = await createAudioFile('web_recording.wav');
      final recordingStartedAt = DateTime.utc(2026, 4, 21, 18, 29, 55);
      final service = _RecordingTimestampTranscriptionService();
      container = ProviderContainer(
        overrides: [
          secureStorageProvider.overrideWith(
            (_) => MockSecureStorage(apiKey: 'oracy_sk_test'),
          ),
          appDatabaseProvider.overrideWithValue(db),
          transcriptionServiceProvider.overrideWith((_) => service),
          foregroundTranscriptionsAreQueuedProvider.overrideWithValue(false),
        ],
      );

      await container
          .read(transcriptionProvider.notifier)
          .transcribe(audioFile.path, recordedAt: recordingStartedAt);

      expect(service.recordedAts, [recordingStartedAt]);
    },
  );

  test(
    'Given a queued native recording filename contains a capture timestamp, When recordedAt is derived, Then the filename timestamp wins over queue creation time',
    () {
      container = ProviderContainer();
      final filenameTimestamp = DateTime.utc(2026, 4, 21, 18, 29, 55);
      final upload = PendingUpload(
        id: 1,
        audioPath:
            '${tempDir.path}/oracy_recording_${filenameTimestamp.millisecondsSinceEpoch}.wav',
        createdAt: DateTime.utc(2026, 4, 21, 18, 34, 55),
        retryCount: 0,
        status: UploadStatus.pending.index,
        idempotencyKey: 'stable-key',
      );

      expect(recordedAtForQueuedUpload(upload), filenameTimestamp);
    },
  );

  test(
    'Given a recovered queued native recording filename contains a capture timestamp, When recordedAt is derived, Then the filename timestamp wins over queue creation time',
    () {
      container = ProviderContainer();
      final filenameTimestamp = DateTime.utc(2026, 4, 21, 18, 29, 55);
      final upload = PendingUpload(
        id: 1,
        audioPath:
            '${tempDir.path}/oracy_recording_${filenameTimestamp.millisecondsSinceEpoch}_recovered.wav',
        createdAt: DateTime.utc(2026, 4, 21, 18, 34, 55),
        retryCount: 0,
        status: UploadStatus.pending.index,
        idempotencyKey: 'stable-key',
      );

      expect(recordedAtForQueuedUpload(upload), filenameTimestamp);
    },
  );

  test(
    'Given a queued recording filename has no capture timestamp, When recordedAt is derived, Then queue creation time is used',
    () {
      container = ProviderContainer();
      final upload = PendingUpload(
        id: 1,
        audioPath: '${tempDir.path}/oracy_recording_orphaned.wav',
        createdAt: DateTime.utc(2026, 4, 21, 18, 34, 55),
        retryCount: 0,
        status: UploadStatus.pending.index,
        idempotencyKey: 'stable-key',
      );

      expect(recordedAtForQueuedUpload(upload), upload.createdAt.toUtc());
    },
  );

  test(
    'Given foreground terminal job failure asks for a fresh attempt, When the user retries, Then only the idempotency key changes',
    () async {
      final audioFile = await createAudioFile('fresh_foreground.wav');
      final service = _FreshAttemptForegroundTranscriptionService();
      container = ProviderContainer(
        overrides: [
          secureStorageProvider.overrideWith(
            (_) => MockSecureStorage(apiKey: 'oracy_sk_test'),
          ),
          appDatabaseProvider.overrideWithValue(db),
          transcriptionServiceProvider.overrideWith((_) => service),
          foregroundTranscriptionsAreQueuedProvider.overrideWithValue(false),
        ],
      );

      await container
          .read(transcriptionProvider.notifier)
          .transcribe(audioFile.path, recordedAt: testRecordingStartedAt);

      expect(container.read(transcriptionProvider), isA<TranscriptionError>());

      final retried = await container
          .read(transcriptionProvider.notifier)
          .retry();

      expect(retried, isTrue);
      expect(
        container.read(transcriptionProvider),
        isA<TranscriptionSuccess>(),
      );
      expect(service.idempotencyKeys, hasLength(2));
      expect(service.idempotencyKeys[1], isNot(service.idempotencyKeys[0]));
      expect(service.recordedAts[1], service.recordedAts[0]);
    },
  );

  test(
    'Given foreground replay accepted server work whose voice note was deleted, When the user dismisses it, Then transcription returns to idle and cannot retry the old attempt',
    () async {
      final audioFile = await createAudioFile('deleted_replay.wav');
      container = ProviderContainer(
        overrides: [
          secureStorageProvider.overrideWith(
            (_) => MockSecureStorage(apiKey: 'oracy_sk_test'),
          ),
          appDatabaseProvider.overrideWithValue(db),
          transcriptionServiceProvider.overrideWith(
            (_) => _DeletedReplayTranscriptionService(),
          ),
          foregroundTranscriptionsAreQueuedProvider.overrideWithValue(false),
        ],
      );

      await container
          .read(transcriptionProvider.notifier)
          .transcribe(audioFile.path, recordedAt: testRecordingStartedAt);

      expect(
        container.read(transcriptionProvider),
        isA<TranscriptionVoiceNoteDeleted>(),
      );

      container.read(transcriptionProvider.notifier).reset();

      expect(container.read(transcriptionProvider), isA<TranscriptionIdle>());
      expect(
        await container.read(transcriptionProvider.notifier).retry(),
        isFalse,
      );
    },
  );

  test(
    'Given transcription returns file too large, When audio was recorded, Then audio becomes terminal and excluded from retries',
    () async {
      final audioFile = await createAudioFile('too_large.wav');
      container = createContainer(_StatusCodeFailingTranscriptionService(413));

      await container
          .read(transcriptionProvider.notifier)
          .transcribe(audioFile.path, recordedAt: testRecordingStartedAt);

      final state = container.read(transcriptionProvider);
      final pendingUploads = await db.getPendingUploads();
      final storedUpload = await db.getUploadByAudioPath(audioFile.path);

      expect(state, isA<TranscriptionError>());
      expect((state as TranscriptionError).isRetryable, isFalse);
      expect(pendingUploads, isEmpty);
      expect(storedUpload, isNotNull);
      expect(storedUpload!.status, UploadStatus.terminalFailure.index);
      expect(await audioFile.exists(), isTrue);
    },
  );

  test(
    'Given transcription returns unsupported media type, When audio was recorded, Then audio becomes terminal and excluded from retries',
    () async {
      final audioFile = await createAudioFile('unsupported.wav');
      container = createContainer(_StatusCodeFailingTranscriptionService(415));

      await container
          .read(transcriptionProvider.notifier)
          .transcribe(audioFile.path, recordedAt: testRecordingStartedAt);

      final state = container.read(transcriptionProvider);
      final pendingUploads = await db.getPendingUploads();
      final storedUpload = await db.getUploadByAudioPath(audioFile.path);

      expect(state, isA<TranscriptionError>());
      expect((state as TranscriptionError).isRetryable, isFalse);
      expect(pendingUploads, isEmpty);
      expect(storedUpload, isNotNull);
      expect(storedUpload!.status, UploadStatus.terminalFailure.index);
      expect(await audioFile.exists(), isTrue);
    },
  );

  test(
    'Given transcription times out, When audio was recorded, Then audio remains retryable',
    () async {
      final audioFile = await createAudioFile('timeout.wav');
      container = createContainer(_TimeoutTranscriptionService());

      await container
          .read(transcriptionProvider.notifier)
          .transcribe(audioFile.path, recordedAt: testRecordingStartedAt);

      final state = container.read(transcriptionProvider);
      final pendingUploads = await db.getPendingUploads();
      final storedUpload = await db.getUploadByAudioPath(audioFile.path);

      expect(state, isA<TranscriptionError>());
      expect((state as TranscriptionError).isRetryable, isTrue);
      expect(pendingUploads, hasLength(1));
      expect(storedUpload, isNotNull);
      expect(storedUpload!.status, UploadStatus.failed.index);
      expect(await audioFile.exists(), isTrue);
    },
  );

  test(
    'Given transcription returns a server error, When audio was recorded, Then audio remains retryable',
    () async {
      final audioFile = await createAudioFile('server_error.wav');
      container = createContainer(
        _StatusCodeFailingTranscriptionService(502, detail: 'try later'),
      );

      await container
          .read(transcriptionProvider.notifier)
          .transcribe(audioFile.path, recordedAt: testRecordingStartedAt);

      final state = container.read(transcriptionProvider);
      final pendingUploads = await db.getPendingUploads();
      final storedUpload = await db.getUploadByAudioPath(audioFile.path);

      expect(state, isA<TranscriptionError>());
      expect((state as TranscriptionError).isRetryable, isTrue);
      expect(pendingUploads, hasLength(1));
      expect(storedUpload, isNotNull);
      expect(storedUpload!.status, UploadStatus.failed.index);
      expect(await audioFile.exists(), isTrue);
    },
  );

  test(
    'Given transcription fails before any server response, When audio was recorded, Then audio becomes terminal and non-retryable',
    () async {
      final audioFile = await createAudioFile('local_failure.wav');
      container = createContainer(_ThrowingTranscriptionService());

      await container
          .read(transcriptionProvider.notifier)
          .transcribe(audioFile.path, recordedAt: testRecordingStartedAt);

      final state = container.read(transcriptionProvider);
      final pendingUploads = await db.getPendingUploads();
      final storedUpload = await db.getUploadByAudioPath(audioFile.path);

      expect(state, isA<TranscriptionError>());
      expect((state as TranscriptionError).isRetryable, isFalse);
      expect(pendingUploads, isEmpty);
      expect(storedUpload, isNotNull);
      expect(storedUpload!.status, UploadStatus.terminalFailure.index);
      expect(await audioFile.exists(), isTrue);
    },
  );

  test(
    'Given transcription succeeds, When audio was recorded, Then queued audio is deleted',
    () async {
      final audioFile = await createAudioFile('successful.wav');
      container = createContainer(_SuccessfulTranscriptionService());

      await container
          .read(transcriptionProvider.notifier)
          .transcribe(audioFile.path, recordedAt: testRecordingStartedAt);

      final state = container.read(transcriptionProvider);
      final pendingUploads = await db.getPendingUploads();

      expect(state, isA<TranscriptionSuccess>());
      expect(pendingUploads, isEmpty);
      expect(await audioFile.exists(), isFalse);
    },
  );

  test(
    'Given transcription succeeds but local cleanup fails, When audio was recorded, Then upload cleanup remains pending without requeueing',
    () async {
      final audioFile = await createAudioFile('cleanup_pending.wav');
      container = ProviderContainer(
        overrides: [
          secureStorageProvider.overrideWith(
            (_) => MockSecureStorage(apiKey: 'oracy_sk_test'),
          ),
          appDatabaseProvider.overrideWithValue(db),
          transcriptionServiceProvider.overrideWith(
            (_) => _SuccessfulTranscriptionService(),
          ),
          localFileDeleterProvider.overrideWithValue((String _) async {
            throw const FileSystemException('simulated delete failure');
          }),
        ],
      );

      await container
          .read(transcriptionProvider.notifier)
          .transcribe(audioFile.path, recordedAt: testRecordingStartedAt);

      final state = container.read(transcriptionProvider);
      final pendingUploads = await db.getPendingUploads();
      final storedUpload = await db.getUploadByAudioPath(audioFile.path);

      expect(state, isA<TranscriptionSuccess>());
      expect(pendingUploads, isEmpty);
      expect(storedUpload, isNotNull);
      expect(storedUpload!.status, UploadStatus.cleanupPending.index);
      expect(await audioFile.exists(), isTrue);
    },
  );

  test(
    'Given a terminal failure is preserved, When sync counts unsynced recordings, Then terminal failures remain visible',
    () async {
      final audioFile = await createAudioFile('terminal_visible.wav');
      final uploadId = await db.ensurePendingUpload(audioPath: audioFile.path);
      await db.markAsTerminalFailure(
        uploadId,
        errorMessage: 'Unsupported audio format.',
      );

      expect(await db.getPendingCount(), 1);
      expect(await db.watchPendingCount().first, 1);
    },
  );

  test(
    'Given a retryable upload has exhausted automatic retries, When another retryable failure is recorded, Then it becomes a terminal failure for manual recovery',
    () async {
      container = ProviderContainer();
      final audioFile = await createAudioFile('exhausted_retry.wav');
      final uploadId = await db.ensurePendingUpload(audioPath: audioFile.path);
      await (db.update(
        db.pendingUploads,
      )..where((t) => t.id.equals(uploadId))).write(
        PendingUploadsCompanion(
          status: Value(UploadStatus.failed.index),
          retryCount: Value(maxRetryAttempts - 1),
        ),
      );

      await recordUploadFailure(
        db,
        uploadId,
        const UploadFailureClassification(
          message: 'Unable to connect to server.',
          errorType: TranscriptionErrorType.network,
          isRetryable: true,
        ),
      );

      final storedUpload = await db.getUploadById(uploadId);

      expect(storedUpload, isNotNull);
      expect(storedUpload!.status, UploadStatus.terminalFailure.index);
      expect(storedUpload.retryCount, maxRetryAttempts);
      expect(
        storedUpload.errorMessage,
        contains('Automatic retries exhausted'),
      );
      expect(await db.getPendingUploads(), isEmpty);
      expect(await db.getPendingCount(), 1);
      expect(await db.watchTerminalFailureUploads().first, hasLength(1));
    },
  );

  test(
    'Given a terminal job failure is retryable by the client, When the failure is recorded, Then the queued upload receives a fresh idempotency key',
    () async {
      container = ProviderContainer();
      final audioFile = await createAudioFile('fresh_attempt.wav');
      final uploadId = await db.ensurePendingUpload(audioPath: audioFile.path);
      final originalUpload = await db.getUploadById(uploadId);

      await recordUploadFailure(
        db,
        uploadId,
        const UploadFailureClassification(
          message: 'Backend retries were exhausted.',
          errorType: TranscriptionErrorType.transcription,
          isRetryable: true,
          requiresFreshIdempotencyKey: true,
        ),
      );

      final failedUpload = await db.getUploadById(uploadId);

      expect(originalUpload, isNotNull);
      expect(failedUpload, isNotNull);
      expect(failedUpload!.status, UploadStatus.failed.index);
      expect(
        failedUpload.idempotencyKey,
        isNot(originalUpload!.idempotencyKey),
      );
      expect(
        failedUpload.idempotencyKey,
        matches(
          RegExp(
            r'^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$',
          ),
        ),
      );
    },
  );
}
