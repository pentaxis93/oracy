import 'dart:io';

import 'package:connectivity_plus/connectivity_plus.dart';
import 'package:dio/dio.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter_secure_storage/flutter_secure_storage.dart';
import 'package:oracy/db/database.dart';
import 'package:oracy/services/api_client.dart';
import 'package:oracy/services/recording_recovery_service.dart';
import 'package:oracy/services/transcription_service.dart';
import 'package:oracy/services/upload_retry_policy.dart';
import 'package:workmanager/workmanager.dart';

/// Unique name for the background sync task.
const String backgroundSyncTaskName = 'oracy.backgroundSync';

/// Unique name for the one-off sync task (triggered when connectivity changes).
const String backgroundSyncOneOffTaskName = 'oracy.backgroundSyncOneOff';

/// Minimum interval between periodic sync attempts (Android limitation: min 15 min).
const Duration periodicSyncInterval = Duration(minutes: 15);

/// Age beyond which an `uploading` row is considered stranded in background sync.
/// Must exceed the Dio receiveTimeout above. A row can only stay
/// in `uploading` longer than the receive timeout if the process
/// died - in which case it is genuinely stranded.
const Duration staleUploadingRestoreThreshold = Duration(minutes: 10);

typedef ConnectivityCheck = Future<List<ConnectivityResult>> Function();
typedef ApiKeyReader = Future<String?> Function();
typedef BackgroundTranscriptionServiceFactory =
    TranscriptionService Function(String apiKey);
typedef BackgroundRecordingReconciler = Future<int> Function(AppDatabase db);

/// Top-level callback dispatcher for workmanager.
///
/// This must be a top-level function and cannot be a class method.
/// It runs in a separate isolate, so it cannot access Riverpod providers.
@pragma('vm:entry-point')
void callbackDispatcher() {
  Workmanager().executeTask((taskName, inputData) async {
    debugPrint('Background sync task started: $taskName');

    try {
      final connectivity = Connectivity();
      const storage = FlutterSecureStorage();
      final db = AppDatabase();

      try {
        return await runBackgroundSyncPass(
          db: db,
          checkConnectivity: connectivity.checkConnectivity,
          readApiKey: () => storage.read(key: kApiKeyStorageKey),
          createTranscriptionService: (apiKey) {
            final dio = Dio(
              BaseOptions(
                baseUrl: kDefaultBaseUrl,
                headers: {'Authorization': 'Bearer $apiKey'},
                connectTimeout: const Duration(seconds: 30),
                receiveTimeout: const Duration(minutes: 5),
                sendTimeout: const Duration(minutes: 2),
              ),
            );
            return TranscriptionService(dio);
          },
        );
      } finally {
        await db.close();
      }
    } catch (e, stackTrace) {
      debugPrint('Background sync error: $e');
      debugPrint('Stack trace: $stackTrace');
      return false; // Will be rescheduled
    }
  });
}

Future<bool> runBackgroundSyncPass({
  required AppDatabase db,
  required ConnectivityCheck checkConnectivity,
  required ApiKeyReader readApiKey,
  required BackgroundTranscriptionServiceFactory createTranscriptionService,
  int maxRetries = maxRetryAttempts,
  LocalFileDeleter deleteLocalFile = defaultLocalFileDeleter,
  BackgroundRecordingReconciler reconcilePersistentRecordings =
      RecordingRecoveryService.reconcilePersistentRecordings,
}) async {
  await retryPendingUploadCleanup(db, deleteLocalFile: deleteLocalFile);
  await db.restoreStaleUploadingUploads(staleUploadingRestoreThreshold);
  await reconcilePersistentRecordings(db);

  final connectivityResult = await checkConnectivity();
  if (!_isConnected(connectivityResult)) {
    debugPrint('Background sync: No connectivity, skipping');
    return true;
  }

  final apiKey = await readApiKey();
  if (apiKey == null || apiKey.isEmpty) {
    debugPrint('Background sync: No API key configured, skipping');
    return true;
  }

  final pendingUploads = await db.getPendingUploads(maxRetries: maxRetries);
  if (pendingUploads.isEmpty) {
    debugPrint('Background sync: No pending uploads');
    return true;
  }

  debugPrint('Background sync: Found ${pendingUploads.length} pending uploads');

  final transcriptionService = createTranscriptionService(apiKey);

  for (final upload in pendingUploads) {
    final currentConnectivity = await checkConnectivity();
    if (!_isConnected(currentConnectivity)) {
      debugPrint('Background sync: Lost connectivity, stopping');
      break;
    }

    await _processUpload(
      db,
      transcriptionService,
      upload,
      deleteLocalFile: deleteLocalFile,
    );
  }

  debugPrint('Background sync: Completed');
  return true;
}

