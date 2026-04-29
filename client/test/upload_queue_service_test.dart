import 'dart:async';
import 'dart:io';

import 'package:connectivity_plus/connectivity_plus.dart';
import 'package:connectivity_plus_platform_interface/connectivity_plus_platform_interface.dart';
import 'package:dio/dio.dart';
import 'package:drift/drift.dart' show Value;
import 'package:drift/native.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:oracy/db/database.dart';
import 'package:oracy/services/api_client.dart';
import 'package:oracy/services/transcription_service.dart';
import 'package:oracy/services/upload_queue_service.dart';

import 'helpers/test_utils.dart';

class _FakeConnectivityPlatform extends ConnectivityPlatform {
  _FakeConnectivityPlatform(this._results);

  final List<ConnectivityResult> _results;
  final StreamController<List<ConnectivityResult>> _controller =
      StreamController<List<ConnectivityResult>>.broadcast();

  @override
  Future<List<ConnectivityResult>> checkConnectivity() async => _results;

  @override
  Stream<List<ConnectivityResult>> get onConnectivityChanged =>
      _controller.stream;

  Future<void> dispose() async {
    await _controller.close();
  }
}

class _CountingTranscriptionService extends TranscriptionService {
  _CountingTranscriptionService() : super(Dio());

  int callCount = 0;
  final List<String?> idempotencyKeys = [];

  @override
  Future<TranscriptResponse> transcribe(
    String filePath, {
    String? language,
    String? idempotencyKey,
    void Function(double progress)? onProgress,
  }) async {
    callCount++;
    idempotencyKeys.add(idempotencyKey);
    return createMockTranscript();
  }
}

class _RetryingTranscriptionService extends TranscriptionService {
  _RetryingTranscriptionService() : super(Dio());

  int callCount = 0;
  final List<String?> idempotencyKeys = [];

