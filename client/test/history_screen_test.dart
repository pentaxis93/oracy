import 'package:dio/dio.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:oracy/screens/history_screen.dart';
import 'package:oracy/services/history_service.dart';

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

void main() {
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
}
