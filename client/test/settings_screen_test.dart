import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:oracy/screens/settings_screen.dart';
import 'package:oracy/services/api_client.dart';
import 'package:oracy/services/preferences_service.dart';
import 'package:shared_preferences/shared_preferences.dart';

import 'helpers/test_utils.dart';

void main() {
  setUp(() {
    SharedPreferences.setMockInitialValues({});
  });

  Future<void> pumpSettings(
    WidgetTester tester, {
    required SharedPreferences prefs,
    required MockSecureStorage storage,
  }) async {
    await tester.pumpWidget(
      ProviderScope(
        overrides: [
          sharedPreferencesProvider.overrideWithValue(prefs),
          secureStorageProvider.overrideWith((_) => storage),
        ],
        child: const MaterialApp(home: SettingsScreen()),
      ),
    );
    await tester.pumpAndSettle();
    await tester.drag(
      find.byType(SingleChildScrollView),
      const Offset(0, -500),
    );
    await tester.pumpAndSettle();
  }

  testWidgets(
    'Given an existing API key, When the server URL is changed and confirmed, Then the override is stored and the API key is cleared',
    (tester) async {
      final prefs = await SharedPreferences.getInstance();
      final storage = MockSecureStorage(apiKey: 'oracy_sk_existing');

      await pumpSettings(tester, prefs: prefs, storage: storage);

      await tester.enterText(
        find.widgetWithText(TextFormField, 'Server URL'),
        ' https://staging.oracy.app/api/ ',
      );
      await tester.tap(find.text('Save Server URL'));
      await tester.pumpAndSettle();
      await tester.tap(find.widgetWithText(FilledButton, 'Change Server'));
      await tester.pumpAndSettle();

      final service = PreferencesService(prefs);
      expect(service.apiBaseUrlOverride, 'https://staging.oracy.app/api');
      expect(await storage.hasApiKey(), isFalse);
      expect(find.text('API Key Configured'), findsNothing);
    },
  );

  testWidgets(
    'Given a server URL override, When reset to default is confirmed, Then the override is removed and the API key is cleared',
    (tester) async {
      final prefs = await SharedPreferences.getInstance();
      final service = PreferencesService(prefs);
      await service.setApiBaseUrlOverride('https://staging.oracy.app');
      final storage = MockSecureStorage(apiKey: 'oracy_sk_existing');

      await pumpSettings(tester, prefs: prefs, storage: storage);

      await tester.tap(find.text('Reset to Default'));
      await tester.pumpAndSettle();
      await tester.tap(find.widgetWithText(FilledButton, 'Change Server'));
      await tester.pumpAndSettle();

      expect(service.apiBaseUrlOverride, isNull);
      expect(service.apiBaseUrl, kDefaultBaseUrl);
      expect(await storage.hasApiKey(), isFalse);
    },
  );

  testWidgets(
    'Given an invalid server URL, When saving the server setting, Then validation blocks the change',
    (tester) async {
      final prefs = await SharedPreferences.getInstance();
      final storage = MockSecureStorage(apiKey: 'oracy_sk_existing');

      await pumpSettings(tester, prefs: prefs, storage: storage);

      await tester.enterText(
        find.widgetWithText(TextFormField, 'Server URL'),
        'ftp://staging.oracy.app',
      );
      await tester.tap(find.text('Save Server URL'));
      await tester.pumpAndSettle();

      expect(find.text('Server URL must use http or https.'), findsOneWidget);
      expect(PreferencesService(prefs).apiBaseUrlOverride, isNull);
      expect(await storage.hasApiKey(), isTrue);
      expect(find.text('Change Server'), findsNothing);
    },
  );
}
