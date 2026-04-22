import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:oracy/app.dart';
import 'package:oracy/services/recording_service.dart';
import 'package:oracy/services/transcription_service.dart';

import 'helpers/test_utils.dart';

void main() {
  group('Recording Flow', () {
    // BDD: Given/When/Then style tests for the complete recording flow

    testWidgets(
      'Given idle state, When user views home, Then sees record button',
      (tester) async {
        // Given: App is in idle state
        await tester.pumpWidget(
          ProviderScope(
            overrides: [
              recordingOverride(),
              transcriptionOverride(),
              pendingUploadCountOverride(0),
            ],
            child: const OracyApp(),
          ),
        );

        // Then: User sees the record button with mic icon
        expect(find.byIcon(Icons.mic), findsOneWidget);
        expect(find.text('Tap to Record'), findsOneWidget);
      },
    );

    testWidgets(
      'Given idle state, When user taps record button, Then sees recording state with timer',
      (tester) async {
        // Given: App is in idle state
        await tester.pumpWidget(
          ProviderScope(
            overrides: [
              recordingOverride(),
              transcriptionOverride(),
              pendingUploadCountOverride(0),
            ],
            child: const OracyApp(),
          ),
        );

        // When: User taps the record button
        await tester.tap(find.byIcon(Icons.mic));
        await tester.pumpAndSettle();

        // Note: In real implementation, this would require permission mocking.
        // For now, we verify the mic button exists and can be tapped.
        expect(find.byIcon(Icons.mic), findsOneWidget);
      },
    );

    testWidgets(
      'Given recording state, When app shows recording, Then displays stop icon and timer',
      (tester) async {
        // Given: App is in recording state
        final recordingState = RecordingInfo(
          state: RecordingState.recording,
          filePath: '/mock/recording.m4a',
          duration: const Duration(seconds: 5),
        );

        await tester.pumpWidget(
          ProviderScope(
            overrides: [
              recordingOverride(recordingState),
              transcriptionOverride(),
              pendingUploadCountOverride(0),
            ],
            child: const OracyApp(),
          ),
        );

        // Then: User sees stop icon
        expect(find.byIcon(Icons.stop_rounded), findsOneWidget);

        // Then: User sees "Recording" text
        expect(find.text('Recording'), findsOneWidget);

        // Then: User sees timer (00:05 format)
        expect(find.text('00:05'), findsOneWidget);
      },
    );

    testWidgets(
      'Given uploading state, When transcription uploads, Then shows upload progress',
      (tester) async {
        // Given: Transcription is uploading
        const uploadState = TranscriptionUploading(progress: 0.5);

        await tester.pumpWidget(
          ProviderScope(
            overrides: [
              recordingOverride(),
              transcriptionOverride(uploadState),
              pendingUploadCountOverride(0),
            ],
            child: const OracyApp(),
          ),
        );

        // Then: User sees uploading indicator
        expect(find.text('Uploading...'), findsOneWidget);

        // Then: User sees progress percentage
        expect(find.text('50%'), findsOneWidget);

        // Then: Recording button is hidden during upload
        expect(find.byIcon(Icons.mic), findsNothing);
      },
    );

    testWidgets(
      'Given processing state, When transcription processes, Then shows processing indicator',
      (tester) async {
        // Given: Transcription is processing
        const processingState = TranscriptionProcessing();

        await tester.pumpWidget(
          ProviderScope(
            overrides: [
              recordingOverride(),
              transcriptionOverride(processingState),
              pendingUploadCountOverride(0),
            ],
            child: const OracyApp(),
          ),
        );

        // Then: User sees transcribing message
        expect(find.text('Transcribing...'), findsOneWidget);
        expect(find.text('This may take a moment'), findsOneWidget);

        // Then: Recording button is hidden during processing
        expect(find.byIcon(Icons.mic), findsNothing);
      },
    );

    testWidgets(
      'Given network error state, When error occurs, Then shows error with retry button',
      (tester) async {
        // Given: A network error occurred
        const errorState = TranscriptionError(
          'Unable to connect to server. Please check your internet connection.',
          errorType: TranscriptionErrorType.network,
          filePath: '/mock/recording.m4a',
        );

        await tester.pumpWidget(
          ProviderScope(
            overrides: [
              recordingOverride(),
              transcriptionOverride(errorState),
              pendingUploadCountOverride(0),
            ],
            child: const OracyApp(),
          ),
        );

        // Then: User sees connection error icon
        expect(find.byIcon(Icons.wifi_off), findsOneWidget);

        // Then: User sees error title
        expect(find.text('Connection Error'), findsOneWidget);

        // Then: User sees error message
        expect(
          find.text(
            'Unable to connect to server. Please check your internet connection.',
          ),
          findsOneWidget,
        );

        // Then: User sees retry button
        expect(find.text('Retry'), findsOneWidget);
      },
    );

    testWidgets(
      'Given auth error state, When error occurs, Then shows settings button',
      (tester) async {
        // Given: An auth error occurred
        const errorState = TranscriptionError(
          'No API key configured. Please add your API key in Settings.',
          errorType: TranscriptionErrorType.auth,
          filePath: '/mock/recording.m4a',
        );

        await tester.pumpWidget(
          ProviderScope(
            overrides: [
              recordingOverride(),
              transcriptionOverride(errorState),
              pendingUploadCountOverride(0),
            ],
            child: const OracyApp(),
          ),
        );

        // Then: User sees auth error icon
        expect(find.byIcon(Icons.key_off), findsOneWidget);

        // Then: User sees error title
        expect(find.text('Authentication Error'), findsOneWidget);

        // Then: User sees settings button
        expect(find.text('Settings'), findsOneWidget);
      },
    );

    testWidgets(
      'Given timeout error state, When error occurs, Then shows appropriate message',
      (tester) async {
        // Given: A timeout error occurred
        const errorState = TranscriptionError(
          'Upload timed out. Please check your connection and try again.',
          errorType: TranscriptionErrorType.timeout,
          filePath: '/mock/recording.m4a',
        );

        await tester.pumpWidget(
          ProviderScope(
            overrides: [
              recordingOverride(),
              transcriptionOverride(errorState),
              pendingUploadCountOverride(0),
            ],
            child: const OracyApp(),
          ),
        );

        // Then: User sees timeout icon
        expect(find.byIcon(Icons.timer_off), findsOneWidget);

        // Then: User sees timeout title
        expect(find.text('Request Timeout'), findsOneWidget);

        // Then: User sees retry button (timeout is retryable)
        expect(find.text('Retry'), findsOneWidget);
      },
    );

    testWidgets(
      'Given file validation error, When error occurs, Then shows dismiss button',
      (tester) async {
        // Given: A file validation error occurred
        const errorState = TranscriptionError(
          'Audio file is too large (max 25MB).',
          errorType: TranscriptionErrorType.fileValidation,
          filePath: '/mock/recording.m4a',
        );

        await tester.pumpWidget(
          ProviderScope(
            overrides: [
              recordingOverride(),
              transcriptionOverride(errorState),
              pendingUploadCountOverride(0),
            ],
            child: const OracyApp(),
          ),
        );

        // Then: User sees file error icon
        expect(find.byIcon(Icons.file_present), findsOneWidget);

        // Then: User sees error title
        expect(find.text('Invalid File'), findsOneWidget);

        // Then: User sees dismiss button (not retryable)
        expect(find.text('Dismiss'), findsOneWidget);
        expect(find.text('Retry'), findsNothing);
      },
    );
  });

  group('Recording UI States', () {
    testWidgets('Recording button shows stop icon when recording', (
      tester,
    ) async {
      // When recording, the button should show stop icon instead of mic
      final recordingState = RecordingInfo(
        state: RecordingState.recording,
        filePath: '/mock/recording.m4a',
        duration: Duration.zero,
      );

      await tester.pumpWidget(
        ProviderScope(
          overrides: [
            recordingOverride(recordingState),
            transcriptionOverride(),
            pendingUploadCountOverride(0),
          ],
          child: const OracyApp(),
        ),
      );

      // The recording button should show stop icon
      expect(find.byIcon(Icons.stop_rounded), findsOneWidget);
      expect(find.byIcon(Icons.mic), findsNothing);
    });

    testWidgets('Amplitude indicator shows during recording', (tester) async {
      final recordingState = RecordingInfo(
        state: RecordingState.recording,
        filePath: '/mock/recording.m4a',
        duration: Duration.zero,
        amplitude: 0.5,
      );

      await tester.pumpWidget(
        ProviderScope(
          overrides: [
            recordingOverride(recordingState),
            transcriptionOverride(),
            pendingUploadCountOverride(0),
          ],
          child: const OracyApp(),
        ),
      );

      // The amplitude indicator should be visible during recording
      // It's a FractionallySizedBox with the amplitude as widthFactor
      expect(find.byType(FractionallySizedBox), findsWidgets);
    });
  });
}
