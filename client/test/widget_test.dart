import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:oracy/app.dart';
import 'helpers/test_utils.dart';

void main() {
  testWidgets('App renders with ProviderScope', (WidgetTester tester) async {
    // Build our app wrapped in ProviderScope and trigger a frame.
    await tester.pumpWidget(
      ProviderScope(
        overrides: [pendingUploadCountOverride(0)],
        child: const OracyApp(),
      ),
    );

    // Verify that the app title is displayed.
    expect(find.text('Oracy'), findsOneWidget);

    // Verify the recording prompt is displayed.
    expect(find.text('Tap to Record'), findsOneWidget);

    // Verify the subtitle is present.
    expect(find.text('Your voice, transcribed'), findsOneWidget);
  });

  testWidgets('Recording button is displayed', (WidgetTester tester) async {
    await tester.pumpWidget(
      ProviderScope(
        overrides: [pendingUploadCountOverride(0)],
        child: const OracyApp(),
      ),
    );

    // The recording button contains a mic icon
    expect(find.byIcon(Icons.mic), findsOneWidget);
  });
}
