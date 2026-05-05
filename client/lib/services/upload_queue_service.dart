import 'dart:async';
import 'dart:io';
import 'dart:math';

import 'package:connectivity_plus/connectivity_plus.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:oracy/db/database.dart';
import 'package:oracy/services/api_client.dart';
import 'package:oracy/services/background_sync_service.dart';
import 'package:oracy/services/recording_recovery_service.dart';
import 'package:oracy/services/transcription_service.dart';
import 'package:oracy/services/upload_retry_policy.dart';

/// Base delay for exponential backoff (in milliseconds).
const int baseBackoffDelayMs = 1000;

/// Maximum delay between retries (5 minutes).
const int maxBackoffDelayMs = 5 * 60 * 1000;

typedef TerminalFailureDeleteAction =
    Future<void> Function(PendingUpload upload);
typedef ApiKeyAvailabilityCheck = Future<bool> Function();

Future<bool> _apiKeyAlwaysConfigured() async => true;

/// Service for managing the offline upload queue.
///
/// Handles:
/// - Queueing recordings when offline
/// - Processing queue when connectivity is restored
/// - Exponential backoff retry logic
/// - Monitoring connectivity changes
class UploadQueueService {
  final AppDatabase _db;
  final TranscriptionService _transcriptionService;
  final Connectivity _connectivity;
  final LocalFileDeleter _deleteLocalFile;
  final ApiKeyAvailabilityCheck _hasApiKey;

  StreamSubscription<List<ConnectivityResult>>? _connectivitySubscription;
  Timer? _processingTimer;
  bool _isProcessing = false;

  UploadQueueService({
    required AppDatabase db,
    required TranscriptionService transcriptionService,
    Connectivity? connectivity,
    LocalFileDeleter deleteLocalFile = defaultLocalFileDeleter,
    ApiKeyAvailabilityCheck hasApiKey = _apiKeyAlwaysConfigured,
  }) : _db = db,
       _transcriptionService = transcriptionService,
       _connectivity = connectivity ?? Connectivity(),
       _deleteLocalFile = deleteLocalFile,
       _hasApiKey = hasApiKey;

  /// Start monitoring connectivity and processing queue.
  Future<void> start() async {
    _connectivitySubscription = _connectivity.onConnectivityChanged.listen(
      _onConnectivityChanged,
    );

    await _restoreStaleUploadsAndProcess();
  }

  /// Stop monitoring and cleanup.
  void stop() {
    _connectivitySubscription?.cancel();
    _connectivitySubscription = null;
    _processingTimer?.cancel();
    _processingTimer = null;
  }

  /// Queue a recording for upload.
  ///
  /// Returns the queue entry ID.
  Future<int> queueUpload(String audioPath, {String? language}) async {
    return await _db.ensurePendingUpload(
      audioPath: audioPath,
      language: language,
    );
  }

  /// Process the upload queue, attempting to upload pending items.
  Future<void> processQueue() async {
    if (_isProcessing) return;
    _isProcessing = true;

    try {
      await retryPendingUploadCleanup(_db, deleteLocalFile: _deleteLocalFile);

      if (!await _hasApiKey()) {
        return;
      }

      final pendingUploads = await _db.getPendingUploads(
        maxRetries: maxRetryAttempts,
      );

      for (final upload in pendingUploads) {
        // Check if we're still online
        final connectivityResult = await _connectivity.checkConnectivity();
        if (!_isConnected(connectivityResult)) {
          break; // Stop processing if offline
        }

        await _processUpload(upload);
      }
    } finally {
      _isProcessing = false;
    }
  }

  /// Process a single upload.
  Future<void> _processUpload(PendingUpload upload) async {
    // Check if file still exists
    final file = File(upload.audioPath);
    if (!await file.exists()) {
      // File is gone, remove from queue
      await _db.deletePendingUpload(upload.id);
      return;
    }

    // Mark as uploading
    await _db.markAsUploading(upload.id);

    try {
      // Attempt the upload
      await _transcriptionService.transcribe(
        upload.audioPath,
        language: upload.language,
        idempotencyKey: upload.idempotencyKey,
        recordedAt: recordedAtForQueuedUpload(upload),
      );

      await finalizeSuccessfulUpload(
        _db,
        uploadId: upload.id,
        filePath: upload.audioPath,
        deleteLocalFile: _deleteLocalFile,
      );
    } catch (e) {
      final classification = classifyUploadFailure(e);
      await recordUploadFailure(_db, upload.id, classification);

      if (classification.isRetryable) {
        // Calculate backoff delay
        final backoffDelay = _calculateBackoff(upload.retryCount);

        // Schedule retry after backoff delay
        _scheduleRetry(backoffDelay);
      }
    }
  }

