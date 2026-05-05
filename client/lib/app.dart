import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:oracy/screens/history_screen.dart';
import 'package:oracy/screens/settings_screen.dart';
import 'package:oracy/screens/transcript_result_screen.dart';
import 'package:oracy/services/home_widget_service.dart';
import 'package:oracy/services/preferences_service.dart';
import 'package:oracy/services/recording_service.dart';
import 'package:oracy/services/transcription_service.dart';
import 'package:oracy/widgets/permission_dialog.dart';
import 'package:oracy/widgets/recording_button.dart';
import 'package:oracy/widgets/sync_status_indicator.dart';

/// The root application widget for Oracy.
///
/// This widget configures the MaterialApp with theming and sets up
/// the home screen for audio transcription.
class OracyApp extends ConsumerWidget {
  const OracyApp({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    if (kDebugMode && kIsWeb) {
      debugPrint('[APP] Building MaterialApp...');
    }
    return MaterialApp(
      title: 'Oracy',
      debugShowCheckedModeBanner: false,
      theme: ThemeData(
        useMaterial3: true,
        colorScheme: ColorScheme.fromSeed(
          seedColor: Colors.deepPurple,
          brightness: Brightness.light,
        ),
      ),
      darkTheme: ThemeData(
        useMaterial3: true,
        colorScheme: ColorScheme.fromSeed(
          seedColor: Colors.deepPurple,
          brightness: Brightness.dark,
        ),
      ),
      themeMode: ThemeMode.system,
      home: const HomePage(),
    );
  }
}

/// The main home page with recording functionality.
class HomePage extends ConsumerStatefulWidget {
  const HomePage({super.key});

  @override
  ConsumerState<HomePage> createState() => _HomePageState();
}

class _HomePageState extends ConsumerState<HomePage>
    with WidgetsBindingObserver {
  @override
  void initState() {
    super.initState();
    // Register lifecycle observer to handle app backgrounding during recording
    WidgetsBinding.instance.addObserver(this);

    if (kDebugMode && kIsWeb) {
      debugPrint('[HOME] initState called');
    }
    // Listen for transcription state changes
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (kDebugMode && kIsWeb) {
        debugPrint('[HOME] PostFrameCallback executing...');
      }
      HomeWidgetService.setOnRecordCallback(_startRecordingFromWidget);
      _setupTranscriptionListener();
    });
  }

  @override
  void dispose() {
    WidgetsBinding.instance.removeObserver(this);
    HomeWidgetService.setOnRecordCallback(null);
    super.dispose();
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    final recording = ref.read(recordingProvider);

    if (kDebugMode) {
      debugPrint(
        '[HOME] App lifecycle changed to: $state, isRecording: ${recording.isRecording}',
      );
    }

    // The foreground service handles keeping the app alive during background recording.
    // This observer is here for logging and potential future enhancements like:
    // - Auto-save checkpoints when backgrounded
    // - Warning the user when they background while recording
    // - Handling edge cases the foreground service might miss
  }

  void _setupTranscriptionListener() {
    ref.listenManual(transcriptionProvider, (previous, next) {
      if (next is TranscriptionSuccess) {
        // Auto-copy to clipboard if enabled
        final autoCopyEnabled = ref.read(autoCopyEnabledProvider);
        if (autoCopyEnabled) {
          Clipboard.setData(ClipboardData(text: next.voiceNote.text));
          // Haptic feedback to indicate copy
          HapticFeedback.mediumImpact();
        }

        // Navigate to result screen when transcription completes
        Navigator.push(
          context,
          MaterialPageRoute(
            builder: (_) => TranscriptResultScreen(
              voiceNote: next.voiceNote,
              wasAutoCopied: autoCopyEnabled,
            ),
          ),
        );
        // Update widget status
        HomeWidgetService.updateStatus('Tap to record');
      } else if (next is TranscriptionError &&
          next.errorType == TranscriptionErrorType.auth) {
        // Show dialog prompting to configure API key
        _showAuthErrorDialog(next.message);
      }
    });

    // Listen for recording state changes to update widget
    ref.listenManual(recordingProvider, (previous, next) {
      switch (next.state) {
        case RecordingState.recording:
          HomeWidgetService.updateStatus('Recording...');
        case RecordingState.paused:
          HomeWidgetService.updateStatus('Paused');
        case RecordingState.idle:
        case RecordingState.completed:
        case RecordingState.error:
          HomeWidgetService.updateStatus('Tap to record');
      }
    });
  }

  void _startRecordingFromWidget() {
    unawaited(_startRecordingFromWidgetAsync());
  }

  Future<void> _startRecordingFromWidgetAsync() async {
    final navigator = Navigator.of(context);
    final started = await startRecordingWithPermission(context, ref);
    if (!started || !context.mounted) {
      return;
    }

    navigator.popUntil((route) => route.isFirst);
  }

  void _showAuthErrorDialog(String message) {
    showDialog(
      context: context,
      builder: (context) => AlertDialog(
        title: const Text('API Key Required'),
        content: Text(message),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(context),
            child: const Text('Cancel'),
          ),
          FilledButton(
            onPressed: () {
              Navigator.pop(context);
              Navigator.push(
                context,
                MaterialPageRoute(builder: (_) => const SettingsScreen()),
              );
            },
            child: const Text('Go to Settings'),
          ),
        ],
      ),
    );
  }

  void _onRecordingComplete(String filePath) {
    // Start transcription
    if (kDebugMode && kIsWeb) {
      debugPrint('[HOME] _onRecordingComplete called with filePath: $filePath');
    }
    ref.read(transcriptionProvider.notifier).transcribe(filePath);
  }

  @override
  Widget build(BuildContext context) {
    if (kDebugMode && kIsWeb) {
      debugPrint('[HOME] build called');
    }
    final transcriptionState = ref.watch(transcriptionProvider);
    if (kDebugMode && kIsWeb) {
      debugPrint('[HOME] transcriptionState: $transcriptionState');
    }

    return Scaffold(
      appBar: AppBar(
        title: const Text('Oracy'),
        centerTitle: true,
        actions: [
          // Sync status indicator (disabled on web for now)
          if (!kIsWeb) const SyncStatusIndicator(),
          IconButton(
            icon: const Icon(Icons.history),
            tooltip: 'History',
            onPressed: () {
              Navigator.push(
                context,
                MaterialPageRoute(builder: (_) => const HistoryScreen()),
              );
            },
          ),
          IconButton(
            icon: const Icon(Icons.settings),
            tooltip: 'Settings',
            onPressed: () {
              Navigator.push(
                context,
                MaterialPageRoute(builder: (_) => const SettingsScreen()),
              );
            },
          ),
        ],
      ),
      body: SafeArea(
        child: Column(
          children: [
            // Main content area
            Expanded(
              child: Center(child: _buildMainContent(transcriptionState)),
            ),

            // Recording button at bottom (hide during transcription)
            if (transcriptionState is! TranscriptionUploading &&
                transcriptionState is! TranscriptionProcessing)
              Padding(
                padding: const EdgeInsets.only(bottom: 48),
                child: RecordingButton(
                  onRecordingComplete: _onRecordingComplete,
                ),
              ),
          ],
        ),
      ),
    );
  }

  Widget _buildMainContent(TranscriptionState state) {
    return switch (state) {
      TranscriptionIdle() => const _IdleContent(),
      TranscriptionUploading(:final progress) => _UploadingContent(
        progress: progress,
      ),
      TranscriptionProcessing() => const _ProcessingContent(),
      TranscriptionSuccess() => const _IdleContent(), // Already navigated away
      TranscriptionVoiceNoteDeleted() => const _VoiceNoteDeletedContent(),
      TranscriptionError(
        :final message,
        :final errorType,
        :final isRetryable,
      ) =>
        _ErrorContent(
          message: message,
          errorType: errorType,
          isRetryable: isRetryable,
        ),
    };
  }
}

