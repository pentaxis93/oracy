import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:shared_preferences/shared_preferences.dart';

/// Keys for shared preferences.
class PreferenceKeys {
  static const String autoCopyToClipboard = 'auto_copy_to_clipboard';
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
