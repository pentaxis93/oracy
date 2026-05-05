import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:oracy/models/voice_note.dart';

class VoiceNoteDetailScreen extends StatelessWidget {
  final VoiceNote voiceNote;

  const VoiceNoteDetailScreen({super.key, required this.voiceNote});

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);

    return Scaffold(
      appBar: AppBar(
        title: const Text('Voice Note'),
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
            Container(
              padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 12),
              color: theme.colorScheme.surfaceContainerHighest,
              child: Row(
                mainAxisAlignment: MainAxisAlignment.spaceAround,
                children: [
                  _MetadataItem(
                    icon: Icons.timer_outlined,
                    label: _formatDuration(voiceNote.audioDurationSeconds),
                    tooltip: 'Audio duration',
                  ),
                  _MetadataItem(
                    icon: Icons.language,
                    label: voiceNote.language.toUpperCase(),
                    tooltip: 'Language',
                  ),
                  _MetadataItem(
                    icon: Icons.attach_money,
                    label: _formatCost(voiceNote.costCents),
                    tooltip: 'Cost',
                  ),
                ],
              ),
            ),
            Expanded(
              child: SingleChildScrollView(
                padding: const EdgeInsets.all(16),
                child: SelectableText(
                  voiceNote.text,
                  style: theme.textTheme.bodyLarge?.copyWith(height: 1.6),
                ),
              ),
            ),
            Padding(
              padding: const EdgeInsets.all(16),
              child: SizedBox(
                width: double.infinity,
                child: OutlinedButton.icon(
                  onPressed: () => _copyVoiceNote(context),
                  icon: const Icon(Icons.copy),
                  label: const Text('Copy'),
                ),
              ),
            ),
          ],
        ),
      ),
    );
  }

  void _copyVoiceNote(BuildContext context) {
    Clipboard.setData(ClipboardData(text: voiceNote.text));
    ScaffoldMessenger.of(context).showSnackBar(
      const SnackBar(
        content: Text('Voice note copied to clipboard'),
        duration: Duration(seconds: 2),
      ),
    );
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

  String _formatCost(int? cents) {
    if (cents == null) {
      return 'N/A';
    }
    return '\$${(cents / 100).toStringAsFixed(3)}';
  }
}

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
