// Native platform implementation (Android, iOS, Desktop)

import 'dart:io';
import 'transcription_service.dart';

Future<FileData> getFileData(String filePath) async {
  final file = File(filePath);
  if (!await file.exists()) {
    throw Exception('Audio file not found: $filePath');
  }

  final bytes = await file.readAsBytes();
  final filename = file.path.split('/').last;

  return FileData(bytes: bytes, filename: filename);
}

Future<void> deleteLocalFileIfExists(String filePath) async {
  final file = File(filePath);
  if (await file.exists()) {
    await file.delete();
  }
}
