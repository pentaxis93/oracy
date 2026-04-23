// Web platform implementation

import 'dart:html' as html;
import 'dart:typed_data';
import 'package:flutter/foundation.dart';
import 'transcription_service.dart';
import 'web_recording_upload_metadata.dart';

Future<FileData> getFileData(String filePath) async {
  // On web, filePath is a blob URL (e.g., blob:https://oracy.app/...)
  // We need to fetch the blob and convert it to bytes

  if (kDebugMode) {
    print('[TRANSCRIPTION_WEB] Getting file data for: $filePath');
  }

  // Fetch the blob URL
  final response = await html.HttpRequest.request(
    filePath,
    responseType: 'arraybuffer',
  );

  final buffer = response.response as ByteBuffer;
  final bytes = Uint8List.view(buffer);

  if (kDebugMode) {
    print('[TRANSCRIPTION_WEB] Got ${bytes.length} bytes');
  }

  // Generate a filename based on timestamp
  final timestamp = DateTime.now().millisecondsSinceEpoch;
  final uploadMetadata = webRecordingUploadMetadata(
    timestamp: timestamp,
    blobContentType: response.getResponseHeader('content-type'),
  );

  if (kDebugMode) {
    print('[TRANSCRIPTION_WEB] Using filename: ${uploadMetadata.filename}');
  }

  return FileData(
    bytes: bytes,
    filename: uploadMetadata.filename,
    contentType: uploadMetadata.contentType,
  );
}

Future<void> deleteLocalFileIfExists(String filePath) async {}
