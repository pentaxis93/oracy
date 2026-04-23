import 'dart:typed_data';

import 'package:flutter_test/flutter_test.dart';
import 'package:oracy/services/transcription_service.dart';
import 'package:oracy/services/web_recording_upload_metadata.dart';

void main() {
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
}
