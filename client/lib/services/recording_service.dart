import 'dart:async';
import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:oracy/services/recording_recovery_service.dart';
import 'package:path/path.dart' as p;
import 'package:record/record.dart';

/// State of an audio recording session.
enum RecordingState {
  /// Not recording, ready to start.
  idle,

  /// Currently recording audio.
  recording,

  /// Paused (not currently used, but available).
  paused,

  /// Recording completed, file available.
  completed,

  /// An error occurred during recording.
  error,
}

/// Information about a recording in progress or completed.
class RecordingInfo {
  final RecordingState state;
  final String? filePath;
  final DateTime? startedAt;
  final Duration duration;
  final String? errorMessage;
  final double? amplitude;

  const RecordingInfo({
    required this.state,
    this.filePath,
    this.startedAt,
    this.duration = Duration.zero,
    this.errorMessage,
    this.amplitude,
  });

  RecordingInfo copyWith({
    RecordingState? state,
    String? filePath,
    DateTime? startedAt,
    Duration? duration,
    String? errorMessage,
    double? amplitude,
  }) {
    return RecordingInfo(
      state: state ?? this.state,
      filePath: filePath ?? this.filePath,
      startedAt: startedAt ?? this.startedAt,
      duration: duration ?? this.duration,
      errorMessage: errorMessage ?? this.errorMessage,
      amplitude: amplitude ?? this.amplitude,
    );
  }

  bool get isRecording => state == RecordingState.recording;
  bool get hasFile => filePath != null && filePath!.isNotEmpty;
}

class RecordingStart {
  final String filePath;
  final DateTime recordedAt;

  const RecordingStart({required this.filePath, required this.recordedAt});
}

class RecordingCompletion {
  final String filePath;
  final DateTime recordedAt;

  const RecordingCompletion({required this.filePath, required this.recordedAt});
}

/// Service for managing audio recording.
class RecordingService {
  final AudioRecorder _recorder = AudioRecorder();
  Timer? _durationTimer;
  DateTime? _startTime;

  /// Check if the device has a recording capability.
  Future<bool> hasPermission() async {
    return await _recorder.hasPermission();
  }

  /// Check if currently recording.
  Future<bool> isRecording() async {
    return await _recorder.isRecording();
  }

  /// Start recording audio.
  ///
  /// Returns the path to the recording file (or empty string on web).
  Future<RecordingStart> startRecording() async {
    final recordedAt = DateTime.now().toUtc();
    String path = '';

    // On web, we don't use file paths - record package handles it via Blob
    if (!kIsWeb) {
      final recordingsDirectory =
          await RecordingRecoveryService.getPersistentRecordingsDirectory();
      await recordingsDirectory.create(recursive: true);
      final timestamp = recordedAt.millisecondsSinceEpoch;
      path = p.join(recordingsDirectory.path, 'oracy_recording_$timestamp.wav');
    }

    // On web, path parameter is ignored by record_web package
    // It records to a Blob and returns a blob URL from stop()
    // Using WAV format for maximum Whisper API compatibility and reliability
    // (AAC/m4a was causing truncation issues with long recordings)
    await _recorder.start(
      RecordConfig(
        encoder: AudioEncoder
            .wav, // WAV for all platforms - most reliable with Whisper
        bitRate: 128000,
        sampleRate: 16000, // 16kHz is optimal for speech recognition
        numChannels: 1, // Mono for speech
        // CRITICAL: Prevent recording from pausing on audio interruptions
        // (notifications, other apps, etc.) - we want uninterrupted recording
        audioInterruption: AudioInterruptionMode.none,
        // Use record package's built-in Android foreground service for
        // reliable background recording - this keeps recording alive when
        // the app is backgrounded or screen is off
        androidConfig: AndroidRecordConfig(
          service: AndroidService(
            title: 'Oracy Recording',
            content: 'Recording in progress...',
          ),
        ),
      ),
      path: path,
    );

    _startTime = recordedAt;
    return RecordingStart(filePath: path, recordedAt: recordedAt);
  }

  /// Stop recording and return the file path.
  Future<RecordingCompletion?> stopRecording() async {
    final recordedAt = _startTime?.toUtc();
    final path = await _recorder.stop();
    _startTime = null;
    if (path == null || recordedAt == null) {
      return null;
    }
    return RecordingCompletion(filePath: path, recordedAt: recordedAt);
  }

  /// Cancel the current recording and delete the file.
  Future<void> cancelRecording() async {
    await RecordingRecoveryService.markRecordingCancelRequested();
    final path = await _recorder.stop();
    _startTime = null;

    if (path != null) {
      final file = File(path);
      if (await file.exists()) {
        await file.delete();
        await RecordingRecoveryService.clearRecordingCancelRequested();
      }
    }
  }

