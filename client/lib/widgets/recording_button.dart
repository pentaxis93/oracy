import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:oracy/services/recording_service.dart';
import 'package:oracy/widgets/permission_dialog.dart';

/// A large button that handles recording start/stop with visual feedback.
class RecordingButton extends ConsumerWidget {
  /// Callback when a recording is completed.
  final void Function(String filePath)? onRecordingComplete;

  const RecordingButton({super.key, this.onRecordingComplete});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final recording = ref.watch(recordingProvider);
    final theme = Theme.of(context);

    return GestureDetector(
      onTap: () => _handleTap(context, ref, recording),
      child: AnimatedContainer(
        duration: const Duration(milliseconds: 200),
        width: recording.isRecording ? 100 : 80,
        height: recording.isRecording ? 100 : 80,
        decoration: BoxDecoration(
          shape: BoxShape.circle,
          color: recording.isRecording
              ? theme.colorScheme.error
              : theme.colorScheme.primary,
          boxShadow: [
            BoxShadow(
              color:
                  (recording.isRecording
                          ? theme.colorScheme.error
                          : theme.colorScheme.primary)
                      .withValues(alpha: 0.4),
              blurRadius: recording.isRecording ? 20 : 10,
              spreadRadius: recording.isRecording ? 5 : 2,
            ),
          ],
        ),
        child: Center(
          child: AnimatedSwitcher(
            duration: const Duration(milliseconds: 200),
            child: recording.isRecording
                ? const Icon(
                    Icons.stop_rounded,
                    key: ValueKey('stop'),
                    color: Colors.white,
                    size: 48,
                  )
                : const Icon(
                    Icons.mic,
                    key: ValueKey('mic'),
                    color: Colors.white,
                    size: 40,
                  ),
          ),
        ),
      ),
    );
  }

  Future<void> _handleTap(
    BuildContext context,
    WidgetRef ref,
    RecordingInfo recording,
  ) async {
    if (kDebugMode && kIsWeb) {
      print(
        '[RECORDING_BUTTON] _handleTap called! isRecording=${recording.isRecording}',
      );
    }
    final notifier = ref.read(recordingProvider.notifier);

    if (recording.isRecording) {
      // Stop recording
      if (kDebugMode && kIsWeb) {
        print('[RECORDING_BUTTON] Stopping recording...');
      }
      final path = await notifier.stopRecording();
      if (path != null && onRecordingComplete != null) {
        onRecordingComplete!(path);
      }
    } else {
      // Check permission and start recording
      if (kDebugMode && kIsWeb) {
        print('[RECORDING_BUTTON] Checking permission...');
      }
      final started = await startRecordingWithPermission(context, ref);
      if (kDebugMode && kIsWeb) {
        print('[RECORDING_BUTTON] Recording started: $started');
      }
    }
  }
}

/// Displays the recording duration in MM:SS format.
class RecordingTimer extends ConsumerWidget {
  const RecordingTimer({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final recording = ref.watch(recordingProvider);
    final theme = Theme.of(context);

    if (!recording.isRecording) {
      return const SizedBox.shrink();
    }

    return AnimatedOpacity(
      opacity: recording.isRecording ? 1.0 : 0.0,
      duration: const Duration(milliseconds: 200),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          // Recording indicator
          Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              Container(
                width: 12,
                height: 12,
                decoration: BoxDecoration(
                  shape: BoxShape.circle,
                  color: theme.colorScheme.error,
                ),
              ),
              const SizedBox(width: 8),
              Text(
                'Recording',
                style: theme.textTheme.bodyLarge?.copyWith(
                  color: theme.colorScheme.error,
                  fontWeight: FontWeight.w600,
                ),
              ),
            ],
          ),
          const SizedBox(height: 16),
          // Duration timer
          Text(
            _formatDuration(recording.duration),
            style: theme.textTheme.displaySmall?.copyWith(
              fontFeatures: const [FontFeature.tabularFigures()],
            ),
          ),
        ],
      ),
    );
  }

  String _formatDuration(Duration duration) {
    final minutes = duration.inMinutes.remainder(60).toString().padLeft(2, '0');
    final seconds = duration.inSeconds.remainder(60).toString().padLeft(2, '0');
    return '$minutes:$seconds';
  }
}

/// Visual indicator of audio amplitude.
class AmplitudeIndicator extends ConsumerWidget {
  const AmplitudeIndicator({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final recording = ref.watch(recordingProvider);
    final theme = Theme.of(context);

    if (!recording.isRecording) {
      return const SizedBox.shrink();
    }

    final amplitude = recording.amplitude ?? 0.0;

    return AnimatedContainer(
      duration: const Duration(milliseconds: 100),
      width: 200,
      height: 4,
      decoration: BoxDecoration(
        borderRadius: BorderRadius.circular(2),
        color: theme.colorScheme.surfaceContainerHighest,
      ),
      child: FractionallySizedBox(
        alignment: Alignment.centerLeft,
        widthFactor: amplitude,
        child: Container(
          decoration: BoxDecoration(
            borderRadius: BorderRadius.circular(2),
            color: theme.colorScheme.primary,
          ),
        ),
      ),
    );
  }
}
