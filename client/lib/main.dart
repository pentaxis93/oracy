import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:oracy/app.dart';
import 'package:oracy/db/database.dart';
import 'package:oracy/services/background_sync_service.dart';
import 'package:oracy/services/home_widget_service.dart';
import 'package:oracy/services/preferences_service.dart';
import 'package:oracy/services/recording_recovery_service.dart';
import 'package:shared_preferences/shared_preferences.dart';

void main() async {
  WidgetsFlutterBinding.ensureInitialized();

  // Debug log for web
  if (kDebugMode && kIsWeb) {
    print('[MAIN] Starting Oracy web app...');
  }

  // Initialize SharedPreferences
  final sharedPreferences = await SharedPreferences.getInstance();

  // Initialize background sync with workmanager (mobile only)
  if (!kIsWeb) {
    await BackgroundSyncService.initialize();

    // Initialize home widget (mobile only)
    await HomeWidgetService.initialize();

    // Check for and recover any interrupted recordings from a previous crash
    final recoveredPath =
        await RecordingRecoveryService.checkForInterruptedRecording();
    if (recoveredPath != null) {
      if (kDebugMode) {
        debugPrint('[MAIN] Recovered interrupted recording: $recoveredPath');
      }

      final db = AppDatabase();
      try {
        await db.ensurePendingUpload(audioPath: recoveredPath);
      } finally {
        await db.close();
      }
    }

    // Clean up old orphaned recording files (older than 1 day)
    await RecordingRecoveryService.cleanupOrphanedRecordings();
  }

  if (kDebugMode && kIsWeb) {
    print('[MAIN] About to run app...');
  }

  runApp(
    ProviderScope(
      overrides: [
        sharedPreferencesProvider.overrideWithValue(sharedPreferences),
      ],
      child: const OracyApp(),
    ),
  );

  if (kDebugMode && kIsWeb) {
    print('[MAIN] App started!');
  }
}
