import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'dart:typed_data';

import 'package:dio/dio.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:oracy/services/transcription_service.dart';

class _CapturedRequest {
  final String method;
  final String path;
  final Map<String, dynamic> headers;
  final List<int> body;

  const _CapturedRequest({
    required this.method,
    required this.path,
    required this.headers,
    required this.body,
  });

  String get bodyText => utf8.decode(body, allowMalformed: true);
}

class _ProtocolAdapter implements HttpClientAdapter {
  final requests = <_CapturedRequest>[];
  final List<Map<String, dynamic>> jobPolls;
  final Map<String, dynamic> openJob;

  _ProtocolAdapter({
    List<Map<String, dynamic>>? jobPolls,
    Map<String, dynamic>? openJob,
  }) : jobPolls =
           jobPolls ??
           [
             _jobJson(status: 'processing'),
             _jobJson(status: 'succeeded', voiceNoteId: 'voice-note-123'),
           ],
       openJob =
           openJob ?? _jobJson(status: 'accepting_chunks', chunksReceived: 0);

  @override
  Future<ResponseBody> fetch(
    RequestOptions options,
    Stream<Uint8List>? requestStream,
    Future<void>? cancelFuture,
  ) async {
    final body = <int>[];
    if (requestStream != null) {
      await for (final chunk in requestStream) {
        body.addAll(chunk);
      }
    }

    requests.add(
      _CapturedRequest(
        method: options.method,
        path: options.path,
        headers: Map<String, dynamic>.from(options.headers),
        body: body,
      ),
    );

    if (options.path == '/api/v1/transcription-jobs') {
      return _jsonResponse(openJob, 201);
    }

    if (options.path.endsWith('/chunks')) {
      return ResponseBody.fromBytes(const [], 204);
    }

    if (options.path.endsWith('/finalize')) {
      return _jsonResponse(_jobJson(status: 'queued', chunksReceived: 2), 202);
    }

    if (options.path == '/api/v1/transcription-jobs/job-123') {
      return _jsonResponse(jobPolls.removeAt(0), 200);
    }

    if (options.path == '/api/v1/voice-notes/voice-note-123') {
      return _jsonResponse(_voiceNoteJson(), 200);
    }

    return _jsonResponse({'detail': 'unexpected ${options.path}'}, 500);
  }

  @override
  void close({bool force = false}) {}
}

ResponseBody _jsonResponse(Map<String, dynamic> body, int statusCode) {
  return ResponseBody.fromString(
    jsonEncode(body),
    statusCode,
    headers: {
      Headers.contentTypeHeader: [Headers.jsonContentType],
    },
  );
}

Map<String, dynamic> _jobJson({
  required String status,
  int chunksReceived = 2,
  String? voiceNoteId,
  String? nextAttemptAt,
}) {
  return {
    'id': 'job-123',
    'status': status,
    'created_at': '2026-04-21T18:30:00Z',
    'updated_at': '2026-04-21T18:30:14Z',
    'chunk_count': 2,
    'chunks_received': chunksReceived,
    'retry_count': 0,
    'max_retries': 3,
    'next_attempt_at': nextAttemptAt,
    'failure_code': null,
    'failure_message': null,
    'retryable_by_client': null,
    'voice_note_id': voiceNoteId,
  };
}

Map<String, dynamic> _voiceNoteJson() {
  return {
    'id': 'voice-note-123',
    'current_version_id': 'version-123',
    'text': 'chunked protocol complete',
    'audio_duration_seconds': 2.0,
    'audio_format': 'wav',
    'audio_size_bytes': 26214401,
    'language': 'en',
    'model': 'gpt-4o-mini-transcribe',
    'processing_time_ms': 1000,
    'cost_cents': null,
    'created_at': '2026-04-21T18:31:00Z',
    'recorded_at': '2026-04-21T18:29:55Z',
    'session_id': null,
    'tags': [],
  };
}

