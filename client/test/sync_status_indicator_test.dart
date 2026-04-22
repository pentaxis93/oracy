import "dart:async";

import "package:flutter/material.dart";
import "package:flutter_riverpod/flutter_riverpod.dart";
import "package:flutter_test/flutter_test.dart";
import "package:oracy/db/database.dart";
import "package:oracy/services/upload_queue_service.dart";
import "package:oracy/widgets/sync_status_indicator.dart";

import "helpers/test_utils.dart";

class _MutableStream<T> {
  _MutableStream(this._value);

  T _value;
  final StreamController<T> _controller = StreamController<T>.broadcast();

  Stream<T> stream() async* {
    yield _value;
    yield* _controller.stream;
  }

  void add(T value) {
    _value = value;
    _controller.add(value);
  }

  Future<void> close() => _controller.close();
}

PendingUpload _terminalFailureUpload({
  required int id,
  required String audioPath,
  required String errorMessage,
}) {
  final now = DateTime(2026, 4, 16, 10);
  return PendingUpload(
    id: id,
    audioPath: audioPath,
    createdAt: now,
    retryCount: 1,
    status: UploadStatus.terminalFailure.index,
    errorMessage: errorMessage,
    updatedAt: now,
    language: null,
  );
}

void main() {
  testWidgets(
    "Given unsynced recordings exist, When the indicator is tapped, Then the panel opens and sync is triggered immediately",
    (WidgetTester tester) async {
      var syncCalls = 0;

      await tester.pumpWidget(
        ProviderScope(
          overrides: [
            pendingUploadCountOverride(1),
            terminalFailureUploadsProvider.overrideWith(
              (ref) => Stream.value(const <PendingUpload>[]),
            ),
            syncTriggerProvider.overrideWithValue(() async {
              syncCalls++;
            }),
          ],
          child: MaterialApp(
            home: Scaffold(
              appBar: AppBar(actions: const [SyncStatusIndicator()]),
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();

      expect(
        find.byTooltip(
          "1 unsynced recording - tap for details and to run sync",
        ),
        findsOneWidget,
      );

      await tester.tap(find.byType(IconButton).first);
      await tester.pumpAndSettle();

      expect(syncCalls, 1);
      expect(find.text("1 unsynced recording"), findsOneWidget);
      expect(
        find.text("May retry automatically or need manual attention"),
        findsOneWidget,
      );
      expect(find.text("Sync now"), findsOneWidget);
    },
  );

  testWidgets(
    "Given terminal failures are shown, When delete is cancelled and confirmed, Then the row stays until confirmation and then disappears",
    (WidgetTester tester) async {
      final countStream = _MutableStream<int>(1);
      final terminalFailure = _terminalFailureUpload(
        id: 7,
        audioPath: "/tmp/oracy_recording_7.wav",
        errorMessage: "Unsupported audio format.",
      );
      final terminalFailuresStream = _MutableStream<List<PendingUpload>>([
        terminalFailure,
      ]);
      var deleteCalls = 0;

      addTearDown(() async {
        await countStream.close();
        await terminalFailuresStream.close();
      });

      await tester.pumpWidget(
        ProviderScope(
          overrides: [
            pendingUploadCountProvider.overrideWith(
              (ref) => countStream.stream(),
            ),
            terminalFailureUploadsProvider.overrideWith(
              (ref) => terminalFailuresStream.stream(),
            ),
            terminalFailureDeleteActionProvider.overrideWithValue((
              PendingUpload upload,
            ) async {
              deleteCalls++;
              terminalFailuresStream.add(const <PendingUpload>[]);
              countStream.add(0);
            }),
            syncTriggerProvider.overrideWithValue(() async {}),
          ],
          child: const MaterialApp(home: Scaffold(body: SyncStatusPanel())),
        ),
      );
      await tester.pumpAndSettle();

      expect(find.text("oracy_recording_7.wav"), findsOneWidget);
      expect(find.text("Unsupported audio format."), findsOneWidget);
      expect(find.text("Delete recording"), findsOneWidget);

      await tester.tap(find.text("Delete recording"));
      await tester.pumpAndSettle();

      expect(find.text("Delete recording?"), findsOneWidget);
      await tester.tap(find.text("Cancel"));
      await tester.pumpAndSettle();

      expect(deleteCalls, 0);
      expect(find.text("oracy_recording_7.wav"), findsOneWidget);

      await tester.tap(find.text("Delete recording"));
      await tester.pumpAndSettle();
      await tester.tap(find.text("Delete"));
      await tester.pumpAndSettle();

      expect(deleteCalls, 1);
      expect(find.text("oracy_recording_7.wav"), findsNothing);
      expect(find.text("All synced"), findsOneWidget);
      expect(find.text("No unsynced recordings"), findsOneWidget);
    },
  );

  testWidgets(
    "Given no unsynced recordings, When sync status is shown, Then the zero state reflects that directly",
    (WidgetTester tester) async {
      await tester.pumpWidget(
        ProviderScope(
          overrides: [
            pendingUploadCountOverride(0),
            terminalFailureUploadsProvider.overrideWith(
              (ref) => Stream.value(const <PendingUpload>[]),
            ),
          ],
          child: const MaterialApp(home: Scaffold(body: SyncStatusPanel())),
        ),
      );
      await tester.pumpAndSettle();

      expect(find.text("All synced"), findsOneWidget);
      expect(find.text("No unsynced recordings"), findsOneWidget);
    },
  );
}
