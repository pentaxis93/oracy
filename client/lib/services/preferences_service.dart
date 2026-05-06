import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:oracy/services/api_base_url_config.dart';
import 'package:shared_preferences/shared_preferences.dart';

/// Keys for shared preferences.
class PreferenceKeys {
  static const String autoCopyToClipboard = 'auto_copy_to_clipboard';
  static const String apiBaseUrl = 'api_base_url';
}

/// Service for managing user preferences.
class PreferencesService {
  final SharedPreferences _prefs;

  PreferencesService(this._prefs);

  /// Whether to automatically copy transcripts to clipboard.
  /// Default: true (enabled)
  bool get autoCopyToClipboard =>
      _prefs.getBool(PreferenceKeys.autoCopyToClipboard) ?? true;

  set autoCopyToClipboard(bool value) =>
      _prefs.setBool(PreferenceKeys.autoCopyToClipboard, value);

  /// Runtime override for the API base URL, if one has been configured.
  String? get apiBaseUrlOverride {
    final value = _prefs.getString(PreferenceKeys.apiBaseUrl);
    if (value == null || value.trim().isEmpty) {
      return null;
    }
    return normalizeApiBaseUrl(value);
  }

  /// Effective API base URL for this install.
  String get apiBaseUrl =>
      apiBaseUrlOverride ?? normalizeApiBaseUrl(kDefaultBaseUrl);

  /// Store a runtime API base URL override.
  Future<void> setApiBaseUrlOverride(String value) async {
    await _prefs.setString(
      PreferenceKeys.apiBaseUrl,
      normalizeApiBaseUrl(value),
    );
  }

  /// Remove the runtime API base URL override.
  Future<void> clearApiBaseUrlOverride() async {
    await _prefs.remove(PreferenceKeys.apiBaseUrl);
  }
}

/// Provider for SharedPreferences instance.
/// Must be overridden in main.dart after initialization.
final sharedPreferencesProvider = Provider<SharedPreferences>((ref) {
  throw UnimplementedError('SharedPreferences not initialized');
});

/// Provider for preferences service.
final preferencesServiceProvider = Provider<PreferencesService>((ref) {
  final prefs = ref.watch(sharedPreferencesProvider);
  return PreferencesService(prefs);
});

/// Notifier for auto-copy setting.
class AutoCopyNotifier extends Notifier<bool> {
  @override
  bool build() {
    final prefsService = ref.watch(preferencesServiceProvider);
    return prefsService.autoCopyToClipboard;
  }

  void toggle(bool value) {
    final prefsService = ref.read(preferencesServiceProvider);
    prefsService.autoCopyToClipboard = value;
    state = value;
  }
}

/// Provider for auto-copy setting (reactive).
final autoCopyEnabledProvider = NotifierProvider<AutoCopyNotifier, bool>(
  AutoCopyNotifier.new,
);