class _VoiceNoteDeletedContent extends ConsumerWidget {
  const _VoiceNoteDeletedContent();

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final theme = Theme.of(context);

    return Padding(
      padding: const EdgeInsets.all(24),
      child: Column(
        mainAxisAlignment: MainAxisAlignment.center,
        children: [
          Icon(
            Icons.delete_outline,
            size: 64,
            color: theme.colorScheme.primary,
          ),
          const SizedBox(height: 24),
          Text('Voice Note Unavailable', style: theme.textTheme.titleLarge),
          const SizedBox(height: 12),
          Text(
            'The upload was already accepted, but the voice note has since been deleted.',
            textAlign: TextAlign.center,
            style: theme.textTheme.bodyMedium?.copyWith(
              color: theme.colorScheme.onSurfaceVariant,
            ),
          ),
          const SizedBox(height: 24),
          OutlinedButton(
            onPressed: () {
              ref.read(transcriptionProvider.notifier).reset();
            },
            child: const Text('Dismiss'),
          ),
        ],
      ),
    );
  }
}

/// Content shown when idle (ready to record).
class _IdleContent extends ConsumerWidget {
  const _IdleContent();

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final theme = Theme.of(context);

    return Column(
      mainAxisAlignment: MainAxisAlignment.center,
      children: [
        // Recording timer (shows during recording)
        const RecordingTimer(),
        const SizedBox(height: 24),

        // Amplitude indicator (shows during recording)
        const AmplitudeIndicator(),
        const SizedBox(height: 24),

        // Instruction text
        Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            Text('Tap to Record', style: theme.textTheme.headlineSmall),
            const SizedBox(height: 8),
            Text(
              'Your voice, transcribed',
              style: theme.textTheme.bodyMedium?.copyWith(
                color: theme.colorScheme.onSurfaceVariant,
              ),
            ),
          ],
        ),
      ],
    );
  }
}

