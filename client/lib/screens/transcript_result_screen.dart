import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:oracy/services/recording_service.dart';
import 'package:oracy/services/transcription_service.dart';

/// Screen that displays the result of a transcription.
class TranscriptResultScreen extends ConsumerStatefulWidget {
  final TranscriptResponse transcript;
  final bool wasAutoCopied;

  const TranscriptResultScreen({
    super.key,
    required this.transcript,
    this.wasAutoCopied = false,
  });

  @override
  ConsumerState<TranscriptResultScreen> createState() =>
      _TranscriptResultScreenState();
}

class _TranscriptResultScreenState
    extends ConsumerState<TranscriptResultScreen> {
  @override
  void initState() {
    super.initState();
    // Show snackbar after build if transcript was auto-copied
    if (widget.wasAutoCopied) {
      WidgetsBinding.instance.addPostFrameCallback((_) {
        ScaffoldMessenger.of(context).showSnackBar(
          const SnackBar(
            content: Text('Transcript copied to clipboard'),
            duration: Duration(seconds: 2),
            behavior: SnackBarBehavior.floating,
          ),
        );
      });
    }
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);

    return Scaffold(
      appBar: AppBar(
        title: const Text('Transcript'),
        centerTitle: true,
        actions: [
          IconButton(
            icon: const Icon(Icons.copy),
            tooltip: 'Copy transcript',
            onPressed: () => _copyTranscript(context),
          ),
        ],
      ),
      body: SafeArea(
        child: Column(
          children: [
            // Metadata bar
            Container(
              padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 12),
              color: theme.colorScheme.surfaceContainerHighest,
              child: Row(
                mainAxisAlignment: MainAxisAlignment.spaceAround,
                children: [
                  _MetadataItem(
                    icon: Icons.timer_outlined,
                    label: _formatDuration(
                      widget.transcript.audioDurationSeconds,
                    ),
                    tooltip: 'Audio duration',
                  ),
                  if (widget.transcript.transcriptLanguage != null)
                    _MetadataItem(
                      icon: Icons.language,
                      label: widget.transcript.transcriptLanguage!
                          .toUpperCase(),
                      tooltip: 'Language',
                    ),
                  _MetadataItem(
                    icon: Icons.attach_money,
                    label:
                        '\$${(widget.transcript.costCents / 100).toStringAsFixed(3)}',
                    tooltip: 'Cost',
                  ),
                ],
              ),
            ),

            // Transcript text
            Expanded(
              child: SingleChildScrollView(
                padding: const EdgeInsets.all(16),
                child: SelectableText(
                  widget.transcript.transcript,
                  style: theme.textTheme.bodyLarge?.copyWith(height: 1.6),
                ),
              ),
            ),

            // Action buttons
            Padding(
              padding: const EdgeInsets.all(16),
              child: Row(
                children: [
                  Expanded(
                    child: OutlinedButton.icon(
                      onPressed: () => _copyTranscript(context),
                      icon: const Icon(Icons.copy),
                      label: const Text('Copy'),
                    ),
                  ),
                  const SizedBox(width: 12),
                  Expanded(
                    child: FilledButton.icon(
                      onPressed: () => _startNewRecording(context),
                      icon: const Icon(Icons.mic),
                      label: const Text('New Recording'),
                    ),
                  ),
                ],
              ),
            ),
          ],
        ),
      ),
    );
  }

  void _copyTranscript(BuildContext context) {
    Clipboard.setData(ClipboardData(text: widget.transcript.transcript));
    ScaffoldMessenger.of(context).showSnackBar(
      const SnackBar(
        content: Text('Transcript copied to clipboard'),
        duration: Duration(seconds: 2),
      ),
    );
  }

  void _startNewRecording(BuildContext context) {
    // Reset transcription state
    ref.read(transcriptionProvider.notifier).reset();
    // Start recording immediately
    ref.read(recordingProvider.notifier).startRecording();
    // Pop back to home page
    Navigator.of(context).popUntil((route) => route.isFirst);
  }

  String _formatDuration(double seconds) {
    final totalSeconds = seconds.round();
    final minutes = totalSeconds ~/ 60;
    final secs = totalSeconds % 60;
    if (minutes > 0) {
      return '${minutes}m ${secs}s';
    }
    return '${secs}s';
  }
}

/// A small metadata display item.
class _MetadataItem extends StatelessWidget {
  final IconData icon;
  final String label;
  final String tooltip;

  const _MetadataItem({
    required this.icon,
    required this.label,
    required this.tooltip,
  });

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);

    return Tooltip(
      message: tooltip,
      child: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          Icon(icon, size: 16, color: theme.colorScheme.onSurfaceVariant),
          const SizedBox(width: 4),
          Text(
            label,
            style: theme.textTheme.bodySmall?.copyWith(
              color: theme.colorScheme.onSurfaceVariant,
            ),
          ),
        ],
      ),
    );
  }
}
