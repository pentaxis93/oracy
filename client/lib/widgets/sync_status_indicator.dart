import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:oracy/db/database.dart';
import 'package:oracy/services/background_sync_service.dart';
import 'package:oracy/services/upload_queue_service.dart';
import 'package:path/path.dart' as p;

typedef SyncTrigger = Future<void> Function();

final syncTriggerProvider = Provider<SyncTrigger>((ref) {
  return BackgroundSyncService.triggerImmediateSync;
});

/// Widget showing the current sync status in the app bar.
///
/// Displays:
/// - Checkmark when synced (no unsynced recordings)
/// - Badge with count when there are unsynced recordings
/// - Manual sync trigger on tap
class SyncStatusIndicator extends ConsumerWidget {
  const SyncStatusIndicator({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final pendingCountAsync = ref.watch(pendingUploadCountProvider);

    return pendingCountAsync.when(
      data: (count) => _SyncStatusButton(pendingCount: count),
      loading: () => const _SyncStatusButton(pendingCount: 0, isLoading: true),
      error: (_, _) => const _SyncStatusButton(pendingCount: 0, hasError: true),
    );
  }
}

class _SyncStatusButton extends ConsumerStatefulWidget {
  final int pendingCount;
  final bool isLoading;
  final bool hasError;

  const _SyncStatusButton({
    required this.pendingCount,
    this.isLoading = false,
    this.hasError = false,
  });

  @override
  ConsumerState<_SyncStatusButton> createState() => _SyncStatusButtonState();
}

class _SyncStatusButtonState extends ConsumerState<_SyncStatusButton>
    with SingleTickerProviderStateMixin {
  late AnimationController _animationController;
  bool _isSyncing = false;

  @override
  void initState() {
    super.initState();
    _animationController = AnimationController(
      vsync: this,
      duration: const Duration(seconds: 1),
    );
  }

  @override
  void dispose() {
    _animationController.dispose();
    super.dispose();
  }

  Future<void> _triggerSync() async {
    if (_isSyncing) return;

    setState(() => _isSyncing = true);
    _animationController.repeat();

    try {
      await ref.read(syncTriggerProvider)();

      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          const SnackBar(
            content: Text('Sync triggered'),
            duration: Duration(seconds: 2),
          ),
        );
      }
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(
            content: Text('Failed to trigger sync: $e'),
            backgroundColor: Theme.of(context).colorScheme.error,
          ),
        );
      }
    } finally {
      await Future.delayed(const Duration(milliseconds: 500));
      if (mounted) {
        _animationController.stop();
        _animationController.reset();
        setState(() => _isSyncing = false);
      }
    }
  }

  Future<void> _showPanelAndTriggerSync() async {
    unawaited(_triggerSync());

    await showModalBottomSheet<void>(
      context: context,
      isScrollControlled: true,
      builder: (context) => const _SyncStatusSheet(),
    );
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);

    if (widget.isLoading) {
      return IconButton(
        icon: const SizedBox(
          width: 20,
          height: 20,
          child: CircularProgressIndicator(strokeWidth: 2),
        ),
        onPressed: null,
        tooltip: 'Loading sync status...',
      );
    }

    if (widget.hasError) {
      return IconButton(
        icon: Icon(Icons.sync_problem, color: theme.colorScheme.error),
        onPressed: _triggerSync,
        tooltip: 'Sync error - tap to retry',
      );
    }

    if (widget.pendingCount == 0) {
      return IconButton(
        icon: Icon(Icons.cloud_done, color: theme.colorScheme.primary),
        onPressed: _triggerSync,
        tooltip: 'All synced',
      );
    }

    return IconButton(
      icon: _isSyncing
          ? RotationTransition(
              turns: _animationController,
              child: const Icon(Icons.sync),
            )
          : Badge(
              label: Text(
                widget.pendingCount > 99 ? '99+' : '${widget.pendingCount}',
              ),
              child: const Icon(Icons.cloud_upload_outlined),
            ),
      onPressed: _showPanelAndTriggerSync,
      tooltip:
          '${widget.pendingCount} unsynced recording${widget.pendingCount == 1 ? '' : 's'} - tap for details and to run sync',
    );
  }
}

class _SyncStatusSheet extends StatelessWidget {
  const _SyncStatusSheet();

  @override
  Widget build(BuildContext context) {
    return SafeArea(
      child: Padding(
        padding: const EdgeInsets.fromLTRB(16, 16, 16, 24),
        child: SingleChildScrollView(child: const SyncStatusPanel()),
      ),
    );
  }
}

