import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:oracy/app.dart';
import 'package:oracy/screens/transcript_result_screen.dart';
import 'package:oracy/services/home_widget_service.dart';
import 'package:oracy/services/permission_service.dart';
import 'package:oracy/services/recording_service.dart';
import 'package:oracy/services/transcription_service.dart';

import 'helpers/test_utils.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  const homeWidgetChannel = MethodChannel('home_widget');
  const homeWidgetUpdatesChannel = MethodChannel('home_widget/updates');
  const oracyWidgetChannel = MethodChannel('app.oracy.oracy/widget');

  Future<void> sendWidgetRecordRequest() async {
    final messenger =
        TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;
    await messenger.handlePlatformMessage(
      oracyWidgetChannel.name,
      oracyWidgetChannel.codec.encodeMethodCall(
        const MethodCall('startRecordingFromWidget'),
      ),
      (ByteData? _) {},
    );
  }

  setUp(() {
    final messenger =
        TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;
    messenger.setMockMethodCallHandler(homeWidgetChannel, (_) async => true);
    messenger.setMockMethodCallHandler(
      homeWidgetUpdatesChannel,
      (_) async => null,
    );
    messenger.setMockMethodCallHandler(oracyWidgetChannel, (call) async {
      if (call.method == 'consumePendingRecordIntent') {
        return false;
      }
      return null;
    });
    HomeWidgetService.setOnRecordCallback(null);
  });

  tearDown(() {
    final messenger =
        TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;
    messenger.setMockMethodCallHandler(homeWidgetChannel, null);
    messenger.setMockMethodCallHandler(homeWidgetUpdatesChannel, null);
    messenger.setMockMethodCallHandler(oracyWidgetChannel, null);
    HomeWidgetService.setOnRecordCallback(null);
  });

  testWidgets(
    'Given microphone permission is denied, When the Android widget requests recording, Then permission is requested before recording starts',
    (tester) async {
      final permissions = MockPermissionService(
        status: MicrophonePermissionStatus.denied,
      );
      final recordingNotifier = MockRecordingNotifier();

      await HomeWidgetService.initialize();
      await tester.pumpWidget(
        ProviderScope(
          overrides: [
            pendingUploadCountOverride(0),
            transcriptionOverride(),
            permissionOverride(permissions),
            recordingProvider.overrideWith(() => recordingNotifier),
          ],
          child: const OracyApp(),
        ),
      );
      await tester.pump();

      await sendWidgetRecordRequest();
      await tester.pump();
      await tester.pump(const Duration(milliseconds: 100));

      expect(permissions.checkCount, greaterThanOrEqualTo(1));
      expect(recordingNotifier.startCount, 0);
      expect(find.text('Microphone Access Required'), findsOneWidget);
    },
  );

  testWidgets(
    'Given another route is open, When the Android widget starts recording, Then Home shows the recording controls',
    (tester) async {
      final permissions = MockPermissionService();
      final recordingNotifier = MockRecordingNotifier();

      await HomeWidgetService.initialize();
      await tester.pumpWidget(
        ProviderScope(
          overrides: [
            pendingUploadCountOverride(0),
            transcriptionOverride(),
            permissionOverride(permissions),
            recordingProvider.overrideWith(() => recordingNotifier),
          ],
          child: const OracyApp(),
        ),
      );
      await tester.pump();

      final homeContext = tester.element(find.byType(HomePage));
      unawaited(
        Navigator.of(homeContext).push(
          MaterialPageRoute<void>(
            builder: (_) => const Scaffold(body: Text('Covered Home')),
          ),
        ),
      );
      await tester.pumpAndSettle();
      expect(find.text('Covered Home'), findsOneWidget);

      await sendWidgetRecordRequest();
      await tester.pumpAndSettle();

      expect(permissions.checkCount, greaterThanOrEqualTo(1));
      expect(recordingNotifier.startCount, 1);
      expect(find.text('Tap to Record'), findsOneWidget);
      expect(find.byIcon(Icons.stop_rounded), findsOneWidget);
      expect(find.text('Covered Home'), findsNothing);
    },
  );

  testWidgets(
    'Given microphone permission is denied, When New Recording is tapped from a transcript, Then the result screen remains and recording does not start',
    (tester) async {
      final permissions = MockPermissionService(
        status: MicrophonePermissionStatus.denied,
      );
      final recordingNotifier = MockRecordingNotifier();

      await tester.pumpWidget(
        ProviderScope(
          overrides: [
            transcriptionOverride(TranscriptionSuccess(createMockTranscript())),
            permissionOverride(permissions),
            recordingProvider.overrideWith(() => recordingNotifier),
          ],
          child: MaterialApp(
            home: Builder(
              builder: (context) => TextButton(
                onPressed: () => Navigator.of(context).push(
                  MaterialPageRoute<void>(
                    builder: (_) => TranscriptResultScreen(
                      transcript: createMockTranscript(
                        transcript: 'Saved result',
                      ),
                    ),
                  ),
                ),
                child: const Text('Open result'),
              ),
            ),
          ),
        ),
      );
      await tester.tap(find.text('Open result'));
      await tester.pumpAndSettle();

      await tester.tap(find.text('New Recording'));
      await tester.pump();
      await tester.pump(const Duration(milliseconds: 100));

      expect(permissions.checkCount, greaterThanOrEqualTo(1));
      expect(recordingNotifier.startCount, 0);
      expect(find.text('Transcript'), findsOneWidget);
      expect(find.text('Microphone Access Required'), findsOneWidget);
    },
  );

  testWidgets(
    'Given microphone permission is granted after a completed recording, When New Recording is tapped from a transcript, Then recording starts and returns home',
    (tester) async {
      final permissions = MockPermissionService();
      final recordingNotifier = MockRecordingNotifier(
        const RecordingInfo(
          state: RecordingState.completed,
          filePath: '/mock/previous.m4a',
        ),
      );

      await tester.pumpWidget(
        ProviderScope(
          overrides: [
            transcriptionOverride(TranscriptionSuccess(createMockTranscript())),
            permissionOverride(permissions),
            recordingProvider.overrideWith(() => recordingNotifier),
          ],
          child: MaterialApp(
            home: Builder(
              builder: (context) => TextButton(
                onPressed: () => Navigator.of(context).push(
                  MaterialPageRoute<void>(
                    builder: (_) => TranscriptResultScreen(
                      transcript: createMockTranscript(
                        transcript: 'Saved result',
                      ),
                    ),
                  ),
                ),
                child: const Text('Open result'),
              ),
            ),
          ),
        ),
      );
      await tester.tap(find.text('Open result'));
      await tester.pumpAndSettle();

      await tester.tap(find.text('New Recording'));
      await tester.pumpAndSettle();

      expect(permissions.checkCount, greaterThanOrEqualTo(1));
      expect(recordingNotifier.startCount, 1);
      expect(find.text('Open result'), findsOneWidget);
      expect(find.text('Transcript'), findsNothing);
    },
  );
}
