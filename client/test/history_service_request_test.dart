import 'dart:convert';
import 'dart:typed_data';

import 'package:dio/dio.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:oracy/services/history_service.dart';

class _CapturedRequest {
  final String path;
  final Map<String, dynamic> queryParameters;

  const _CapturedRequest({required this.path, required this.queryParameters});
}

class _CapturingAdapter implements HttpClientAdapter {
  final requests = <_CapturedRequest>[];

  @override
  Future<ResponseBody> fetch(
    RequestOptions options,
    Stream<Uint8List>? requestStream,
    Future<void>? cancelFuture,
  ) async {
    requests.add(
      _CapturedRequest(
        path: options.path,
        queryParameters: Map<String, dynamic>.from(options.queryParameters),
      ),
    );

    return ResponseBody.fromString(
      jsonEncode({'items': [], 'next_cursor': null}),
      200,
      headers: {
        Headers.contentTypeHeader: [Headers.jsonContentType],
      },
    );
  }

  @override
  void close({bool force = false}) {}
}

void main() {
  test(
    'Given an initial history request, When voice notes are loaded, Then the v0.1.0 collection endpoint is called without cursor or limit',
    () async {
      final adapter = _CapturingAdapter();
      final dio = Dio()..httpClientAdapter = adapter;
      final service = HistoryService(dio);

      await service.getVoiceNotes();

      expect(adapter.requests.single.path, '/api/v1/voice-notes');
      expect(adapter.requests.single.queryParameters, isEmpty);
    },
  );

  test(
    'Given cursor query and explicit limit, When voice notes are loaded, Then only applicable query parameters are sent',
    () async {
      final adapter = _CapturingAdapter();
      final dio = Dio()..httpClientAdapter = adapter;
      final service = HistoryService(dio);

      await service.getVoiceNotes(
        cursor: 'next-page',
        limit: 25,
        query: 'needle',
      );

      expect(adapter.requests.single.path, '/api/v1/voice-notes');
      expect(adapter.requests.single.queryParameters, {
        'cursor': 'next-page',
        'limit': 25,
        'q': 'needle',
      });
    },
  );
}