  @override
  Future<TranscriptResponse> transcribe(
    String filePath, {
    String? language,
    String? idempotencyKey,
    void Function(double progress)? onProgress,
  }) async {
    callCount++;
    idempotencyKeys.add(idempotencyKey);

    if (callCount == 1) {
      throw DioException(
        requestOptions: RequestOptions(path: '/api/v1/transcribe'),
        type: DioExceptionType.connectionError,
      );
    }

    return createMockTranscript();
  }
}

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  const pathProviderChannel = MethodChannel('plugins.flutter.io/path_provider');

  late Directory tempDir;
  late AppDatabase db;
  late ConnectivityPlatform originalConnectivityPlatform;
  late _FakeConnectivityPlatform fakeConnectivityPlatform;

  setUp(() async {
    tempDir = await Directory.systemTemp.createTemp('oracy_upload_queue_test_');
    db = AppDatabase(NativeDatabase.memory());
    originalConnectivityPlatform = ConnectivityPlatform.instance;
    fakeConnectivityPlatform = _FakeConnectivityPlatform([
      ConnectivityResult.wifi,
    ]);
    ConnectivityPlatform.instance = fakeConnectivityPlatform;
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(pathProviderChannel, (methodCall) async {
          if (methodCall.method == 'getApplicationSupportDirectory') {
            return tempDir.path;
          }
          return null;
        });
  });

  tearDown(() async {
    ConnectivityPlatform.instance = originalConnectivityPlatform;
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(pathProviderChannel, null);
    await fakeConnectivityPlatform.dispose();
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

  Future<void> settleStartupWork() async {
    for (var i = 0; i < 10; i++) {
      await Future<void>.delayed(Duration.zero);
    }
  }

  Future<void> setConnectivityResults(List<ConnectivityResult> results) async {
    await fakeConnectivityPlatform.dispose();
    fakeConnectivityPlatform = _FakeConnectivityPlatform(results);
    ConnectivityPlatform.instance = fakeConnectivityPlatform;
  }

  test(
    'Given no API key is configured, When the foreground upload queue starts, Then pending audio is not uploaded',
    () async {
      final audioFile = await createAudioFile('missing_api_key.wav');
      await db.ensurePendingUpload(audioPath: audioFile.path);
      final storage = MockSecureStorage();
      final transcriptionService = _CountingTranscriptionService();
      final container = ProviderContainer(
        overrides: [
          secureStorageProvider.overrideWith((_) => storage),
          appDatabaseProvider.overrideWithValue(db),
          transcriptionServiceProvider.overrideWith(
            (_) => transcriptionService,
          ),
        ],
      );
      addTearDown(container.dispose);

      final service = container.read(uploadQueueServiceProvider);
      await settleStartupWork();

      expect(service, isNotNull);
      expect(transcriptionService.callCount, 0);
      expect(await db.getUploadByAudioPath(audioFile.path), isNotNull);
      expect(await audioFile.exists(), isTrue);
    },
  );

  test(
    'Given queued audio was held for configuration, When an API key becomes configured, Then the foreground upload queue resumes',
    () async {
      final audioFile = await createAudioFile('configured_later.wav');
      await db.ensurePendingUpload(audioPath: audioFile.path);
      final storage = MockSecureStorage();
      final transcriptionService = _CountingTranscriptionService();
      final container = ProviderContainer(
        overrides: [
          secureStorageProvider.overrideWith((_) => storage),
          appDatabaseProvider.overrideWithValue(db),
          transcriptionServiceProvider.overrideWith(
            (_) => transcriptionService,
          ),
        ],
      );
      addTearDown(container.dispose);

      container.read(uploadQueueServiceProvider);
      await settleStartupWork();

      expect(transcriptionService.callCount, 0);

      await storage.setApiKey('oracy_sk_test');
      container.invalidate(hasApiKeyProvider);
      container.read(uploadQueueServiceProvider);
      await settleStartupWork();

      expect(transcriptionService.callCount, 1);
      expect(await db.getUploadByAudioPath(audioFile.path), isNull);
      expect(await audioFile.exists(), isFalse);
    },
  );

  test(
    'Given an existing queued row is missing its idempotency key, When the same upload is queued again, Then queueing fails loudly instead of silently backfilling it',
    () async {
      final audioFile = await createAudioFile('legacy_backfill.wav');
      final uploadId = await db.addPendingUpload(audioPath: audioFile.path);

      await (db.update(
        db.pendingUploads,
      )..where((t) => t.id.equals(uploadId))).write(
        PendingUploadsCompanion(
          status: Value(UploadStatus.failed.index),
          retryCount: Value(2),
          errorMessage: Value('previous failure'),
          idempotencyKey: const Value(null),
        ),
      );

      final storedUpload = await db.getUploadById(uploadId);

      expect(storedUpload, isNotNull);
      expect(
        () => db.ensurePendingUpload(audioPath: audioFile.path),
        throwsA(isA<StateError>()),
      );
      expect(storedUpload!.idempotencyKey, isNull);
      expect(await db.getPendingUploads(), hasLength(1));
    },
  );

  test(
    'Given the same audio path is inserted twice, When the second row is added, Then the database rejects the duplicate queue entry',
    () async {
      final audioFile = await createAudioFile('duplicate_insert.wav');

      await db.addPendingUpload(
        audioPath: audioFile.path,
        idempotencyKey: 'first-key',
      );

      expect(
        () => db.addPendingUpload(
          audioPath: audioFile.path,
          idempotencyKey: 'second-key',
        ),
        throwsA(isA<SqliteException>()),
      );
    },
  );

  test(
    'Given foreground startup, When an uploading row was updated recently, Then it remains uploading and excluded from the pending queue',
    () async {
      await setConnectivityResults([ConnectivityResult.none]);

      final uploadingFile = await createAudioFile('recent_startup.wav');
      final uploadingId = await db.ensurePendingUpload(
        audioPath: uploadingFile.path,
      );
      await db.markAsUploading(uploadingId);

      await (db.update(
        db.pendingUploads,
      )..where((t) => t.id.equals(uploadingId))).write(
        PendingUploadsCompanion(
          updatedAt: Value(DateTime.now().subtract(Duration(minutes: 5))),
        ),
      );

      final service = UploadQueueService(
        db: db,
        transcriptionService: _CountingTranscriptionService(),
        connectivity: Connectivity(),
      );

      await service.start();
      await settleStartupWork();
      service.stop();

      final uploadingRow = await db.getUploadByAudioPath(uploadingFile.path);
      final pendingUploads = await db.getPendingUploads();

      expect(uploadingRow, isNotNull);
      expect(uploadingRow!.status, UploadStatus.uploading.index);
      expect(
        pendingUploads.map((upload) => upload.id),
        isNot(contains(uploadingId)),
      );
      expect(await uploadingFile.exists(), isTrue);
    },
  );

  test(
    'Given foreground startup, When an uploading row is stale, Then it is restored to failed and returned to the pending queue',
    () async {
      await setConnectivityResults([ConnectivityResult.none]);

      final uploadingFile = await createAudioFile('stale_startup.wav');
      final uploadingId = await db.ensurePendingUpload(
        audioPath: uploadingFile.path,
      );
      await db.markAsUploading(uploadingId);

      await (db.update(
        db.pendingUploads,
      )..where((t) => t.id.equals(uploadingId))).write(
        PendingUploadsCompanion(
          updatedAt: Value(DateTime.now().subtract(Duration(minutes: 11))),
        ),
      );

      final service = UploadQueueService(
        db: db,
        transcriptionService: _CountingTranscriptionService(),
        connectivity: Connectivity(),
      );

      await service.start();
      await settleStartupWork();
      service.stop();

      final uploadingRow = await db.getUploadByAudioPath(uploadingFile.path);
      final pendingUploads = await db.getPendingUploads();

      expect(uploadingRow, isNotNull);
      expect(uploadingRow!.status, UploadStatus.failed.index);
      expect(pendingUploads.map((upload) => upload.id), contains(uploadingId));
      expect(await uploadingFile.exists(), isTrue);
    },
  );

  test(
    'Given upload cleanup is pending, When queue processing runs again, Then local cleanup retries without reuploading audio',
    () async {
      final audioFile = await createAudioFile('pending_cleanup.wav');
      final transcriptionService = _CountingTranscriptionService();
      final firstService = UploadQueueService(
        db: db,
        transcriptionService: transcriptionService,
        connectivity: Connectivity(),
        deleteLocalFile: (String _) async {
          throw const FileSystemException('simulated delete failure');
        },
      );

      await firstService.queueUpload(audioFile.path);
      await firstService.processQueue();

      final cleanupPendingUpload = await db.getUploadByAudioPath(
        audioFile.path,
      );
      expect(cleanupPendingUpload, isNotNull);
      expect(cleanupPendingUpload!.status, UploadStatus.cleanupPending.index);
      expect(transcriptionService.callCount, 1);
      expect(await audioFile.exists(), isTrue);

      final secondService = UploadQueueService(
        db: db,
        transcriptionService: transcriptionService,
        connectivity: Connectivity(),
        deleteLocalFile: (String filePath) async {
          await File(filePath).delete();
        },
      );

      await secondService.processQueue();

      expect(await db.getUploadByAudioPath(audioFile.path), isNull);
      expect(transcriptionService.callCount, 1);
      expect(await audioFile.exists(), isFalse);
    },
  );

  test(
    'Given a terminal failure row, When the user deletes it, Then the local file and queue row are both removed',
    () async {
      final audioFile = await createAudioFile('terminal_delete.wav');
      final uploadId = await db.ensurePendingUpload(audioPath: audioFile.path);
      await db.markAsTerminalFailure(
        uploadId,
        errorMessage: 'Unsupported audio format.',
      );

      final service = UploadQueueService(
        db: db,
        transcriptionService: _CountingTranscriptionService(),
        connectivity: Connectivity(),
        deleteLocalFile: (String filePath) async {
          await File(filePath).delete();
        },
      );

      final deleted = await service.deleteTerminalFailureRecording(uploadId);

      expect(deleted, isTrue);
      expect(await db.getUploadByAudioPath(audioFile.path), isNull);
      expect(await audioFile.exists(), isFalse);
    },
  );

  test(
    'Given a terminal failure row, When deleting the local file fails, Then the queue row remains for manual attention',
    () async {
      final audioFile = await createAudioFile('terminal_delete_failure.wav');
      final uploadId = await db.ensurePendingUpload(audioPath: audioFile.path);
      await db.markAsTerminalFailure(
        uploadId,
        errorMessage: 'Audio file is too large (max 25MB).',
      );

      final service = UploadQueueService(
        db: db,
        transcriptionService: _CountingTranscriptionService(),
        connectivity: Connectivity(),
        deleteLocalFile: (String _) async {
          throw const FileSystemException('simulated delete failure');
        },
      );

      expect(
        () => service.deleteTerminalFailureRecording(uploadId),
        throwsA(isA<FileSystemException>()),
      );
      expect(await db.getUploadByAudioPath(audioFile.path), isNotNull);
      expect(await audioFile.exists(), isTrue);
    },
  );

  test(
    'Given the same recording is queued and retried twice, When queue processing uploads it again, Then the queue entry and idempotency key stay stable across retries',
    () async {
      final audioFile = await createAudioFile('stable_idempotency.wav');
      final transcriptionService = _RetryingTranscriptionService();
      final service = UploadQueueService(
        db: db,
        transcriptionService: transcriptionService,
        connectivity: Connectivity(),
        deleteLocalFile: (String filePath) async {
          await File(filePath).delete();
        },
      );

      final firstUploadId = await service.queueUpload(audioFile.path);
      final secondUploadId = await service.queueUpload(audioFile.path);
      final queuedUpload = await db.getUploadByAudioPath(audioFile.path);

      expect(firstUploadId, secondUploadId);
      expect(queuedUpload, isNotNull);
      expect(queuedUpload!.idempotencyKey, isNotNull);
      expect(await db.getPendingUploads(), hasLength(1));

      await service.processQueue();

      final failedUpload = await db.getUploadByAudioPath(audioFile.path);
      expect(failedUpload, isNotNull);
      expect(failedUpload!.status, UploadStatus.failed.index);
      expect(failedUpload.idempotencyKey, queuedUpload.idempotencyKey);

      await service.processQueue();

      expect(
        transcriptionService.idempotencyKeys,
        equals([queuedUpload.idempotencyKey, queuedUpload.idempotencyKey]),
      );
      expect(await db.getUploadByAudioPath(audioFile.path), isNull);
      expect(await audioFile.exists(), isFalse);
    },
  );

  test(
    'Given a queued row is inserted without an explicit idempotency key, When queue processing uploads it, Then the generated stored key is sent to the server',
    () async {
      final audioFile = await createAudioFile('legacy_upload.wav');
      final uploadId = await db.addPendingUpload(audioPath: audioFile.path);
      final transcriptionService = _CountingTranscriptionService();
      final service = UploadQueueService(
        db: db,
        transcriptionService: transcriptionService,
        connectivity: Connectivity(),
        deleteLocalFile: (String filePath) async {
          await File(filePath).delete();
        },
      );

      final queuedUpload = await db.getUploadById(uploadId);

      await service.processQueue();

      expect(queuedUpload, isNotNull);
      expect(queuedUpload!.idempotencyKey, isNotNull);
      expect(
        transcriptionService.idempotencyKeys,
        equals([queuedUpload.idempotencyKey]),
      );
      expect(await db.getUploadById(uploadId), isNull);
      expect(await audioFile.exists(), isFalse);
    },
  );
}
