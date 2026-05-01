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
  Future<VoiceNoteResponse> transcribe(
    String filePath, {
    String? language,
    String? idempotencyKey,
    void Function(double progress)? onProgress,
  }) async {
    throw DioException(
      requestOptions: RequestOptions(path: '/api/v1/transcribe'),
      type: DioExceptionType.connectionError,
    );
  }
}

class _SuccessfulTranscriptionService extends TranscriptionService {
  _SuccessfulTranscriptionService() : super(Dio());

  @override
  Future<VoiceNoteResponse> transcribe(
    String filePath, {
    String? language,
    String? idempotencyKey,
    void Function(double progress)? onProgress,
  }) async {
    return createMockVoiceNote();
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
  Future<VoiceNoteResponse> transcribe(
    String filePath, {
    String? language,
    String? idempotencyKey,
    void Function(double progress)? onProgress,
  }) async {
    final apiKey = await storage.getApiKey();
    if (apiKey != validApiKey) {
      throw DioException(
        requestOptions: RequestOptions(path: '/api/v1/transcribe'),
        response: Response(
          requestOptions: RequestOptions(path: '/api/v1/transcribe'),
          statusCode: 401,
          data: {'detail': 'invalid key'},
        ),
        type: DioExceptionType.badResponse,
      );
    }

    return createMockVoiceNote();
  }
}

class _StatusCodeFailingTranscriptionService extends TranscriptionService {
  final int statusCode;
  final String? detail;

  _StatusCodeFailingTranscriptionService(this.statusCode, {this.detail})
    : super(Dio());

  @override
  Future<VoiceNoteResponse> transcribe(
    String filePath, {
    String? language,
    String? idempotencyKey,
    void Function(double progress)? onProgress,
  }) async {
    throw DioException(
      requestOptions: RequestOptions(path: '/api/v1/transcribe'),
      response: Response(
        requestOptions: RequestOptions(path: '/api/v1/transcribe'),
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
  Future<VoiceNoteResponse> transcribe(
    String filePath, {
    String? language,
    String? idempotencyKey,
    void Function(double progress)? onProgress,
  }) async {
    throw DioException(
      requestOptions: RequestOptions(path: '/api/v1/transcribe'),
      response: Response(
        requestOptions: RequestOptions(path: '/api/v1/transcribe'),
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
  Future<VoiceNoteResponse> transcribe(
    String filePath, {
    String? language,
    String? idempotencyKey,
    void Function(double progress)? onProgress,
  }) async {
    throw DioException(
      requestOptions: RequestOptions(path: '/api/v1/transcribe'),
      type: DioExceptionType.connectionTimeout,
    );
  }
}

class _ThrowingTranscriptionService extends TranscriptionService {
  _ThrowingTranscriptionService() : super(Dio());

  @override
  Future<VoiceNoteResponse> transcribe(
    String filePath, {
    String? language,
    String? idempotencyKey,
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
          .transcribe(audioFile.path);

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
          .transcribe(audioFile.path);

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
          .transcribe(audioFile.path);

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
          .transcribe(audioFile.path);

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
            .transcribe(audioFile.path),
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
          .transcribe(audioFile.path);

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
    'Given transcription returns file too large, When audio was recorded, Then audio becomes terminal and excluded from retries',
    () async {
      final audioFile = await createAudioFile('too_large.wav');
      container = createContainer(_StatusCodeFailingTranscriptionService(413));

      await container
          .read(transcriptionProvider.notifier)
          .transcribe(audioFile.path);

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
          .transcribe(audioFile.path);

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
          .transcribe(audioFile.path);

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
          .transcribe(audioFile.path);

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
          .transcribe(audioFile.path);

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
          .transcribe(audioFile.path);

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
          .transcribe(audioFile.path);

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
}
