import 'dart:io';

import 'package:connectivity_plus/connectivity_plus.dart';
import 'package:dio/dio.dart';
import 'package:drift/drift.dart' show Value;
import 'package:drift/native.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:oracy/db/database.dart';
import 'package:oracy/services/background_sync_service.dart';
import 'package:oracy/services/recording_recovery_service.dart';
import 'package:oracy/services/transcription_service.dart';

import 'helpers/test_utils.dart';

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

class _CountingTranscriptionService extends TranscriptionService {
  _CountingTranscriptionService() : super(Dio());

  int callCount = 0;

  @override
  Future<TranscriptionSubmissionResult> transcribe(
    String filePath, {
    String? language,
    required String idempotencyKey,
    required DateTime recordedAt,
    void Function(double progress)? onProgress,
  }) async {
    callCount++;
    return TranscriptionSubmissionVoiceNote(createMockVoiceNote());
  }
}

class _AcceptedWithoutVoiceNoteTranscriptionService
    extends TranscriptionService {
  _AcceptedWithoutVoiceNoteTranscriptionService() : super(Dio());

  int callCount = 0;

  @override
  Future<TranscriptionSubmissionResult> transcribe(
    String filePath, {
    String? language,
    required String idempotencyKey,
    required DateTime recordedAt,
    void Function(double progress)? onProgress,
  }) async {
    callCount++;
    return const TranscriptionSubmissionAcceptedWithoutVoiceNote();
  }
}

void main() {
  late Directory tempDir;
  late AppDatabase db;

  setUp(() async {
    tempDir = await Directory.systemTemp.createTemp('oracy_bg_sync_test_');
    db = AppDatabase(NativeDatabase.memory());
  });

  tearDown(() async {
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

  test(
    'Given a background sync pass, When an uploading row is stale, Then it is restored to failed and returned to the pending queue',
    () async {
      final uploadingFile = await createAudioFile('stale_uploading.wav');
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

      final processed = await runBackgroundSyncPass(
        db: db,
        checkConnectivity: () async => [ConnectivityResult.none],
        readApiKey: () async => '',
        createTranscriptionService: (_) => _SuccessfulTranscriptionService(),
        reconcilePersistentRecordings: (_) async => 0,
      );

      final uploadingRow = await db.getUploadByAudioPath(uploadingFile.path);
      final pendingUploads = await db.getPendingUploads();

      expect(processed, isTrue);
      expect(uploadingRow, isNotNull);
      expect(uploadingRow!.status, UploadStatus.failed.index);
      expect(pendingUploads.map((upload) => upload.id), contains(uploadingId));
      expect(await uploadingFile.exists(), isTrue);
    },
  );

  test(
    'Given a background sync pass, When an uploading row was updated recently, Then it remains uploading and excluded from the pending queue',
    () async {
      final uploadingFile = await createAudioFile('recent_uploading.wav');
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

      final processed = await runBackgroundSyncPass(
        db: db,
        checkConnectivity: () async => [ConnectivityResult.none],
        readApiKey: () async => '',
        createTranscriptionService: (_) => _SuccessfulTranscriptionService(),
        reconcilePersistentRecordings: (_) async => 0,
      );

      final uploadingRow = await db.getUploadByAudioPath(uploadingFile.path);
      final pendingUploads = await db.getPendingUploads();

      expect(processed, isTrue);
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
    'Given background upload cleanup is pending, When sync runs again, Then local cleanup retries without reuploading audio',
    () async {
      final audioFile = await createAudioFile('cleanup_pending.wav');
      final transcriptionService = _CountingTranscriptionService();
      await db.ensurePendingUpload(audioPath: audioFile.path);

      final firstPass = await runBackgroundSyncPass(
        db: db,
        checkConnectivity: () async => [ConnectivityResult.wifi],
        readApiKey: () async => 'oracy_sk_test',
        createTranscriptionService: (_) => transcriptionService,
        reconcilePersistentRecordings: (_) async => 0,
        deleteLocalFile: (String _) async {
          throw const FileSystemException('simulated delete failure');
        },
      );

      final cleanupPendingUpload = await db.getUploadByAudioPath(
        audioFile.path,
      );

      expect(firstPass, isTrue);
      expect(cleanupPendingUpload, isNotNull);
      expect(cleanupPendingUpload!.status, UploadStatus.cleanupPending.index);
      expect(transcriptionService.callCount, 1);
      expect(await audioFile.exists(), isTrue);

      final secondPass = await runBackgroundSyncPass(
        db: db,
        checkConnectivity: () async => [ConnectivityResult.wifi],
        readApiKey: () async => 'oracy_sk_test',
        createTranscriptionService: (_) => transcriptionService,
        reconcilePersistentRecordings: (_) async => 0,
        deleteLocalFile: (String filePath) async {
          await File(filePath).delete();
        },
      );

      expect(secondPass, isTrue);
      expect(await db.getUploadByAudioPath(audioFile.path), isNull);
      expect(transcriptionService.callCount, 1);
      expect(await audioFile.exists(), isFalse);
    },
  );

  test(
    'Given a persistent recording with no queue row, When background sync runs, Then reconciliation queues and uploads it in the same pass',
    () async {
      final recordingsDir = Directory('${tempDir.path}/recordings')
        ..createSync(recursive: true);
      final orphanedRecording = File(
        '${recordingsDir.path}/oracy_recording_orphaned.wav',
      );
      await orphanedRecording.writeAsBytes(List<int>.filled(2048, 1));

      final processed = await runBackgroundSyncPass(
        db: db,
        checkConnectivity: () async => [ConnectivityResult.wifi],
        readApiKey: () async => 'oracy_sk_test',
        createTranscriptionService: (_) => _SuccessfulTranscriptionService(),
        reconcilePersistentRecordings: (database) {
          return RecordingRecoveryService.reconcilePersistentRecordings(
            database,
            recordingsDirectory: recordingsDir,
          );
        },
        deleteLocalFile: (String filePath) async {
          await File(filePath).delete();
        },
      );

      expect(processed, isTrue);
      expect(await db.getPendingUploads(), isEmpty);
      expect(await db.getUploadByAudioPath(orphanedRecording.path), isNull);
      expect(await orphanedRecording.exists(), isFalse);
    },
  );

  test(
    'Given background sync replays accepted server work whose voice note was deleted, When upload completes, Then the queue row is dequeued without recording failure',
    () async {
      final audioFile = await createAudioFile('background_deleted_replay.wav');
      final transcriptionService =
          _AcceptedWithoutVoiceNoteTranscriptionService();
      await db.ensurePendingUpload(audioPath: audioFile.path);

      final processed = await runBackgroundSyncPass(
        db: db,
        checkConnectivity: () async => [ConnectivityResult.wifi],
        readApiKey: () async => 'oracy_sk_test',
        createTranscriptionService: (_) => transcriptionService,
        reconcilePersistentRecordings: (_) async => 0,
        deleteLocalFile: (String filePath) async {
          await File(filePath).delete();
        },
      );

      expect(processed, isTrue);
      expect(transcriptionService.callCount, 1);
      expect(await db.getUploadByAudioPath(audioFile.path), isNull);
      expect(await db.getPendingUploads(), isEmpty);
      expect(await audioFile.exists(), isFalse);
    },
  );
}