  /// Calculate exponential backoff delay with jitter.
  int _calculateBackoff(int retryCount) {
    // Exponential backoff: baseDelay * 2^retryCount
    final exponentialDelay = baseBackoffDelayMs * pow(2, retryCount).toInt();

    // Cap at max delay
    final cappedDelay = min(exponentialDelay, maxBackoffDelayMs);

    // Add jitter (0-25% of the delay)
    final jitter = (cappedDelay * 0.25 * Random().nextDouble()).toInt();

    return cappedDelay + jitter;
  }

  /// Schedule a retry after the given delay.
  void _scheduleRetry(int delayMs) {
    _processingTimer?.cancel();
    _processingTimer = Timer(Duration(milliseconds: delayMs), processQueue);
  }

  /// Handle connectivity changes.
  void _onConnectivityChanged(List<ConnectivityResult> results) {
    if (_isConnected(results)) {
      // We're back online, process the queue in foreground
      processQueue();

      // Also trigger background sync for when app is closed
      BackgroundSyncService.triggerImmediateSync();
    }
  }

  /// Check connectivity and process if online.
  Future<void> _checkConnectivityAndProcess() async {
    final result = await _connectivity.checkConnectivity();
    if (_isConnected(result)) {
      await processQueue();
    }
  }

  Future<bool> deleteTerminalFailureRecording(int uploadId) async {
    final upload = await _db.getUploadById(uploadId);
    if (upload == null || upload.status != UploadStatus.terminalFailure.index) {
      return false;
    }

    await _deleteLocalFile(upload.audioPath);
    await _db.deletePendingUpload(uploadId);
    return true;
  }

  Future<void> _restoreStaleUploadsAndProcess() async {
    await _db.restoreStaleUploadingUploads(staleUploadingRestoreThreshold);
    await retryPendingUploadCleanup(_db, deleteLocalFile: _deleteLocalFile);
    await RecordingRecoveryService.reconcilePersistentRecordings(_db);
    await _checkConnectivityAndProcess();
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

  /// Get the current pending upload count.
  Future<int> getPendingCount() => _db.getPendingCount();

  /// Watch the pending upload count.
  Stream<int> watchPendingCount() => _db.watchPendingCount();

  /// Cleanup old completed entries.
  Future<void> cleanup() => _db.cleanupOldUploads();
}

/// Provider for the upload queue service.
final uploadQueueServiceProvider = Provider<UploadQueueService?>((ref) {
  // Upload queue not supported on web (no offline storage)
  if (kIsWeb) {
    return null;
  }

  final db = ref.watch(appDatabaseProvider);
  final storage = ref.watch(secureStorageProvider);
  final transcriptionService = ref.watch(transcriptionServiceProvider);

  final service = UploadQueueService(
    db: db,
    transcriptionService: transcriptionService,
    hasApiKey: storage.hasApiKey,
  );

  // Start the service
  unawaited(service.start());

  // Cleanup when disposed
  ref.onDispose(() => service.stop());

  return service;
});

/// Provider for watching pending upload count.
final pendingUploadCountProvider = StreamProvider<int>((ref) {
  // On web, upload queue is not supported (no offline storage)
  if (kIsWeb) {
    return Stream.value(0);
  }

  final service = ref.watch(uploadQueueServiceProvider);
  if (service == null) {
    return Stream.value(0);
  }
  return service.watchPendingCount();
});

final terminalFailureUploadsProvider = StreamProvider<List<PendingUpload>>((
  ref,
) {
  if (kIsWeb) {
    return Stream.value(const <PendingUpload>[]);
  }

  final db = ref.watch(appDatabaseProvider);
  return db.watchTerminalFailureUploads();
});

final terminalFailureDeleteActionProvider =
    Provider<TerminalFailureDeleteAction>((ref) {
      return (PendingUpload upload) async {
        final service = ref.read(uploadQueueServiceProvider);
        if (service == null) {
          throw StateError("Upload queue is unavailable on this platform.");
        }

        await service.deleteTerminalFailureRecording(upload.id);
      };
    });

/// State for the upload queue status.
class UploadQueueState {
  final int pendingCount;
  final bool isProcessing;
  final String? lastError;

  const UploadQueueState({
    this.pendingCount = 0,
    this.isProcessing = false,
    this.lastError,
  });

  UploadQueueState copyWith({
    int? pendingCount,
    bool? isProcessing,
    String? lastError,
  }) {
    return UploadQueueState(
      pendingCount: pendingCount ?? this.pendingCount,
      isProcessing: isProcessing ?? this.isProcessing,
      lastError: lastError,
    );
  }
}
