// Stub implementation - should never be used at runtime
// This file exists to satisfy the analyzer when neither dart:io nor dart:html is available

import 'transcription_service.dart';

Future<FileData> getFileData(String filePath) {
  throw UnsupportedError('Platform not supported');
}

Future<void> deleteLocalFileIfExists(String filePath) async {}
