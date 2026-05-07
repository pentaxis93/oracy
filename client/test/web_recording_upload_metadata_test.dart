import 'dart:typed_data';

import 'package:drift/native.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:oracy/db/database.dart';
import 'package:oracy/services/transcription_service.dart';
import 'package:oracy/services/web_recording_upload_metadata.dart';

void main() {
  late AppDatabase db;

  setUp(() {
    db = AppDatabase(NativeDatabase.memory());
  });

  tearDown(() async {
    await db.close();
  });

  test(
    'Given a web WAV blob, When upload metadata is built, Then the multipart file uses a WAV filename and audio/wav content type',
    () {
      final metadata = webRecordingUploadMetadata(
        timestamp: 1234,
        blobContentType: 'audio/x-wav',
      );
      final multipartFile = multipartFileFromFileData(
        FileData(
          bytes: Uint8List.fromList([1, 2, 3]),
          filename: metadata.filename,
          contentType: metadata.contentType,
        ),
      );

      expect(metadata.filename, 'recording_1234.wav');
      expect(metadata.contentType.toString(), 'audio/wav');
      expect(multipartFile.filename, 'recording_1234.wav');
      expect(multipartFile.contentType.toString(), 'audio/wav');
    },
  );

  test(
    'Given no web blob content type, When upload metadata is built, Then the existing webm filename fallback is preserved',
    () {
      final metadata = webRecordingUploadMetadata(
        timestamp: 5678,
        blobContentType: null,
      );

      expect(metadata.filename, 'recording_5678.webm');
      expect(metadata.contentType, isNull);
    },
  );

  test(
    'Given persisted web audio exists for an idempotency key, When file data is read after reload, Then durable bytes and metadata are returned without fetching the stale blob URL',
    () async {
      await db.upsertWebUploadPayload(
        idempotencyKey: 'stable-key',
        audioPath: 'blob:https://oracy.test/stale',
        bytes: [7, 8, 9],
        filename: 'recording_1234.wav',
        contentType: 'audio/wav',
      );
      var fallbackCalled = false;
      final reader = durableWebFileDataReader(db, (
        String filePath, {
        String? idempotencyKey,
      }) async {
        fallbackCalled = true;
        throw StateError('stale blob URL should not be fetched');
      });

      final fileData = await reader(
        'blob:https://oracy.test/stale',
        idempotencyKey: 'stable-key',
      );

      expect(fileData.bytes, Uint8List.fromList([7, 8, 9]));
      expect(fileData.filename, 'recording_1234.wav');
      expect(fileData.contentType.toString(), 'audio/wav');
      expect(fallbackCalled, isFalse);
    },
  );
}
