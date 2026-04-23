import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:oracy/db/database.dart';
import 'package:path/path.dart' as p;
import 'package:path_provider/path_provider.dart';
import 'package:shared_preferences/shared_preferences.dart';

/// Service for recovering recordings that were interrupted by app crashes
/// or unexpected termination.
///
/// This service tracks active recording sessions and can detect/recover
/// orphaned recording files that weren't properly finalized.
class RecordingRecoveryService {
  static const _activeRecordingPathKey = 'active_recording_path';
  static const _activeRecordingStartKey = 'active_recording_start';
  static const _activeRecordingCancelRequestedKey =
      'active_recording_cancel_requested';
  static const _recordingFilePrefix = 'oracy_recording_';
  static const _supportedRecordingExtensions = {
    '.wav',
    '.mp3',
    '.mp4',
    '.m4a',
    '.webm',
    '.opus',
  };

  /// Mark that a recording has started.
  ///
  /// Call this when recording begins. If the app crashes, we can use this
  /// to detect and potentially recover the orphaned recording file.
  static Future<void> markRecordingStarted(String filePath) async {
    if (kIsWeb) return;

    final prefs = await SharedPreferences.getInstance();
    await prefs.remove(_activeRecordingCancelRequestedKey);
    await prefs.setString(_activeRecordingPathKey, filePath);
    await prefs.setInt(
      _activeRecordingStartKey,
      DateTime.now().millisecondsSinceEpoch,
    );

    if (kDebugMode) {
      debugPrint('[RECOVERY] Marked recording started: $filePath');
    }
  }

  /// Mark that a recording has completed successfully.
  ///
  /// Call this when recording finishes normally (either stopped or cancelled).
  static Future<void> markRecordingCompleted() async {
    if (kIsWeb) return;

    final prefs = await SharedPreferences.getInstance();
    await prefs.remove(_activeRecordingPathKey);
    await prefs.remove(_activeRecordingStartKey);
    await prefs.remove(_activeRecordingCancelRequestedKey);

    if (kDebugMode) {
      debugPrint('[RECOVERY] Marked recording completed');
    }
  }

  /// Mark that the active recording should be discarded if recovery runs.
  static Future<void> markRecordingCancelRequested() async {
    if (kIsWeb) return;

    final prefs = await SharedPreferences.getInstance();
    await prefs.setBool(_activeRecordingCancelRequestedKey, true);

    if (kDebugMode) {
      debugPrint('[RECOVERY] Marked recording cancel requested');
    }
  }

  /// Clear the cancel-intent marker once discard has completed.
  static Future<void> clearRecordingCancelRequested() async {
    if (kIsWeb) return;

    final prefs = await SharedPreferences.getInstance();
    await prefs.remove(_activeRecordingCancelRequestedKey);

    if (kDebugMode) {
      debugPrint('[RECOVERY] Cleared recording cancel requested');
    }
  }

  /// Check for and recover any interrupted recordings.
  ///
  /// Call this on app startup. Returns the path to a recovered recording
  /// file if one was found and successfully recovered, null otherwise.
  static Future<String?> checkForInterruptedRecording() async {
    if (kIsWeb) return null;

    try {
      final prefs = await SharedPreferences.getInstance();
      final activePath = prefs.getString(_activeRecordingPathKey);
      final cancelRequested =
          prefs.getBool(_activeRecordingCancelRequestedKey) ?? false;

      if (activePath == null) {
        if (cancelRequested) {
          await clearRecordingCancelRequested();
        }
        return null;
      }

      if (kDebugMode) {
        debugPrint('[RECOVERY] Found interrupted recording: $activePath');
      }

      // Check if the file exists
      final file = File(activePath);
      if (cancelRequested) {
        if (!await file.exists()) {
          await markRecordingCompleted();
          return null;
        }

        try {
          await file.delete();
          await markRecordingCompleted();
        } catch (e) {
          if (kDebugMode) {
            debugPrint(
              '[RECOVERY] Error deleting canceled recording during recovery: $e',
            );
          }
        }
        return null;
      }

      if (!await file.exists()) {
        if (kDebugMode) {
          debugPrint('[RECOVERY] Interrupted recording file not found');
        }
        await markRecordingCompleted();
        return null;
      }

      // Check file size - if it's too small, it's probably not recoverable
      final fileSize = await file.length();
      if (fileSize < 1024) {
        // Less than 1KB - not worth recovering
        if (kDebugMode) {
          debugPrint('[RECOVERY] File too small to recover: $fileSize bytes');
        }
        await file.delete();
        await markRecordingCompleted();
        return null;
      }

      // Try to fix up the WAV header if needed
      final fixedPath = await _repairWavFile(activePath);

      // Clear the active recording marker
      await markRecordingCompleted();

      return fixedPath;
    } catch (e) {
      if (kDebugMode) {
        debugPrint('[RECOVERY] Error checking for interrupted recording: $e');
      }
      // Clear the marker to prevent infinite loops
      await markRecordingCompleted();
      return null;
    }
  }