  /// Get the current recording duration.
  Duration get currentDuration {
    if (_startTime == null) return Duration.zero;
    return DateTime.now().difference(_startTime!);
  }

  /// Get the current amplitude level (0.0 to 1.0).
  Future<double?> getAmplitude() async {
    final amp = await _recorder.getAmplitude();
    // Convert dB to 0-1 scale
    // Typical range is -160 (silence) to 0 (max)
    final db = amp.current;
    if (db == double.negativeInfinity) return 0.0;
    // Normalize: -60dB (quiet) to 0dB (loud) -> 0-1
    return ((db + 60) / 60).clamp(0.0, 1.0);
  }

  /// Dispose of the recorder resources.
  void dispose() {
    _durationTimer?.cancel();
    _recorder.dispose();
  }
}

/// Provider for the recording service.
final recordingServiceProvider = Provider<RecordingService>((ref) {
  final service = RecordingService();
  ref.onDispose(() => service.dispose());
  return service;
});

/// Notifier that manages recording state.
class RecordingNotifier extends Notifier<RecordingInfo> {
  Timer? _updateTimer;

  RecordingService get _service => ref.read(recordingServiceProvider);

  @override
  RecordingInfo build() {
    ref.onDispose(() {
      _updateTimer?.cancel();
    });
    return const RecordingInfo(state: RecordingState.idle);
  }

  /// Start recording.
  Future<void> startRecording() async {
    if (kDebugMode && kIsWeb) {
      debugPrint('[RECORDING_SERVICE] startRecording called');
    }
    if (state.isRecording) return;

    try {
      if (kDebugMode && kIsWeb) {
        debugPrint('[RECORDING_SERVICE] Calling _service.startRecording()...');
      }
      final recordingStart = await _service.startRecording();
      if (kDebugMode && kIsWeb) {
        debugPrint('[RECORDING_SERVICE] Got path: ${recordingStart.filePath}');
      }
      state = RecordingInfo(
        state: RecordingState.recording,
        filePath: recordingStart.filePath,
        startedAt: recordingStart.recordedAt,
        duration: Duration.zero,
      );
      if (kDebugMode && kIsWeb) {
        debugPrint('[RECORDING_SERVICE] State updated to recording');
      }

      // Mark recording as active for crash recovery
      if (recordingStart.filePath.isNotEmpty) {
        await RecordingRecoveryService.markRecordingStarted(
          recordingStart.filePath,
        );
      }

      // Start periodic updates for duration and amplitude
      _updateTimer = Timer.periodic(
        const Duration(milliseconds: 100),
        (_) => _updateState(),
      );
    } catch (e) {
      if (kDebugMode && kIsWeb) {
        debugPrint('[RECORDING_SERVICE] ERROR: $e');
      }
      state = RecordingInfo(
        state: RecordingState.error,
        errorMessage: e.toString(),
      );
    }
  }

  /// Stop recording and return the file path.
  Future<RecordingCompletion?> stopRecording() async {
    if (!state.isRecording) return null;

    _updateTimer?.cancel();
    _updateTimer = null;
    final duration = _service.currentDuration;

    try {
      final completion = await _service.stopRecording();

      // Mark recording as completed for crash recovery
      await RecordingRecoveryService.markRecordingCompleted();

      state = RecordingInfo(
        state: RecordingState.completed,
        filePath: completion?.filePath,
        startedAt: completion?.recordedAt,
        duration: duration,
      );
      return completion;
    } catch (e) {
      // Mark recording as completed (even on error, it's not active anymore)
      await RecordingRecoveryService.markRecordingCompleted();

      state = RecordingInfo(
        state: RecordingState.error,
        errorMessage: e.toString(),
      );
      return null;
    }
  }

  /// Cancel the current recording.
  Future<void> cancelRecording() async {
    if (!state.isRecording) return;

    _updateTimer?.cancel();
    _updateTimer = null;

    await _service.cancelRecording();

    // Mark recording as completed (cancelled is also completed)
    await RecordingRecoveryService.markRecordingCompleted();

    state = const RecordingInfo(state: RecordingState.idle);
  }

  /// Reset to idle state (e.g., after handling a completed recording).
  void reset() {
    state = const RecordingInfo(state: RecordingState.idle);
  }

  void _updateState() async {
    if (!state.isRecording) return;

    final amplitude = await _service.getAmplitude();
    state = state.copyWith(
      duration: _service.currentDuration,
      amplitude: amplitude,
    );
  }
}

/// Provider for recording state and controls.
final recordingProvider = NotifierProvider<RecordingNotifier, RecordingInfo>(
  RecordingNotifier.new,
);
