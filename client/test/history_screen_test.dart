import 'dart:async';

import 'package:dio/dio.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:oracy/screens/history_screen.dart';
import 'package:oracy/services/history_service.dart';
import 'package:oracy/services/transcription_service.dart';

import 'helpers/test_utils.dart';

class _FakeHistoryService extends HistoryService {
  _FakeHistoryService() : super(Dio());

  final queries = <String?>[];

  @override
  Future<TranscriptListResponse> getTranscripts({
    int offset = 0,
    int limit = 20,
    String? query,
  }) async {
    queries.add(query);
    final transcripts = query == null || query.isEmpty
        ? [createMockTranscript(id: 'visible', transcript: 'Visible page')]
        : [createMockTranscript(id: 'remote', transcript: 'Remote needle hit')];
    return TranscriptListResponse(
      transcripts: transcripts,
      total: transcripts.length,
      offset: offset,
      limit: limit,
    );
  }
}

class _PendingHistoryRequest {
  final int offset;
  final int limit;
  final String? query;
  final Completer<TranscriptListResponse> completer = Completer();

  _PendingHistoryRequest({
    required this.offset,
    required this.limit,
    required this.query,
  });

  void complete(List<TranscriptResponse> transcripts, {int? total}) {
    completer.complete(
      TranscriptListResponse(
        transcripts: transcripts,
        total: total ?? transcripts.length,
        offset: offset,
        limit: limit,
      ),
    );
  }
}

class _ControlledHistoryService extends HistoryService {
  _ControlledHistoryService() : super(Dio());

  final requests = <_PendingHistoryRequest>[];

  @override
  Future<TranscriptListResponse> getTranscripts({
    int offset = 0,
    int limit = 20,
    String? query,
  }) {
    final request = _PendingHistoryRequest(
      offset: offset,
      limit: limit,
      query: query,
    );
    requests.add(request);
    return request.completer.future;
  }
}

void main() {
  test(
    'Given an older history query finishes last, When requests complete out of order, Then stale results are ignored',
    () async {
      final service = _ControlledHistoryService();
      final container = ProviderContainer(
        overrides: [historyServiceProvider.overrideWithValue(service)],
      );
      addTearDown(container.dispose);

      final notifier = container.read(transcriptHistoryProvider.notifier);
      final oldSearch = notifier.search('old');
      expect(service.requests.single.query, 'old');

      final newSearch = notifier.search('new');
      expect(service.requests.last.query, 'new');

      service.requests.last.complete([
        createMockTranscript(id: 'new-result', transcript: 'new result'),
      ]);
      await newSearch;

      service.requests.first.complete([
        createMockTranscript(id: 'old-result', transcript: 'old result'),
      ]);
      await oldSearch;

      final state = container.read(transcriptHistoryProvider);
      expect(state.query, 'new');
      expect(state.transcripts.map((t) => t.id), ['new-result']);
    },
  );

  testWidgets(
    'Given a match exists outside the loaded page, When history search changes, Then the transcript collection is queried and remote results are shown',
    (tester) async {
      final service = _FakeHistoryService();

      await tester.pumpWidget(
        ProviderScope(
          overrides: [historyServiceProvider.overrideWithValue(service)],
          child: const MaterialApp(home: HistoryScreen()),
        ),
      );
      await tester.pump();
      await tester.pump(const Duration(milliseconds: 20));

      await tester.enterText(find.byType(TextField), 'needle');
      await tester.pump(const Duration(milliseconds: 350));
      await tester.pumpAndSettle();

      expect(service.queries, contains('needle'));
      expect(find.textContaining('Remote needle hit'), findsOneWidget);
    },
  );

  testWidgets(
    'Given more history pages exist but none is loading, When history is rendered, Then the footer spinner is hidden',
    (tester) async {
      await tester.pumpWidget(
        ProviderScope(
          overrides: [
            historyOverride(
              transcripts: [createMockTranscript()],
              hasMore: true,
            ),
          ],
          child: const MaterialApp(home: HistoryScreen()),
        ),
      );
      await tester.pump();
      await tester.pump(const Duration(milliseconds: 20));

      expect(find.byType(CircularProgressIndicator), findsNothing);
    },
  );

  testWidgets(
    'Given active search has more pages, When the results are scrolled, Then the next search page loads with a footer spinner',
    (tester) async {
      final service = _ControlledHistoryService();

      await tester.pumpWidget(
        ProviderScope(
          overrides: [historyServiceProvider.overrideWithValue(service)],
          child: const MaterialApp(home: HistoryScreen()),
        ),
      );
      await tester.pump();

      service.requests.single.complete([]);
      await tester.pumpAndSettle();

      await tester.enterText(find.byType(TextField), 'needle');
      await tester.pump(const Duration(milliseconds: 350));

      service.requests.last.complete(
        List.generate(
          20,
          (index) => createMockTranscript(
            id: 'needle-$index',
            transcript: 'Remote needle hit ${index + 1}',
            createdAt: DateTime.now().subtract(Duration(minutes: index)),
          ),
        ),
        total: 25,
      );
      await tester.pumpAndSettle();

      await tester.drag(find.byType(ListView), const Offset(0, -3000));
      await tester.pump(const Duration(milliseconds: 100));

      expect(service.requests.length, 3);
      expect(service.requests.last.query, 'needle');
      expect(service.requests.last.offset, 20);
      expect(
        find.byType(CircularProgressIndicator, skipOffstage: false),
        findsOneWidget,
      );
    },
  );

  testWidgets(
    'Given history exists, When a search has no matches, Then the search-empty message is shown',
    (tester) async {
      final service = _ControlledHistoryService();

      await tester.pumpWidget(
        ProviderScope(
          overrides: [historyServiceProvider.overrideWithValue(service)],
          child: const MaterialApp(home: HistoryScreen()),
        ),
      );
      await tester.pump();

      service.requests.single.complete([
        createMockTranscript(id: 'visible', transcript: 'Visible history'),
      ]);
      await tester.pumpAndSettle();

      await tester.enterText(find.byType(TextField), 'missing');
      await tester.pump(const Duration(milliseconds: 350));

      service.requests.last.complete([]);
      await tester.pumpAndSettle();

      expect(find.text('No results found'), findsOneWidget);
      expect(find.text('No transcripts yet'), findsNothing);
      expect(find.text('No transcripts match "missing"'), findsOneWidget);
    },
  );
}
