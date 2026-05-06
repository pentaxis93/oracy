import 'dart:convert';
import 'dart:typed_data';

import 'package:dio/dio.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:oracy/services/api_client.dart';
import 'package:oracy/services/preferences_service.dart';
import 'package:shared_preferences/shared_preferences.dart';

import 'helpers/test_utils.dart';

class _CapturedRequest {
  final Map<String, dynamic> headers;

  const _CapturedRequest({required this.headers});
}

class _CapturingAdapter implements HttpClientAdapter {
  final requests = <_CapturedRequest>[];

  @override
  Future<ResponseBody> fetch(
    RequestOptions options,
    Stream<Uint8List>? requestStream,
    Future<void>? cancelFuture,
  ) async {
    requests.add(
      _CapturedRequest(headers: Map<String, dynamic>.from(options.headers)),
    );

    return ResponseBody.fromString(
      jsonEncode({'ok': true}),
      200,
      headers: {
        Headers.contentTypeHeader: [Headers.jsonContentType],
      },
    );
  }

  @override
  void close({bool force = false}) {}
}

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

      await service.setApiBaseUrlOverride(' HTTPS://staging.oracy.app/ ');

      expect(service.apiBaseUrlOverride, 'https://staging.oracy.app');
      expect(service.apiBaseUrl, 'https://staging.oracy.app');
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
        'https://staging.oracy.app/api',
        'https://staging.oracy.app/api#fragment',
      ]) {
        expect(() => normalizeApiBaseUrl(value), throwsFormatException);
      }
    },
  );

  test(
    'Given an origin URL with redundant casing and a trailing slash, When normalization runs, Then the origin is normalized',
    () {
      expect(
        normalizeApiBaseUrl(' HTTPS://STAGING.ORACY.APP/ '),
        'https://staging.oracy.app',
      );
    },
  );

  test(
    'Given a stored API key bound to a previous effective URL, When the effective URL is reconciled, Then the key is cleared and rebound',
    () async {
      final prefs = await SharedPreferences.getInstance();
      final service = PreferencesService(prefs);
      await service.markApiKeyBoundToCurrentUrl();
      await service.setApiBaseUrlOverride('https://staging.oracy.app');
      final storage = MockSecureStorage(apiKey: 'oracy_sk_existing');

      await service.reconcileApiCredentialBinding(
        hasApiKey: storage.hasApiKey,
        deleteApiKey: storage.deleteApiKey,
      );

      expect(await storage.hasApiKey(), isFalse);
      expect(service.lastEffectiveApiBaseUrl, 'https://staging.oracy.app');
    },
  );

  test(
    'Given no persisted URL and no API key, When the effective URL is reconciled, Then the current URL is recorded',
    () async {
      final prefs = await SharedPreferences.getInstance();
      final service = PreferencesService(prefs);
      final storage = MockSecureStorage();

      await service.reconcileApiCredentialBinding(
        hasApiKey: storage.hasApiKey,
        deleteApiKey: storage.deleteApiKey,
      );

      expect(await storage.hasApiKey(), isFalse);
      expect(service.lastEffectiveApiBaseUrl, service.apiBaseUrl);
    },
  );

  test(
    'Given no persisted URL and an API key, When the effective URL is reconciled, Then the uncertain key is cleared silently',
    () async {
      final prefs = await SharedPreferences.getInstance();
      final service = PreferencesService(prefs);
      final storage = MockSecureStorage(apiKey: 'oracy_sk_existing');

      await service.reconcileApiCredentialBinding(
        hasApiKey: storage.hasApiKey,
        deleteApiKey: storage.deleteApiKey,
      );

      expect(await storage.hasApiKey(), isFalse);
      expect(service.lastEffectiveApiBaseUrl, service.apiBaseUrl);
    },
  );

  test(
    'Given a previously persisted pathful override, When the effective URL is resolved, Then the invalid override is ignored',
    () async {
      SharedPreferences.setMockInitialValues({
        PreferenceKeys.apiBaseUrl: 'https://staging.oracy.app/api',
      });
      final prefs = await SharedPreferences.getInstance();
      final service = PreferencesService(prefs);

      expect(service.apiBaseUrlOverride, isNull);
      expect(service.apiBaseUrl, normalizeApiBaseUrl(kDefaultBaseUrl));
    },
  );

  test(
    'Given a runtime override keeps the effective URL stable, When the build default changes, Then the key remains bound',
    () async {
      final prefs = await SharedPreferences.getInstance();
      final service = PreferencesService(prefs);
      await service.setApiBaseUrlOverride('https://staging.oracy.app');
      await service.markApiKeyBoundToCurrentUrl();
      final storage = MockSecureStorage(apiKey: 'oracy_sk_existing');

      await service.reconcileApiCredentialBinding(
        hasApiKey: storage.hasApiKey,
        deleteApiKey: storage.deleteApiKey,
      );

      expect(await storage.hasApiKey(), isTrue);
      expect(service.lastEffectiveApiBaseUrl, 'https://staging.oracy.app');
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

  test(
    'Given a stored API key bound to a previous effective URL, When a Dio request is prepared, Then the stale key is not sent',
    () async {
      final prefs = await SharedPreferences.getInstance();
      final preferences = PreferencesService(prefs);
      await preferences.markApiKeyBoundToCurrentUrl();
      await preferences.setApiBaseUrlOverride('https://staging.oracy.app');
      final storage = MockSecureStorage(apiKey: 'oracy_sk_existing');
      final adapter = _CapturingAdapter();
      final dio = ApiClientFactory(
        storage,
        reconcileCredentialBinding: () =>
            preferences.reconcileApiCredentialBinding(
              hasApiKey: storage.hasApiKey,
              deleteApiKey: storage.deleteApiKey,
            ),
        config: ApiClientConfig(baseUrl: preferences.apiBaseUrl),
      ).createClient()..httpClientAdapter = adapter;

      await dio.get('/api/v1/voice-notes');

      expect(await storage.hasApiKey(), isFalse);
      expect(adapter.requests.single.headers, isNot(contains('Authorization')));
    },
  );
}
