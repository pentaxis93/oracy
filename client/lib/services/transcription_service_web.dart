// Web platform implementation

import 'dart:html' as html;
import 'dart:typed_data';
import 'package:flutter/foundation.dart';
import 'transcription_service.dart';

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
  // The record_web package produces webm files
  final timestamp = DateTime.now().millisecondsSinceEpoch;
  final filename = 'recording_$timestamp.webm';

  if (kDebugMode) {
    print('[TRANSCRIPTION_WEB] Using filename: $filename');
  }

  return FileData(bytes: bytes, filename: filename);
}

Future<void> deleteLocalFileIfExists(String filePath) async {}