/// Full sync status widget for displaying in a panel or dialog.
///
/// Shows more detailed information about sync status and manual controls.
class SyncStatusPanel extends ConsumerWidget {
  const SyncStatusPanel({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final pendingCountAsync = ref.watch(pendingUploadCountProvider);
    final terminalFailuresAsync = ref.watch(terminalFailureUploadsProvider);
    final theme = Theme.of(context);

    return pendingCountAsync.when(
      data: (count) {
        final terminalFailures =
            terminalFailuresAsync.asData?.value ?? const <PendingUpload>[];
        final children = <Widget>[
          if (count == 0)
            ListTile(
              contentPadding: EdgeInsets.zero,
              leading: Icon(
                Icons.cloud_done,
                color: theme.colorScheme.primary,
                size: 32,
              ),
              title: const Text('All synced'),
              subtitle: const Text('No unsynced recordings'),
            )
          else
            ListTile(
              contentPadding: EdgeInsets.zero,
              leading: Badge(
                label: Text('$count'),
                child: Icon(
                  Icons.cloud_upload_outlined,
                  color: theme.colorScheme.tertiary,
                  size: 32,
                ),
              ),
              title: Text('$count unsynced recording${count == 1 ? '' : 's'}'),
              subtitle: const Text(
                'May retry automatically or need manual attention',
              ),
              trailing: FilledButton.tonal(
                onPressed: () {
                  ref.read(syncTriggerProvider)();
                },
                child: const Text('Sync now'),
              ),
            ),
        ];

        if (terminalFailuresAsync.isLoading && terminalFailures.isEmpty) {
          children.add(
            const Padding(
              padding: EdgeInsets.symmetric(vertical: 16),
              child: Center(child: CircularProgressIndicator()),
            ),
          );
        }

        if (terminalFailuresAsync.hasError) {
          children.add(
            Padding(
              padding: const EdgeInsets.only(top: 12),
              child: Text(
                'Could not load failed recordings: ${terminalFailuresAsync.error}',
                style: TextStyle(color: theme.colorScheme.error),
              ),
            ),
          );
        }

        if (terminalFailures.isNotEmpty) {
          children.add(const Divider(height: 32));
          children.add(
            Text('Needs manual attention', style: theme.textTheme.titleMedium),
          );
          children.add(const SizedBox(height: 8));
          children.addAll(
            terminalFailures.map(
              (upload) => Padding(
                padding: const EdgeInsets.only(bottom: 12),
                child: _TerminalFailureCard(upload: upload),
              ),
            ),
          );
        }

        return Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: children,
        );
      },
      loading: () => const ListTile(
        contentPadding: EdgeInsets.zero,
        leading: CircularProgressIndicator(),
        title: Text('Checking sync status...'),
      ),
      error: (error, _) => ListTile(
        contentPadding: EdgeInsets.zero,
        leading: Icon(
          Icons.sync_problem,
          color: theme.colorScheme.error,
          size: 32,
        ),
        title: const Text('Sync error'),
        subtitle: Text(error.toString()),
        trailing: IconButton(
          icon: const Icon(Icons.refresh),
          onPressed: () => ref.read(syncTriggerProvider)(),
        ),
      ),
    );
  }
}

class _TerminalFailureCard extends ConsumerWidget {
  final PendingUpload upload;

  const _TerminalFailureCard({required this.upload});

  Future<void> _deleteRecording(BuildContext context, WidgetRef ref) async {
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: const Text('Delete recording?'),
        content: Text(
          'Delete ${p.basename(upload.audioPath)} and clear this sync failure? This cannot be undone.',
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(context, false),
            child: const Text('Cancel'),
          ),
          TextButton(
            onPressed: () => Navigator.pop(context, true),
            style: TextButton.styleFrom(foregroundColor: Colors.red),
            child: const Text('Delete'),
          ),
        ],
      ),
    );

    if (confirmed != true || !context.mounted) {
      return;
    }

    try {
      await ref.read(terminalFailureDeleteActionProvider)(upload);
    } catch (e) {
      if (!context.mounted) {
        return;
      }

      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(
          content: Text('Failed to delete recording: $e'),
          backgroundColor: Theme.of(context).colorScheme.error,
        ),
      );
    }
  }

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final theme = Theme.of(context);

    return Card(
      child: Padding(
        padding: const EdgeInsets.all(16),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(
              p.basename(upload.audioPath),
              style: theme.textTheme.titleSmall,
            ),
            const SizedBox(height: 8),
            Text(upload.errorMessage ?? 'Manual action required.'),
            const SizedBox(height: 12),
            Align(
              alignment: Alignment.centerLeft,
              child: TextButton(
                onPressed: () => _deleteRecording(context, ref),
                style: TextButton.styleFrom(
                  foregroundColor: theme.colorScheme.error,
                ),
                child: const Text('Delete recording'),
              ),
            ),
          ],
        ),
      ),
    );
  }
}
