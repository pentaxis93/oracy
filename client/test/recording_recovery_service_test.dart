import 'dart:io';
import 'dart:typed_data';

import 'package:drift/native.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:oracy/db/database.dart';
import 'package:oracy/services/recording_recovery_service.dart';
import 'package:shared_preferences/shared_preferences.dart';

void main() {
  late Directory tempDir;
  late Directory recordingsDir;
  late AppDatabase db;

  setUp(() async {
    tempDir = await Directory.systemTemp.createTemp('oracy_recovery_test_');
    recordingsDir = Directory('${tempDir.path}/recordings')..createSync();
    db = AppDatabase(NativeDatabase.memory());
    SharedPreferences.setMockInitialValues({});
  });

  tearDown(() async {
    await db.close();
    if (await tempDir.exists()) {
      await tempDir.delete(recursive: true);
    }
  });

  Future<File> createRecording(String name) async {
    final file = File('${recordingsDir.path}/$name');
    await file.writeAsBytes(List<int>.filled(2048, 1));
    return file;
  }

  Future<File> createInterruptedWav(String name) async {
    final file = File('${recordingsDir.path}/$name');
    final bytes = Uint8List(2048);
    bytes.setAll(0, 'RIFF'.codeUnits);
    bytes.setAll(8, 'WAVE'.codeUnits);
    await file.writeAsBytes(bytes);
    return file;
  }

  test(
    'Given an unqueued persistent recording, When startup reconciliation runs, Then the recording is queued once',
    () async {
      final recording = await createRecording('oracy_recording_1.wav');

      final queuedCount =
          await RecordingRecoveryService.reconcilePersistentRecordings(
            db,
            recordingsDirectory: recordingsDir,
          );

      final storedUpload = await db.getUploadByAudioPath(recording.path);
      final pendingUploads = await db.getPendingUploads();

      expect(queuedCount, 1);
      expect(storedUpload, isNotNull);
      expect(pendingUploads, hasLength(1));
      expect(pendingUploads.single.audioPath, recording.path);
    },
  );

  test(
    'Given an already queued persistent recording, When startup reconciliation runs, Then no duplicate queue entry is created',
    () async {
      final recording = await createRecording(
        'oracy_recording_2_recovered.wav',
      );
      await db.ensurePendingUpload(audioPath: recording.path);

      final queuedCount =
          await RecordingRecoveryService.reconcilePersistentRecordings(
            db,
            recordingsDirectory: recordingsDir,
          );

      final storedUpload = await db.getUploadByAudioPath(recording.path);
      final pendingUploads = await db.getPendingUploads();

      expect(queuedCount, 0);
      expect(storedUpload, isNotNull);
      expect(pendingUploads, hasLength(1));
      expect(pendingUploads.single.audioPath, recording.path);
    },
  );

  test(
    'Given unrelated files in the persistent recordings directory, When startup reconciliation runs, Then only app recordings are queued',
    () async {
      await createRecording('oracy_recording_3.wav');
      final ignoredFile = File('${recordingsDir.path}/notes.txt');
      await ignoredFile.writeAsString('ignore me');

      final queuedCount =
          await RecordingRecoveryService.reconcilePersistentRecordings(
            db,
            recordingsDirectory: recordingsDir,
          );

      final pendingUploads = await db.getPendingUploads();
      final ignoredUpload = await db.getUploadByAudioPath(ignoredFile.path);

      expect(queuedCount, 1);
      expect(pendingUploads, hasLength(1));
      expect(ignoredUpload, isNull);
    },
  );

  test(
    'Given an interrupted WAV recording, When recovery repairs it, Then only the recovered file remains on disk',
    () async {
      final interruptedRecording = await createInterruptedWav(
        'oracy_recording_interrupted.wav',
      );
      SharedPreferences.setMockInitialValues({
        'active_recording_path': interruptedRecording.path,
        'active_recording_start': DateTime.now().millisecondsSinceEpoch,
      });

      final recoveredPath =
          await RecordingRecoveryService.checkForInterruptedRecording();

      expect(recoveredPath, endsWith('_recovered.wav'));
      expect(await interruptedRecording.exists(), isFalse);
      expect(await File(recoveredPath!).exists(), isTrue);
    },
  );

  test(
    'Given an interrupted recording marked for cancel, When recovery runs, Then the file is deleted instead of recovered',
    () async {
      final recording = await createRecording('oracy_recording_cancelled.wav');
      SharedPreferences.setMockInitialValues({
        'active_recording_path': recording.path,
        'active_recording_start': DateTime.now().millisecondsSinceEpoch,
        'active_recording_cancel_requested': true,
      });

      final recoveredPath =
          await RecordingRecoveryService.checkForInterruptedRecording();
      final prefs = await SharedPreferences.getInstance();
      final queuedCount =
          await RecordingRecoveryService.reconcilePersistentRecordings(
            db,
            recordingsDirectory: recordingsDir,
            sharedPreferences: prefs,
          );

      expect(recoveredPath, isNull);
      expect(queuedCount, 0);
      expect(await db.getPendingUploads(), isEmpty);
      expect(await recording.exists(), isFalse);
      expect(prefs.getString('active_recording_path'), isNull);
      expect(prefs.getInt('active_recording_start'), isNull);
      expect(prefs.getBool('active_recording_cancel_requested'), isNull);
    },
  );

  test(
    'Given a canceled recording whose file is already gone, When recovery runs, Then the stale markers are cleared',
    () async {
      final recording = File('${recordingsDir.path}/oracy_recording_gone.wav');
      SharedPreferences.setMockInitialValues({
        'active_recording_path': recording.path,
        'active_recording_start': DateTime.now().millisecondsSinceEpoch,
        'active_recording_cancel_requested': true,
      });

      final recoveredPath =
          await RecordingRecoveryService.checkForInterruptedRecording();
      final prefs = await SharedPreferences.getInstance();

      expect(recoveredPath, isNull);
      expect(await recording.exists(), isFalse);
      expect(prefs.getString('active_recording_path'), isNull);
      expect(prefs.getInt('active_recording_start'), isNull);
      expect(prefs.getBool('active_recording_cancel_requested'), isNull);
    },
  );

  test(
    'Given a persistent recording that is still active, When startup reconciliation runs, Then the active file is skipped',
    () async {
      final recording = await createRecording('oracy_recording_active.wav');
      SharedPreferences.setMockInitialValues({
        'active_recording_path': recording.path,
        'active_recording_start': DateTime.now().millisecondsSinceEpoch,
      });
      final prefs = await SharedPreferences.getInstance();

      final queuedCount =
          await RecordingRecoveryService.reconcilePersistentRecordings(
            db,
            recordingsDirectory: recordingsDir,
            sharedPreferences: prefs,
          );

      expect(queuedCount, 0);
      expect(await db.getUploadByAudioPath(recording.path), isNull);
      expect(await db.getPendingUploads(), isEmpty);
    },
  );

  test(
    'Given a recording whose active marker was cleared, When startup reconciliation runs, Then that file is queued normally',
    () async {
      final recording = await createRecording('oracy_recording_completed.wav');
      SharedPreferences.setMockInitialValues({
        'active_recording_path': recording.path,
        'active_recording_start': DateTime.now().millisecondsSinceEpoch,
      });
      final prefs = await SharedPreferences.getInstance();
      await prefs.remove('active_recording_path');
      await prefs.remove('active_recording_start');

      final queuedCount =
          await RecordingRecoveryService.reconcilePersistentRecordings(
            db,
            recordingsDirectory: recordingsDir,
            sharedPreferences: prefs,
          );

      final storedUpload = await db.getUploadByAudioPath(recording.path);

      expect(queuedCount, 1);
      expect(storedUpload, isNotNull);
      expect(storedUpload!.audioPath, recording.path);
    },
  );
}
