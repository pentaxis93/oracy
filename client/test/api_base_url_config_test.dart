import 'package:flutter_test/flutter_test.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:oracy/services/api_client.dart';
import 'package:oracy/services/preferences_service.dart';
import 'package:shared_preferences/shared_preferences.dart';

void main() {
  setUp(() {
    SharedPreferences.setMockInitialValues({});
  });

  test(
    'Given no operator configuration, When the API base URL is resolved, Then the documented default is used',
    () async {
      final prefs = await SharedPreferences.getInstance();
      final service = PreferencesService(prefs);

      expect(service.apiBaseUrl, normalizeApiBaseUrl(kDefaultBaseUrl));
      expect(service.apiBaseUrlOverride, isNull);
    },
  );

  test(
    'Given a build-time API base URL define, When the default is read, Then it follows that define',
    () {
      const buildTimeDefault = String.fromEnvironment(
        'ORACY_API_BASE_URL',
        defaultValue: 'https://api.oracy.app',
      );

      expect(kDefaultBaseUrl, buildTimeDefault);
    },
  );

  test(
    'Given an operator override, When the API base URL is resolved, Then the normalized override wins',
    () async {
      final prefs = await SharedPreferences.getInstance();
      final service = PreferencesService(prefs);

      await service.setApiBaseUrlOverride(' HTTPS://staging.oracy.app/api/ ');

      expect(service.apiBaseUrlOverride, 'https://staging.oracy.app/api');
      expect(service.apiBaseUrl, 'https://staging.oracy.app/api');
    },
  );

  test(
    'Given invalid API base URL values, When normalization runs, Then they are rejected',
    () {
      for (final value in [
        '',
        '/api',
        'ftp://staging.oracy.app',
        'https://user:pass@staging.oracy.app',
        'https://staging.oracy.app/api?token=abc',
        'https://staging.oracy.app/api#fragment',
      ]) {
        expect(() => normalizeApiBaseUrl(value), throwsFormatException);
      }
    },
  );

  test(
    'Given an API base URL override, When a Dio client is created, Then requests use that override',
    () async {
      final prefs = await SharedPreferences.getInstance();
      await PreferencesService(
        prefs,
      ).setApiBaseUrlOverride('https://staging.oracy.app');

      final container = ProviderContainer(
        overrides: [sharedPreferencesProvider.overrideWithValue(prefs)],
      );
      addTearDown(container.dispose);

      final dio = container.read(apiClientProvider);

      expect(dio.options.baseUrl, 'https://staging.oracy.app');
    },
  );
}