/// Content shown while uploading audio.
class _UploadingContent extends StatelessWidget {
  final double progress;

  const _UploadingContent({required this.progress});

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);

    return Column(
      mainAxisAlignment: MainAxisAlignment.center,
      children: [
        SizedBox(
          width: 120,
          height: 120,
          child: Stack(
            alignment: Alignment.center,
            children: [
              CircularProgressIndicator(
                value: progress > 0 ? progress : null,
                strokeWidth: 8,
                backgroundColor: theme.colorScheme.surfaceContainerHighest,
              ),
              Icon(
                Icons.cloud_upload,
                size: 48,
                color: theme.colorScheme.primary,
              ),
            ],
          ),
        ),
        const SizedBox(height: 24),
        Text('Uploading...', style: theme.textTheme.titleMedium),
        const SizedBox(height: 8),
        Text(
          '${(progress * 100).toInt()}%',
          style: theme.textTheme.bodyMedium?.copyWith(
            color: theme.colorScheme.onSurfaceVariant,
          ),
        ),
      ],
    );
  }
}

/// Content shown while processing/transcribing.
class _ProcessingContent extends StatelessWidget {
  const _ProcessingContent();

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);

    return Column(
      mainAxisAlignment: MainAxisAlignment.center,
      children: [
        SizedBox(
          width: 120,
          height: 120,
          child: Stack(
            alignment: Alignment.center,
            children: [
              const CircularProgressIndicator(strokeWidth: 8),
              Icon(
                Icons.auto_awesome,
                size: 48,
                color: theme.colorScheme.primary,
              ),
            ],
          ),
        ),
        const SizedBox(height: 24),
        Text('Transcribing...', style: theme.textTheme.titleMedium),
        const SizedBox(height: 8),
        Text(
          'This may take a moment',
          style: theme.textTheme.bodyMedium?.copyWith(
            color: theme.colorScheme.onSurfaceVariant,
          ),
        ),
      ],
    );
  }
}

/// Content shown when an error occurs.
class _ErrorContent extends ConsumerWidget {
  final String message;
  final TranscriptionErrorType errorType;
  final bool isRetryable;

  const _ErrorContent({
    required this.message,
    required this.errorType,
    required this.isRetryable,
  });

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final theme = Theme.of(context);
    final (icon, title) = _getIconAndTitle(errorType);

    return Padding(
      padding: const EdgeInsets.all(24),
      child: Column(
        mainAxisAlignment: MainAxisAlignment.center,
        children: [
          Icon(icon, size: 64, color: theme.colorScheme.error),
          const SizedBox(height: 24),
          Text(
            title,
            style: theme.textTheme.titleLarge?.copyWith(
              color: theme.colorScheme.error,
            ),
          ),
          const SizedBox(height: 12),
          Text(
            message,
            textAlign: TextAlign.center,
            style: theme.textTheme.bodyMedium?.copyWith(
              color: theme.colorScheme.onSurfaceVariant,
            ),
          ),
          const SizedBox(height: 24),
          Row(
            mainAxisAlignment: MainAxisAlignment.center,
            children: [
              if (isRetryable)
                OutlinedButton.icon(
                  onPressed: () {
                    ref.read(transcriptionProvider.notifier).retry();
                  },
                  icon: const Icon(Icons.refresh),
                  label: const Text('Retry'),
                )
              else
                OutlinedButton(
                  onPressed: () {
                    ref.read(transcriptionProvider.notifier).reset();
                  },
                  child: const Text('Dismiss'),
                ),
              if (errorType == TranscriptionErrorType.auth) ...[
                const SizedBox(width: 12),
                FilledButton.icon(
                  onPressed: () {
                    Navigator.push(
                      context,
                      MaterialPageRoute(builder: (_) => const SettingsScreen()),
                    );
                  },
                  icon: const Icon(Icons.settings),
                  label: const Text('Settings'),
                ),
              ],
            ],
          ),
        ],
      ),
    );
  }

  (IconData, String) _getIconAndTitle(TranscriptionErrorType type) {
    return switch (type) {
      TranscriptionErrorType.auth => (Icons.key_off, 'Authentication Error'),
      TranscriptionErrorType.network => (Icons.wifi_off, 'Connection Error'),
      TranscriptionErrorType.timeout => (Icons.timer_off, 'Request Timeout'),
      TranscriptionErrorType.fileValidation => (
        Icons.file_present,
        'Invalid File',
      ),
      TranscriptionErrorType.transcription => (
        Icons.text_fields,
        'Transcription Failed',
      ),
      TranscriptionErrorType.unknown => (Icons.error_outline, 'Error'),
    };
  }
}
