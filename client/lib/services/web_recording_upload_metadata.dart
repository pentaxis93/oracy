import 'package:dio/dio.dart';

class WebRecordingUploadMetadata {
  final String filename;
  final DioMediaType? contentType;

  const WebRecordingUploadMetadata({
    required this.filename,
    required this.contentType,
  });
}

WebRecordingUploadMetadata webRecordingUploadMetadata({
  required int timestamp,
  required String? blobContentType,
}) {
  if (_isWavContentType(blobContentType)) {
    return WebRecordingUploadMetadata(
      filename: 'recording_$timestamp.wav',
      contentType: DioMediaType('audio', 'wav'),
    );
  }

  return WebRecordingUploadMetadata(
    filename: 'recording_$timestamp.webm',
    contentType: null,
  );
}

bool _isWavContentType(String? contentType) {
  if (contentType == null) {
    return false;
  }

  final mediaType = contentType.split(';').first.trim().toLowerCase();
  return mediaType == 'audio/wav' ||
      mediaType == 'audio/x-wav' ||
      mediaType == 'audio/wave' ||
      mediaType == 'audio/vnd.wave';
}