/// Process a single upload in background.
Future<void> _processUpload(
  AppDatabase db,
  TranscriptionService transcriptionService,
  PendingUpload upload, {
  required LocalFileDeleter deleteLocalFile,
}) async {
  // Check if file still exists
  final file = File(upload.audioPath);
  if (!await file.exists()) {
    debugPrint(
      'Background sync: File not found, removing from queue: ${upload.audioPath}',
    );
    await db.deletePendingUpload(upload.id);
    return;
  }

  // Mark as uploading
  await db.markAsUploading(upload.id);

  try {
    debugPrint('Background sync: Uploading ${upload.audioPath}');

    // Attempt the upload
    await transcriptionService.transcribe(
      upload.audioPath,
      language: upload.language,
      idempotencyKey: upload.idempotencyKey!,
      recordedAt: recordedAtForQueuedUpload(upload),
    );

    debugPrint('Background sync: Upload successful');
    await finalizeSuccessfulUpload(
      db,
      uploadId: upload.id,
      filePath: upload.audioPath,
      deleteLocalFile: deleteLocalFile,
    );
  } catch (e) {
    debugPrint('Background sync: Upload failed: $e');
    final classification = classifyUploadFailure(e);
    await recordUploadFailure(db, upload.id, classification);
  }
}

/// Check if we have network connectivity.
bool _isConnected(List<ConnectivityResult> results) {
  return results.any(
    (r) =>
        r == ConnectivityResult.wifi ||
        r == ConnectivityResult.mobile ||
        r == ConnectivityResult.ethernet,
  );
}

/// Service for managing background sync with workmanager.
class BackgroundSyncService {
  /// Initialize workmanager and register background tasks.
  static Future<void> initialize() async {
    await Workmanager().initialize(callbackDispatcher);

    // Register periodic background sync task
    // Note: Android requires minimum 15 minutes between executions
    await Workmanager().registerPeriodicTask(
      backgroundSyncTaskName,
      backgroundSyncTaskName,
      frequency: periodicSyncInterval,
      constraints: Constraints(
        networkType: NetworkType.connected,
        requiresBatteryNotLow: true,
      ),
      existingWorkPolicy: ExistingPeriodicWorkPolicy.keep,
      backoffPolicy: BackoffPolicy.exponential,
      backoffPolicyDelay: const Duration(minutes: 1),
    );

    debugPrint('Background sync: Initialized with periodic task');
  }

  /// Trigger an immediate one-off sync (e.g., when connectivity is restored).
  static Future<void> triggerImmediateSync() async {
    await Workmanager().registerOneOffTask(
      backgroundSyncOneOffTaskName,
      backgroundSyncOneOffTaskName,
      constraints: Constraints(networkType: NetworkType.connected),
      existingWorkPolicy: ExistingWorkPolicy.replace,
    );

    debugPrint('Background sync: Triggered immediate sync');
  }

  /// Cancel all background sync tasks.
  static Future<void> cancelAll() async {
    await Workmanager().cancelAll();
    debugPrint('Background sync: Cancelled all tasks');
  }

  /// Cancel just the periodic task (keep one-off tasks).
  static Future<void> cancelPeriodic() async {
    await Workmanager().cancelByUniqueName(backgroundSyncTaskName);
    debugPrint('Background sync: Cancelled periodic task');
  }
}