  /// Clean up any orphaned recording files in the temp directory.
  ///
  /// Call this periodically to prevent accumulation of old temp files.
  static Future<void> cleanupOrphanedRecordings({
    Duration maxAge = const Duration(days: 1),
  }) async {
    if (kIsWeb) return;

    try {
      final tempDir = await getTemporaryDirectory();
      final now = DateTime.now();

      await for (final entity in tempDir.list()) {
        if (entity is File && entity.path.contains('oracy_recording_')) {
          final stat = await entity.stat();
          final age = now.difference(stat.modified);

          if (age > maxAge) {
            if (kDebugMode) {
              debugPrint(
                '[RECOVERY] Cleaning up old recording: ${entity.path}',
              );
            }
            await entity.delete();
          }
        }
      }
    } catch (e) {
      if (kDebugMode) {
        debugPrint('[RECOVERY] Error cleaning up orphaned recordings: $e');
      }
    }
  }

  /// Get the persistent directory used to store completed recordings.
  static Future<Directory> getPersistentRecordingsDirectory() async {
    final directory = await getApplicationSupportDirectory();
    return Directory(p.join(directory.path, 'recordings'));
  }

  /// Queue completed recordings that exist on disk but have no pending upload.
  static Future<int> reconcilePersistentRecordings(
    AppDatabase db, {
    Directory? recordingsDirectory,
    SharedPreferences? sharedPreferences,
  }) async {
    if (kIsWeb) return 0;

    try {
      final directory =
          recordingsDirectory ?? await getPersistentRecordingsDirectory();
      String? activeRecordingPath;
      try {
        final prefs = sharedPreferences ?? await SharedPreferences.getInstance();
        activeRecordingPath = prefs.getString(_activeRecordingPathKey);
      } catch (e) {
        if (kDebugMode) {
          debugPrint(
            '[RECOVERY] Could not read active recording marker during reconciliation: $e',
          );
        }
      }
      if (!await directory.exists()) {
        return 0;
      }

      var queuedCount = 0;
      await for (final entity in directory.list()) {
        if (entity is! File || !_isManagedRecordingFile(entity.path)) {
          continue;
        }
        if (entity.path == activeRecordingPath) {
          continue;
        }

        final existingUpload = await db.getUploadByAudioPath(entity.path);
        if (existingUpload != null) {
          continue;
        }

        await db.ensurePendingUpload(audioPath: entity.path);
        queuedCount++;
      }

      return queuedCount;
    } catch (e) {
      if (kDebugMode) {
        debugPrint('[RECOVERY] Error reconciling persistent recordings: $e');
      }
      return 0;
    }
  }

  /// Attempt to repair a WAV file that may have an incomplete header.
  ///
  /// WAV files have a header that specifies the total file size. If the
  /// recording was interrupted, this header may be wrong. This method
  /// reads the file, calculates the correct size, and rewrites the header.
  static Future<String?> _repairWavFile(String filePath) async {
    try {
      final file = File(filePath);
      final bytes = await file.readAsBytes();

      if (bytes.length < 44) {
        // Too small to be a valid WAV file
        return null;
      }

      // Check if this is a WAV file (starts with "RIFF" and contains "WAVE")
      final riff = String.fromCharCodes(bytes.sublist(0, 4));
      final wave = String.fromCharCodes(bytes.sublist(8, 12));

      if (riff != 'RIFF' || wave != 'WAVE') {
        if (kDebugMode) {
          debugPrint('[RECOVERY] Not a WAV file, skipping repair');
        }
        return filePath; // Return as-is, might still be usable
      }

      // Fix the file size fields in the header
      final fileSize = bytes.length;
      final dataSize = fileSize - 44; // WAV header is 44 bytes

      // Create a mutable copy
      final fixedBytes = Uint8List.fromList(bytes);

      // Update RIFF chunk size (bytes 4-7): file size - 8
      _writeInt32LE(fixedBytes, 4, fileSize - 8);

      // Update data chunk size (bytes 40-43): data size
      _writeInt32LE(fixedBytes, 40, dataSize);

      // Write the fixed file
      final fixedPath = filePath.replaceAll('.wav', '_recovered.wav');
      final fixedFile = File(fixedPath);
      await fixedFile.writeAsBytes(fixedBytes);

      try {
        await file.delete();
      } catch (e) {
        if (await fixedFile.exists()) {
          await fixedFile.delete();
        }

        if (kDebugMode) {
          debugPrint(
            '[RECOVERY] Could not remove original WAV after repair: $e',
          );
        }

        return filePath;
      }

      if (kDebugMode) {
        debugPrint('[RECOVERY] Repaired WAV file: $fixedPath');
      }

      return fixedPath;
    } catch (e) {
      if (kDebugMode) {
        debugPrint('[RECOVERY] Error repairing WAV file: $e');
      }
      return filePath; // Return original path as fallback
    }
  }

  /// Write a 32-bit little-endian integer to a byte array.
  static void _writeInt32LE(Uint8List bytes, int offset, int value) {
    bytes[offset] = value & 0xFF;
    bytes[offset + 1] = (value >> 8) & 0xFF;
    bytes[offset + 2] = (value >> 16) & 0xFF;
    bytes[offset + 3] = (value >> 24) & 0xFF;
  }

  static bool _isManagedRecordingFile(String filePath) {
    final filename = p.basename(filePath);
    final extension = p.extension(filename).toLowerCase();
    return filename.startsWith(_recordingFilePrefix) &&
        _supportedRecordingExtensions.contains(extension);
  }
}
