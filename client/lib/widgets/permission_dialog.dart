import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:oracy/services/permission_service.dart';
import 'package:oracy/services/recording_service.dart';

/// A dialog that handles microphone permission requests.
///
/// Shows appropriate messages based on permission status and provides
/// buttons to request permission or open settings.
class MicrophonePermissionDialog extends ConsumerStatefulWidget {
  const MicrophonePermissionDialog({super.key});

  /// Show the permission dialog and return true if permission was granted.
  static Future<bool> show(BuildContext context) async {
    final result = await showDialog<bool>(
      context: context,
      barrierDismissible: false,
      builder: (context) => const MicrophonePermissionDialog(),
    );
    return result ?? false;
  }

  @override
  ConsumerState<MicrophonePermissionDialog> createState() =>
      _MicrophonePermissionDialogState();
}

class _MicrophonePermissionDialogState
    extends ConsumerState<MicrophonePermissionDialog>
    with WidgetsBindingObserver {
  bool _refreshPermissionOnResume = false;

  @override
  void dispose() {
    if (_refreshPermissionOnResume) {
      WidgetsBinding.instance.removeObserver(this);
    }
    super.dispose();
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    if (!_refreshPermissionOnResume || state != AppLifecycleState.resumed) {
      return;
    }

    _refreshPermissionOnResume = false;
    WidgetsBinding.instance.removeObserver(this);
    ref.invalidate(microphonePermissionProvider);
  }

  @override
  Widget build(BuildContext context) {
    final permissionStatus = ref.watch(microphonePermissionProvider);

    return permissionStatus.when(
      loading: () => const AlertDialog(
        content: SizedBox(
          height: 100,
          child: Center(child: CircularProgressIndicator()),
        ),
      ),
      error: (error, _) => AlertDialog(
        title: const Text('Error'),
        content: Text('Failed to check permission: $error'),
        actions: [
          TextButton(
            onPressed: () => Navigator.of(context).pop(false),
            child: const Text('Close'),
          ),
        ],
      ),
      data: (status) => _buildDialogForStatus(context, status),
    );
  }

  Widget _buildDialogForStatus(
    BuildContext context,
    MicrophonePermissionStatus status,
  ) {
    switch (status) {
      case MicrophonePermissionStatus.granted:
        // Permission already granted, close and return success
        WidgetsBinding.instance.addPostFrameCallback((_) {
          Navigator.of(context).pop(true);
        });
        return const AlertDialog(
          content: SizedBox(
            height: 100,
            child: Center(child: CircularProgressIndicator()),
          ),
        );

      case MicrophonePermissionStatus.denied:
        return AlertDialog(
          icon: const Icon(Icons.mic_off, size: 48),
          title: const Text('Microphone Access Required'),
          content: const Text(
            'Oracy needs microphone access to record your voice for transcription. '
            'Please grant permission to continue.',
          ),
          actions: [
            TextButton(
              onPressed: () => Navigator.of(context).pop(false),
              child: const Text('Not Now'),
            ),
            FilledButton(
              onPressed: _requestPermission,
              child: const Text('Allow Microphone'),
            ),
          ],
        );

      case MicrophonePermissionStatus.permanentlyDenied:
        return AlertDialog(
          icon: const Icon(Icons.settings, size: 48),
          title: const Text('Permission Required'),
          content: const Text(
            'Microphone permission has been permanently denied. '
            'Please open Settings and enable microphone access for Oracy.',
          ),
          actions: [
            TextButton(
              onPressed: () => Navigator.of(context).pop(false),
              child: const Text('Cancel'),
            ),
            FilledButton(
              onPressed: _openSettings,
              child: const Text('Open Settings'),
            ),
          ],
        );

      case MicrophonePermissionStatus.restricted:
        return AlertDialog(
          icon: const Icon(Icons.block, size: 48),
          title: const Text('Access Restricted'),
          content: const Text(
            'Microphone access is restricted on this device. '
            'This may be due to parental controls or device management policies.',
          ),
          actions: [
            TextButton(
              onPressed: () => Navigator.of(context).pop(false),
              child: const Text('OK'),
            ),
          ],
        );

      case MicrophonePermissionStatus.unknown:
        return AlertDialog(
          icon: const Icon(Icons.mic, size: 48),
          title: const Text('Microphone Access'),
          content: const Text(
            'Oracy needs microphone access to record your voice for transcription.',
          ),
          actions: [
            TextButton(
              onPressed: () => Navigator.of(context).pop(false),
              child: const Text('Not Now'),
            ),
            FilledButton(
              onPressed: _requestPermission,
              child: const Text('Continue'),
            ),
          ],
        );
    }
  }

  Future<void> _requestPermission() async {
    final service = ref.read(permissionServiceProvider);
    final status = await service.requestMicrophonePermission();

    if (!mounted) return;

    if (status == MicrophonePermissionStatus.granted) {
      Navigator.of(context).pop(true);
    } else {
      // Refresh the permission status to show updated dialog
      ref.invalidate(microphonePermissionProvider);
    }
  }

  Future<void> _openSettings() async {
    final service = ref.read(permissionServiceProvider);
    final opened = await service.openSettings();

    if (!mounted || !opened) return;

    if (!_refreshPermissionOnResume) {
      _refreshPermissionOnResume = true;
      WidgetsBinding.instance.addObserver(this);
    }
  }
}

/// A helper function to check and request microphone permission.
///
/// Returns true if permission is granted, false otherwise.
/// Shows a dialog if permission is not already granted.
Future<bool> ensureMicrophonePermission(
  BuildContext context,
  WidgetRef ref,
) async {
  final service = ref.read(permissionServiceProvider);
  final currentStatus = await service.checkMicrophonePermission();

  if (currentStatus == MicrophonePermissionStatus.granted) {
    return true;
  }

  if (!context.mounted) return false;

  return MicrophonePermissionDialog.show(context);
}

/// Start recording through the same permission gate used by every UI entrypoint.
Future<bool> startRecordingWithPermission(
  BuildContext context,
  WidgetRef ref,
) async {
  final hasPermission = await ensureMicrophonePermission(context, ref);
  if (!hasPermission || !context.mounted) {
    return false;
  }

  final recordingState = ref.read(recordingProvider);
  if (recordingState.isRecording) {
    return false;
  }

  await ref.read(recordingProvider.notifier).startRecording();
  return true;
}
