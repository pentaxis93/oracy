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
  Future<VoiceNoteListResponse> getVoiceNotes({
    String? cursor,
    int limit = 20,
    String? query,
  }) async {
    queries.add(query);
    final voiceNotes = query == null || query.isEmpty
        ? [createMockVoiceNote(id: 'visible', text: 'Visible page')]
        : [createMockVoiceNote(id: 'remote', text: 'Remote needle hit')];
    return VoiceNoteListResponse(voiceNotes: voiceNotes);
  }
}

class _PendingHistoryRequest {
  final String? cursor;
  final int limit;
  final String? query;
  final Completer<VoiceNoteListResponse> completer = Completer();

  _PendingHistoryRequest({
    required this.cursor,
    required this.limit,
    required this.query,
  });

  void complete(List<VoiceNoteResponse> voiceNotes, {String? nextCursor}) {
    completer.complete(
      VoiceNoteListResponse(voiceNotes: voiceNotes, nextCursor: nextCursor),
    );
  }
}

class _ControlledHistoryService extends HistoryService {
  _ControlledHistoryService() : super(Dio());

  final requests = <_PendingHistoryRequest>[];

  @override
  Future<VoiceNoteListResponse> getVoiceNotes({
    String? cursor,
    int limit = 20,
    String? query,
  }) {
    final request = _PendingHistoryRequest(
      cursor: cursor,
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

      final notifier = container.read(voiceNoteHistoryProvider.notifier);
      final oldSearch = notifier.search('old');
      expect(service.requests.single.query, 'old');

      final newSearch = notifier.search('new');
      expect(service.requests.last.query, 'new');

      service.requests.last.complete([
        createMockVoiceNote(id: 'new-result', text: 'new result'),
      ]);
      await newSearch;

      service.requests.first.complete([
        createMockVoiceNote(id: 'old-result', text: 'old result'),
      ]);
      await oldSearch;

      final state = container.read(voiceNoteHistoryProvider);
      expect(state.query, 'new');
      expect(state.voiceNotes.map((t) => t.id), ['new-result']);
    },
  );

  testWidgets(
    'Given a match exists outside the loaded page, When history search changes, Then the voice note collection is queried and remote results are shown',
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
            historyOverride(voiceNotes: [createMockVoiceNote()], hasMore: true),
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
          (index) => createMockVoiceNote(
            id: 'needle-$index',
            text: 'Remote needle hit ${index + 1}',
            createdAt: DateTime.now().subtract(Duration(minutes: index)),
          ),
        ),
        nextCursor: 'needle-page-2',
      );
      await tester.pumpAndSettle();

      await tester.drag(find.byType(ListView), const Offset(0, -3000));
      await tester.pump(const Duration(milliseconds: 100));

      expect(service.requests.length, 3);
      expect(service.requests.last.query, 'needle');
      expect(service.requests.last.cursor, 'needle-page-2');
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
        createMockVoiceNote(id: 'visible', text: 'Visible history'),
      ]);
      await tester.pumpAndSettle();

      await tester.enterText(find.byType(TextField), 'missing');
      await tester.pump(const Duration(milliseconds: 350));

      service.requests.last.complete([]);
      await tester.pumpAndSettle();

      expect(find.text('No results found'), findsOneWidget);
      expect(find.text('No voice notes yet'), findsNothing);
      expect(find.text('No voice notes match "missing"'), findsOneWidget);
    },
  );
}
