import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:oracy/services/transcription_service.dart';
import 'package:oracy/widgets/permission_dialog.dart';

/// Screen that displays the result of a transcription.
class VoiceNoteResultScreen extends ConsumerStatefulWidget {
  final VoiceNoteResponse voiceNote;
  final bool wasAutoCopied;

  const VoiceNoteResultScreen({
    super.key,
    required this.voiceNote,
    this.wasAutoCopied = false,
  });

  @override
  ConsumerState<VoiceNoteResultScreen> createState() =>
      _VoiceNoteResultScreenState();
}

class _VoiceNoteResultScreenState extends ConsumerState<VoiceNoteResultScreen> {
  @override
  void initState() {
    super.initState();
    // Show snackbar after build if voiceNote was auto-copied
    if (widget.wasAutoCopied) {
      WidgetsBinding.instance.addPostFrameCallback((_) {
        ScaffoldMessenger.of(context).showSnackBar(
          const SnackBar(
            content: Text('Voice note copied to clipboard'),
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
        title: const Text('Voice note'),
        centerTitle: true,
        actions: [
          IconButton(
            icon: const Icon(Icons.copy),
            tooltip: 'Copy voice note',
            onPressed: () => _copyVoiceNote(context),
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
                      widget.voiceNote.audioDurationSeconds,
                    ),
                    tooltip: 'Audio duration',
                  ),
                  if (widget.voiceNote.language != null)
                    _MetadataItem(
                      icon: Icons.language,
                      label: widget.voiceNote.language!.toUpperCase(),
                      tooltip: 'Language',
                    ),
                  _MetadataItem(
                    icon: Icons.attach_money,
                    label:
                        '\$${(widget.voiceNote.costCents / 100).toStringAsFixed(3)}',
                    tooltip: 'Cost',
                  ),
                ],
              ),
            ),

            // Voice note text
            Expanded(
              child: SingleChildScrollView(
                padding: const EdgeInsets.all(16),
                child: SelectableText(
                  widget.voiceNote.text,
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
                      onPressed: () => _copyVoiceNote(context),
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

  void _copyVoiceNote(BuildContext context) {
    Clipboard.setData(ClipboardData(text: widget.voiceNote.text));
    ScaffoldMessenger.of(context).showSnackBar(
      const SnackBar(
        content: Text('Voice note copied to clipboard'),
        duration: Duration(seconds: 2),
      ),
    );
  }

  Future<void> _startNewRecording(BuildContext context) async {
    final started = await startRecordingWithPermission(context, ref);
    if (!started || !context.mounted) {
      return;
    }

    // Reset transcription state
    ref.read(transcriptionProvider.notifier).reset();
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