void main() {
  late Directory tempDir;

  setUp(() async {
    tempDir = await Directory.systemTemp.createTemp(
      'oracy_transcription_protocol_test_',
    );
  });

  tearDown(() async {
    if (await tempDir.exists()) {
      await tempDir.delete(recursive: true);
    }
  });

  Future<File> createAudioFile(String name, int byteCount) async {
    final file = File('${tempDir.path}/$name');
    await file.writeAsBytes(List<int>.generate(byteCount, (i) => i % 251));
    return file;
  }

  test(
    'Given audio larger than one chunk, When transcription runs, Then the client opens pushes finalizes polls and fetches the voice note',
    () async {
      final audioFile = await createAudioFile(
        'oracy_recording_1770000000000.wav',
        maxTranscriptionChunkBytes + 1,
      );
      final adapter = _ProtocolAdapter();
      final dio = Dio()..httpClientAdapter = adapter;
      final sleeps = <Duration>[];
      final service = TranscriptionService(
        dio,
        pollInterval: Duration.zero,
        sleep: (duration) async => sleeps.add(duration),
      );

      final voiceNote = await service.transcribe(
        audioFile.path,
        language: 'en',
        idempotencyKey: 'stable-key',
        recordedAt: DateTime.utc(2026, 4, 21, 18, 29, 55),
      );

      expect(voiceNote.id, 'voice-note-123');
      expect(voiceNote.text, 'chunked protocol complete');
      expect(adapter.requests.map((request) => request.path), [
        '/api/v1/transcription-jobs',
        '/api/v1/transcription-jobs/job-123/chunks',
        '/api/v1/transcription-jobs/job-123/chunks',
        '/api/v1/transcription-jobs/job-123/finalize',
        '/api/v1/transcription-jobs/job-123',
        '/api/v1/transcription-jobs/job-123',
        '/api/v1/voice-notes/voice-note-123',
      ]);
      expect(
        adapter.requests.any((request) => request.path == '/api/v1/transcribe'),
        isFalse,
      );

      final open = adapter.requests.first;
      expect(open.headers['Idempotency-Key'], 'stable-key');
      final openBody = jsonDecode(open.bodyText) as Map<String, dynamic>;
      expect(openBody['recorded_at'], '2026-04-21T18:29:55.000Z');
      expect(openBody['chunk_count'], 2);
      expect(openBody['audio_format'], 'wav');
      expect(openBody['language'], 'en');

      final firstChunk = adapter.requests[1].bodyText;
      final secondChunk = adapter.requests[2].bodyText;
      final chunks = chunkAudio(await audioFile.readAsBytes());
      expect(firstChunk, contains('name="chunk_index"'));
      expect(firstChunk, contains('0'));
      expect(firstChunk, contains(sha256Hex(chunks[0])));
      expect(secondChunk, contains('name="chunk_index"'));
      expect(secondChunk, contains('1'));
      expect(secondChunk, contains(sha256Hex(chunks[1])));
      expect(sleeps, [Duration.zero, Duration.zero]);
    },
  );

  test(
    'Given retry waiting with a future attempt time, When polling the job, Then polling sleeps until next attempt instead of the fixed interval',
    () async {
      final audioFile = await createAudioFile('recording.wav', 4);
      final adapter = _ProtocolAdapter(
        jobPolls: [
          _jobJson(
            status: 'retry_waiting',
            nextAttemptAt: '2026-04-21T18:30:10Z',
          ),
          _jobJson(status: 'succeeded', voiceNoteId: 'voice-note-123'),
        ],
      );
      final dio = Dio()..httpClientAdapter = adapter;
      final sleeps = <Duration>[];
      final service = TranscriptionService(
        dio,
        pollInterval: const Duration(seconds: 5),
        sleep: (duration) async => sleeps.add(duration),
        now: () => DateTime.utc(2026, 4, 21, 18, 30, 0),
      );

      await service.transcribe(
        audioFile.path,
        idempotencyKey: 'live-key',
        recordedAt: DateTime.utc(2026, 4, 21, 18, 29, 55),
      );

      expect(sleeps, [const Duration(seconds: 5), const Duration(seconds: 10)]);
    },
  );

  test(
    'Given retry waiting with a non-future attempt time, When polling the job, Then polling uses the fixed interval',
    () async {
      final audioFile = await createAudioFile('recording.wav', 4);
      const pollInterval = Duration(seconds: 5);

      for (final nextAttemptAt in [
        '2026-04-21T18:29:50Z',
        '2026-04-21T18:30:00Z',
      ]) {
        final adapter = _ProtocolAdapter(
          jobPolls: [
            _jobJson(status: 'retry_waiting', nextAttemptAt: nextAttemptAt),
            _jobJson(status: 'succeeded', voiceNoteId: 'voice-note-123'),
          ],
        );
        final dio = Dio()..httpClientAdapter = adapter;
        final sleeps = <Duration>[];
        final service = TranscriptionService(
          dio,
          pollInterval: pollInterval,
          sleep: (duration) async => sleeps.add(duration),
          now: () => DateTime.utc(2026, 4, 21, 18, 30),
        );

        await service.transcribe(
          audioFile.path,
          idempotencyKey: 'live-key-$nextAttemptAt',
          recordedAt: DateTime.utc(2026, 4, 21, 18, 29, 55),
        );

        expect(sleeps, [pollInterval, pollInterval]);
      }
    },
  );

  test(
    'Given opening replays a succeeded job, When transcription runs, Then the client fetches the voice note without pushing chunks again',
    () async {
      final audioFile = await createAudioFile('recording.wav', 4);
      final adapter = _ProtocolAdapter(
        openJob: _jobJson(status: 'succeeded', voiceNoteId: 'voice-note-123'),
      );
      final dio = Dio()..httpClientAdapter = adapter;
      final service = TranscriptionService(dio);

      final voiceNote = await service.transcribe(
        audioFile.path,
        idempotencyKey: 'stable-key',
        recordedAt: DateTime.utc(2026, 4, 21, 18, 29, 55),
      );

      expect(voiceNote.id, 'voice-note-123');
      expect(adapter.requests.map((request) => request.path), [
        '/api/v1/transcription-jobs',
        '/api/v1/voice-notes/voice-note-123',
      ]);
    },
  );
}
