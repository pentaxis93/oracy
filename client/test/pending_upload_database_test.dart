import 'package:drift/native.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:oracy/db/database.dart';

void main() {
  late AppDatabase db;

  setUp(() {
    db = AppDatabase(NativeDatabase.memory());
  });

  tearDown(() async {
    await db.close();
  });

  test(
    'Given a queued upload already has the same language, When it is queued again, Then the idempotency key stays stable',
    () async {
      const audioPath = '/tmp/stable-language.wav';

      final firstId = await db.ensurePendingUpload(
        audioPath: audioPath,
        language: 'en',
      );
      final firstUpload = await db.getUploadById(firstId);

      final secondId = await db.ensurePendingUpload(
        audioPath: audioPath,
        language: 'en',
      );
      final secondUpload = await db.getUploadById(secondId);

      expect(secondId, firstId);
      expect(firstUpload, isNotNull);
      expect(secondUpload, isNotNull);
      expect(secondUpload!.language, 'en');
      expect(secondUpload.idempotencyKey, firstUpload!.idempotencyKey);
      expect(await db.getPendingUploads(), hasLength(1));
    },
  );

  test(
    'Given a queued upload already has no language, When it is queued again with no language, Then the idempotency key stays stable',
    () async {
      const audioPath = '/tmp/null-language.wav';

      final firstId = await db.ensurePendingUpload(audioPath: audioPath);
      final firstUpload = await db.getUploadById(firstId);

      final secondId = await db.ensurePendingUpload(audioPath: audioPath);
      final secondUpload = await db.getUploadById(secondId);

      expect(secondId, firstId);
      expect(firstUpload, isNotNull);
      expect(secondUpload, isNotNull);
      expect(secondUpload!.language, isNull);
      expect(secondUpload.idempotencyKey, firstUpload!.idempotencyKey);
      expect(await db.getPendingUploads(), hasLength(1));
    },
  );

  test(
    'Given a queued upload already has a language, When it is queued again with a different language, Then the idempotency key is refreshed',
    () async {
      const audioPath = '/tmp/changed-language.wav';

      final firstId = await db.ensurePendingUpload(
        audioPath: audioPath,
        language: 'en',
      );
      final firstUpload = await db.getUploadById(firstId);

      final secondId = await db.ensurePendingUpload(
        audioPath: audioPath,
        language: 'fr',
      );
      final secondUpload = await db.getUploadById(secondId);
      final storedUploads = await db.getPendingUploads();

      expect(secondId, firstId);
      expect(firstUpload, isNotNull);
      expect(secondUpload, isNotNull);
      expect(secondUpload!.language, 'fr');
      expect(secondUpload.idempotencyKey, isNot(firstUpload!.idempotencyKey));
      expect(
        storedUploads.map((upload) => upload.idempotencyKey),
        isNot(contains(firstUpload.idempotencyKey)),
      );
      expect(storedUploads, hasLength(1));
    },
  );

  test(
    'Given durable web audio bytes are stored for a queued upload, When stale web payload cleanup runs, Then referenced bytes remain and orphaned stale bytes are removed',
    () async {
      await db.addPendingUpload(
        audioPath: 'blob:https://oracy.test/live',
        idempotencyKey: 'live-key',
      );
      await db.upsertWebUploadPayload(
        idempotencyKey: 'live-key',
        audioPath: 'blob:https://oracy.test/live',
        bytes: [1, 2, 3],
        filename: 'recording_1234.webm',
      );
      await db.upsertWebUploadPayload(
        idempotencyKey: 'orphaned-key',
        audioPath: 'blob:https://oracy.test/orphaned',
        bytes: [4, 5, 6],
        filename: 'recording_5678.wav',
        contentType: 'audio/wav',
      );

      final removed = await db.cleanupStaleWebUploadPayloads(
        maxAge: const Duration(days: -1),
      );

      expect(removed, 1);
      expect(await db.getWebUploadPayload('live-key'), isNotNull);
      expect(await db.getWebUploadPayload('orphaned-key'), isNull);
    },
  );
}
