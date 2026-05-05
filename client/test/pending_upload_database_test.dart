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
}
