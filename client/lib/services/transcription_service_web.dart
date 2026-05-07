import 'dart:js_interop';

import 'package:flutter/foundation.dart';
import 'package:web/web.dart' as web;

import 'transcription_service.dart';
import 'web_recording_upload_metadata.dart';

Future<FileData> getFileData(String filePath, {String? idempotencyKey}) async {
  // On web, filePath is a blob URL (e.g., blob:https://oracy.app/...)
  // We need to fetch the blob and convert it to bytes

  if (kDebugMode) {
    debugPrint('[TRANSCRIPTION_WEB] Getting file data for: $filePath');
  }

  // Fetch the blob URL
  final response = await web.window.fetch(filePath.toJS).toDart;

  final buffer = await response.arrayBuffer().toDart;
  final bytes = buffer.toDart.asUint8List();

  if (kDebugMode) {
    debugPrint('[TRANSCRIPTION_WEB] Got ${bytes.length} bytes');
  }

  // Generate a filename based on timestamp
  final timestamp = DateTime.now().millisecondsSinceEpoch;
  final uploadMetadata = webRecordingUploadMetadata(
    timestamp: timestamp,
    blobContentType: response.headers.get('content-type'),
  );

  if (kDebugMode) {
    debugPrint(
      '[TRANSCRIPTION_WEB] Using filename: ${uploadMetadata.filename}',
    );
  }

  return FileData(
    bytes: bytes,
    filename: uploadMetadata.filename,
    contentType: uploadMetadata.contentType,
  );
}

Future<void> deleteLocalFileIfExists(String filePath) async {}
