import 'package:dio/dio.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_secure_storage/flutter_secure_storage.dart';
import 'package:oracy/services/api_base_url_config.dart';
import 'package:oracy/services/preferences_service.dart';

export 'package:oracy/services/api_base_url_config.dart'
    show kDefaultBaseUrl, normalizeApiBaseUrl;

/// Key used to store the API key in secure storage.
const kApiKeyStorageKey = 'oracy_api_key';

/// Service for secure storage of sensitive data.
class SecureStorageService {
  final FlutterSecureStorage _storage;

  SecureStorageService() : _storage = const FlutterSecureStorage();

  /// Get the stored API key.
  Future<String?> getApiKey() async {
    return await _storage.read(key: kApiKeyStorageKey);
  }

  /// Store the API key.
  Future<void> setApiKey(String apiKey) async {
    await _storage.write(key: kApiKeyStorageKey, value: apiKey);
  }

  /// Delete the stored API key.
  Future<void> deleteApiKey() async {
    await _storage.delete(key: kApiKeyStorageKey);
  }

  /// Check if an API key is stored.
  Future<bool> hasApiKey() async {
    final key = await getApiKey();
    return key != null && key.isNotEmpty;
  }
}

/// Provider for secure storage service.
final secureStorageProvider = Provider<SecureStorageService>((ref) {
  return SecureStorageService();
});

/// Interceptor that adds Bearer token authentication to requests.
class AuthInterceptor extends Interceptor {
  final SecureStorageService _storage;
  final void Function()? onAuthError;

  AuthInterceptor(this._storage, {this.onAuthError});

  @override
  void onRequest(
    RequestOptions options,
    RequestInterceptorHandler handler,
  ) async {
    final apiKey = await _storage.getApiKey();
    if (apiKey != null && apiKey.isNotEmpty) {
      options.headers['Authorization'] = 'Bearer $apiKey';
    }
    handler.next(options);
  }

  @override
  void onError(DioException err, ErrorInterceptorHandler handler) {
    if (err.response?.statusCode == 401) {
      // Unauthorized - trigger re-auth flow
      onAuthError?.call();
    }
    handler.next(err);
  }
}

/// Configuration for the API client.
class ApiClientConfig {
  final String baseUrl;
  final Duration connectTimeout;
  final Duration receiveTimeout;
  final Duration sendTimeout;

  const ApiClientConfig({
    this.baseUrl = kDefaultBaseUrl,
    this.connectTimeout = const Duration(seconds: 30),
    this.receiveTimeout = const Duration(
      seconds: 120,
    ), // Long for transcription
    this.sendTimeout = const Duration(seconds: 120), // Long for upload
  });
}

/// Factory for creating configured Dio instances.
class ApiClientFactory {
  final SecureStorageService _storage;
  final ApiClientConfig config;
  void Function()? onAuthError;

  ApiClientFactory(this._storage, {this.config = const ApiClientConfig()});

  /// Create a new Dio instance with authentication.
  Dio createClient() {
    final dio = Dio(
      BaseOptions(
        baseUrl: config.baseUrl,
        connectTimeout: config.connectTimeout,
        receiveTimeout: config.receiveTimeout,
        sendTimeout: config.sendTimeout,
        headers: {
          'Accept': 'application/json',
          'Content-Type': 'application/json',
        },
      ),
    );

    // Add auth interceptor
    dio.interceptors.add(AuthInterceptor(_storage, onAuthError: onAuthError));

    // Add logging in debug mode
    assert(() {
      dio.interceptors.add(
        LogInterceptor(
          requestBody: true,
          responseBody: true,
          logPrint: (o) {
            if (kDebugMode) {
              debugPrint('[DIO] $o');
            }
          },
        ),
      );
      return true;
    }());

    return dio;
  }
}

/// Provider for API client factory.
final apiClientFactoryProvider = Provider<ApiClientFactory>((ref) {
  final storage = ref.watch(secureStorageProvider);
  final preferences = ref.watch(preferencesServiceProvider);
  return ApiClientFactory(
    storage,
    config: ApiClientConfig(baseUrl: preferences.apiBaseUrl),
  );
});

/// Provider for the main Dio client.
///
/// This is the primary way to get an HTTP client for API calls.
final apiClientProvider = Provider<Dio>((ref) {
  final factory = ref.watch(apiClientFactoryProvider);
  return factory.createClient();
});

/// Provider for checking if the user has configured an API key.
final hasApiKeyProvider = FutureProvider<bool>((ref) async {
  final storage = ref.watch(secureStorageProvider);
  return storage.hasApiKey();
});
