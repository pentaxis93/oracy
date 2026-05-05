import 'dart:async';

import 'package:dio/dio.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:oracy/models/voice_note.dart';
import 'package:oracy/screens/history_screen.dart';
import 'package:oracy/services/history_service.dart';

import 'helpers/test_utils.dart';

class _FakeHistoryService extends HistoryService {
  _FakeHistoryService() : super(Dio());

  final queries = <String?>[];

  @override
  Future<VoiceNoteCollectionResponse> getVoiceNotes({
    String? cursor,
    int? limit,
    String? query,
  }) async {
    queries.add(query);
    final voiceNotes = query == null || query.isEmpty
        ? [createMockVoiceNote(id: 'visible', text: 'Visible page')]
        : [createMockVoiceNote(id: 'remote', text: 'Remote needle hit')];
    return VoiceNoteCollectionResponse(items: voiceNotes, nextCursor: null);
  }
}

class _PendingHistoryRequest {
  final String? cursor;
  final int? limit;
  final String? query;
  final Completer<VoiceNoteCollectionResponse> completer = Completer();

  _PendingHistoryRequest({
    required this.cursor,
    required this.limit,
    required this.query,
  });

  void complete(List<VoiceNote> voiceNotes, {String? nextCursor}) {
    completer.complete(
      VoiceNoteCollectionResponse(items: voiceNotes, nextCursor: nextCursor),
    );
  }
}

class _ControlledHistoryService extends HistoryService {
  _ControlledHistoryService() : super(Dio());

  final requests = <_PendingHistoryRequest>[];

  @override
  Future<VoiceNoteCollectionResponse> getVoiceNotes({
    String? cursor,
    int? limit,
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
    'Given a match exists outside the loaded page, When history search changes, Then the voice-note collection is queried and remote results are shown',
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
    'Given search results span dates, When history renders, Then response order is preserved',
    (tester) async {
      final service = _ControlledHistoryService();
      final now = DateTime.now();
      final todayStart = DateTime(now.year, now.month, now.day);

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

      service.requests.last.complete([
        createMockVoiceNote(
          id: 'older-relevant',
          text: 'Older relevant hit',
          createdAt: now.subtract(const Duration(days: 3)),
        ),
        createMockVoiceNote(
          id: 'newer-less-relevant',
          text: 'Newer less relevant hit',
          createdAt: todayStart.add(const Duration(hours: 12)),
        ),
      ]);
      await tester.pumpAndSettle();

      expect(find.text('Today'), findsNothing);
      expect(
        tester.getTopLeft(find.textContaining('Older relevant hit')).dy,
        lessThan(
          tester.getTopLeft(find.textContaining('Newer less relevant hit')).dy,
        ),
      );
    },
  );

  testWidgets(
    'Given browse history spans dates, When history renders, Then date grouping is preserved',
    (tester) async {
      final now = DateTime.now();
      final todayStart = DateTime(now.year, now.month, now.day);

      await tester.pumpWidget(
        ProviderScope(
          overrides: [
            historyOverride(
              voiceNotes: [
                createMockVoiceNote(
                  id: 'today-note',
                  text: 'Today history note',
                  createdAt: todayStart.add(const Duration(hours: 12)),
                ),
                createMockVoiceNote(
                  id: 'yesterday-note',
                  text: 'Yesterday history note',
                  createdAt: todayStart
                      .subtract(const Duration(days: 1))
                      .add(const Duration(hours: 12)),
                ),
              ],
            ),
          ],
          child: const MaterialApp(home: HistoryScreen()),
        ),
      );
      await tester.pump();
      await tester.pump(const Duration(milliseconds: 20));

      expect(find.text('Today'), findsOneWidget);
      expect(find.text('Yesterday'), findsOneWidget);
      expect(
        tester.getTopLeft(find.text('Today')).dy,
        lessThan(
          tester.getTopLeft(find.textContaining('Today history note')).dy,
        ),
      );
      expect(
        tester.getTopLeft(find.text('Yesterday')).dy,
        lessThan(
          tester.getTopLeft(find.textContaining('Yesterday history note')).dy,
        ),
      );
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
    'Given a voice note has no language, When history is rendered, Then the note appears without a language badge',
    (tester) async {
      await tester.pumpWidget(
        ProviderScope(
          overrides: [
            historyOverride(
              voiceNotes: [
                createMockVoiceNote(
                  id: 'no-language',
                  text: 'Visible note without language',
                  language: null,
                ),
              ],
            ),
          ],
          child: const MaterialApp(home: HistoryScreen()),
        ),
      );
      await tester.pump();
      await tester.pump(const Duration(milliseconds: 20));

      expect(
        find.textContaining('Visible note without language'),
        findsOneWidget,
      );
      expect(find.text('EN'), findsNothing);
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
        nextCursor: 'next-search-page',
      );
      await tester.pumpAndSettle();

      await tester.drag(find.byType(ListView), const Offset(0, -3000));
      await tester.pump(const Duration(milliseconds: 100));

      expect(service.requests.length, 3);
      expect(service.requests.last.query, 'needle');
      expect(service.requests.last.cursor, 'next-search-page');
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
