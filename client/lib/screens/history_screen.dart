import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:oracy/models/voice_note.dart';
import 'package:oracy/screens/voice_note_detail_screen.dart';
import 'package:oracy/services/history_service.dart';

/// Notifier for the search query state.
class SearchQueryNotifier extends Notifier<String> {
  @override
  String build() => '';

  void update(String query) {
    state = query;
  }

  void clear() {
    state = '';
  }
}

/// Provider for the current search query.
final searchQueryProvider = NotifierProvider<SearchQueryNotifier, String>(
  SearchQueryNotifier.new,
);

/// Groups a list of voice notes by date category.
class _DateGroupedVoiceNotes {
  final List<VoiceNote> today;
  final List<VoiceNote> yesterday;
  final Map<String, List<VoiceNote>> older;

  _DateGroupedVoiceNotes({
    required this.today,
    required this.yesterday,
    required this.older,
  });

  factory _DateGroupedVoiceNotes.fromList(List<VoiceNote> voiceNotes) {
    final now = DateTime.now();
    final todayStart = DateTime(now.year, now.month, now.day);
    final yesterdayStart = todayStart.subtract(const Duration(days: 1));

    final today = <VoiceNote>[];
    final yesterday = <VoiceNote>[];
    final older = <String, List<VoiceNote>>{};

    for (final voiceNote in voiceNotes) {
      if (voiceNote.createdAt.isAfter(todayStart)) {
        today.add(voiceNote);
      } else if (voiceNote.createdAt.isAfter(yesterdayStart)) {
        yesterday.add(voiceNote);
      } else {
        final key = _formatMonthYear(voiceNote.createdAt);
        older.putIfAbsent(key, () => []).add(voiceNote);
      }
    }

    return _DateGroupedVoiceNotes(
      today: today,
      yesterday: yesterday,
      older: older,
    );
  }

  static String _formatMonthYear(DateTime date) {
    const months = [
      'January',
      'February',
      'March',
      'April',
      'May',
      'June',
      'July',
      'August',
      'September',
      'October',
      'November',
      'December',
    ];
    return '${months[date.month - 1]} ${date.year}';
  }

  bool get isEmpty => today.isEmpty && yesterday.isEmpty && older.isEmpty;
}

/// Screen displaying voice-note history with search and pagination.
class HistoryScreen extends ConsumerStatefulWidget {
  const HistoryScreen({super.key});

  @override
  ConsumerState<HistoryScreen> createState() => _HistoryScreenState();
}

class _HistoryScreenState extends ConsumerState<HistoryScreen> {
  final _scrollController = ScrollController();
  final _searchController = TextEditingController();
  Timer? _debounceTimer;

  @override
  void initState() {
    super.initState();
    // Load initial data
    WidgetsBinding.instance.addPostFrameCallback((_) {
      ref.read(voiceNoteHistoryProvider.notifier).loadInitial();
    });

    // Listen for scroll to load more
    _scrollController.addListener(_onScroll);

    // Listen for search input changes
    _searchController.addListener(_onSearchChanged);
  }

  @override
  void dispose() {
    _scrollController.dispose();
    _searchController.dispose();
    _debounceTimer?.cancel();
    super.dispose();
  }

  void _onScroll() {
    // Load more when near the bottom.
    if (_scrollController.position.pixels >=
        _scrollController.position.maxScrollExtent - 200) {
      ref.read(voiceNoteHistoryProvider.notifier).loadMore();
    }
  }

  void _onSearchChanged() {
    // Debounce search input (300ms)
    _debounceTimer?.cancel();
    _debounceTimer = Timer(const Duration(milliseconds: 300), () {
      final query = _searchController.text;
      ref.read(searchQueryProvider.notifier).update(query);
      unawaited(ref.read(voiceNoteHistoryProvider.notifier).search(query));
    });
  }

  void _clearSearch() {
    _searchController.clear();
    ref.read(searchQueryProvider.notifier).clear();
    unawaited(ref.read(voiceNoteHistoryProvider.notifier).search(''));
  }

  @override
  Widget build(BuildContext context) {
    final state = ref.watch(voiceNoteHistoryProvider);
    final searchQuery = ref.watch(searchQueryProvider);
    final theme = Theme.of(context);

    return Scaffold(
      appBar: AppBar(
        title: const Text('History'),
        centerTitle: true,
        actions: [
          IconButton(
            icon: const Icon(Icons.refresh),
            tooltip: 'Refresh',
            onPressed: state.isLoading
                ? null
                : () => ref.read(voiceNoteHistoryProvider.notifier).refresh(),
          ),
        ],
      ),
      body: Column(
        children: [
          // Search bar
          Padding(
            padding: const EdgeInsets.all(16),
            child: TextField(
              controller: _searchController,
              decoration: InputDecoration(
                hintText: 'Search voice notes...',
                prefixIcon: const Icon(Icons.search),
                suffixIcon: searchQuery.isNotEmpty
                    ? IconButton(
                        icon: const Icon(Icons.clear),
                        onPressed: _clearSearch,
                        tooltip: 'Clear search',
                      )
                    : null,
                filled: true,
                fillColor: theme.colorScheme.surfaceContainerHighest,
                border: OutlineInputBorder(
                  borderRadius: BorderRadius.circular(12),
                  borderSide: BorderSide.none,
                ),
                contentPadding: const EdgeInsets.symmetric(
                  horizontal: 16,
                  vertical: 12,
                ),
              ),
            ),
          ),

          // Results
          Expanded(
            child: _buildBody(state, state.voiceNotes, searchQuery, theme),
          ),
        ],
      ),
    );
  }

  Widget _buildBody(
    VoiceNoteHistoryState state,
    List<VoiceNote> filteredVoiceNotes,
    String searchQuery,
    ThemeData theme,
  ) {
    if (state.isLoading && state.voiceNotes.isEmpty) {
      return const Center(child: CircularProgressIndicator());
    }

    if (state.error != null && state.voiceNotes.isEmpty) {
      return _ErrorView(
        message: state.error!,
        onRetry: () =>
            ref.read(voiceNoteHistoryProvider.notifier).loadInitial(),
      );
    }

    // Show no results message for search
    if (state.query.isNotEmpty && filteredVoiceNotes.isEmpty) {
      return _NoSearchResults(query: state.query);
    }

    if (state.isEmpty) {
      return const _EmptyView();
    }

    final grouped = _DateGroupedVoiceNotes.fromList(filteredVoiceNotes);

    return RefreshIndicator(
      onRefresh: () => ref.read(voiceNoteHistoryProvider.notifier).refresh(),
      child: ListView(
        controller: _scrollController,
        padding: const EdgeInsets.only(bottom: 16),
        children: [
          // Today
          if (grouped.today.isNotEmpty) ...[
            _DateHeader(label: 'Today'),
            ...grouped.today.map(
              (voiceNote) => _VoiceNoteTile(
                voiceNote: voiceNote,
                onTap: () => _openVoiceNote(voiceNote),
                searchQuery: searchQuery,
              ),
            ),
          ],

          // Yesterday
          if (grouped.yesterday.isNotEmpty) ...[
            _DateHeader(label: 'Yesterday'),
            ...grouped.yesterday.map(
              (voiceNote) => _VoiceNoteTile(
                voiceNote: voiceNote,
                onTap: () => _openVoiceNote(voiceNote),
                searchQuery: searchQuery,
              ),
            ),
          ],

          // Older (by month)
          for (final entry in grouped.older.entries) ...[
            _DateHeader(label: entry.key),
            ...entry.value.map(
              (voiceNote) => _VoiceNoteTile(
                voiceNote: voiceNote,
                onTap: () => _openVoiceNote(voiceNote),
                searchQuery: searchQuery,
              ),
            ),
          ],

          // Loading indicator
          if (state.isLoadingMore)
            const Padding(
              padding: EdgeInsets.all(16),
              child: Center(child: CircularProgressIndicator()),
            ),
        ],
      ),
    );
  }

  void _openVoiceNote(VoiceNote voiceNote) {
    Navigator.push(
      context,
      MaterialPageRoute(
        builder: (_) => VoiceNoteDetailScreen(voiceNote: voiceNote),
      ),
    );
  }
}

/// Date header for grouping voice notes.
class _DateHeader extends StatelessWidget {
  final String label;

  const _DateHeader({required this.label});

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);

    return Padding(
      padding: const EdgeInsets.fromLTRB(16, 16, 16, 8),
      child: Text(
        label,
        style: theme.textTheme.titleSmall?.copyWith(
          color: theme.colorScheme.primary,
          fontWeight: FontWeight.w600,
        ),
      ),
    );
  }
}

/// A tile displaying a voice-note summary with expand/collapse.
class _VoiceNoteTile extends StatefulWidget {
  final VoiceNote voiceNote;
  final VoidCallback onTap;
  final String searchQuery;

  const _VoiceNoteTile({
    required this.voiceNote,
    required this.onTap,
    this.searchQuery = '',
  });

  @override
  State<_VoiceNoteTile> createState() => _VoiceNoteTileState();
}

class _VoiceNoteTileState extends State<_VoiceNoteTile> {
  bool _isExpanded = false;

  void _copyVoiceNote() {
    Clipboard.setData(ClipboardData(text: widget.voiceNote.text));
    ScaffoldMessenger.of(context).showSnackBar(
      const SnackBar(
        content: Text('Voice note copied to clipboard'),
        duration: Duration(seconds: 2),
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final voiceNote = widget.voiceNote;

    final fullText = voiceNote.text;
    final previewText = fullText.length > 100
        ? '${fullText.substring(0, 100)}...'
        : fullText;
    final displayText = _isExpanded ? fullText : previewText;
    final canExpand = fullText.length > 100;

    return Card(
      margin: const EdgeInsets.symmetric(horizontal: 16, vertical: 4),
      child: InkWell(
        onTap: widget.onTap,
        borderRadius: BorderRadius.circular(12),
        child: Padding(
          padding: const EdgeInsets.all(16),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text(
                displayText.isEmpty ? '(No voice-note text)' : displayText,
                style: theme.textTheme.bodyMedium,
              ),

              // Expand/collapse button
              if (canExpand)
                Padding(
                  padding: const EdgeInsets.only(top: 8),
                  child: GestureDetector(
                    onTap: () => setState(() => _isExpanded = !_isExpanded),
                    child: Text(
                      _isExpanded ? 'Show less' : 'Show more',
                      style: theme.textTheme.bodySmall?.copyWith(
                        color: theme.colorScheme.primary,
                        fontWeight: FontWeight.w500,
                      ),
                    ),
                  ),
                ),

              const SizedBox(height: 12),

              // Metadata and actions row
              Row(
                children: [
                  // Duration
                  _MetadataChip(
                    icon: Icons.timer_outlined,
                    label: _formatDuration(voiceNote.audioDurationSeconds),
                  ),
                  const SizedBox(width: 12),

                  // Timestamp
                  _MetadataChip(
                    icon: Icons.schedule,
                    label: _formatTime(voiceNote.createdAt),
                  ),

                  const Spacer(),

                  // Copy button
                  IconButton(
                    icon: const Icon(Icons.copy, size: 20),
                    onPressed: _copyVoiceNote,
                    tooltip: 'Copy voice note',
                    visualDensity: VisualDensity.compact,
                    style: IconButton.styleFrom(
                      foregroundColor: theme.colorScheme.onSurfaceVariant,
                    ),
                  ),

                  Container(
                    padding: const EdgeInsets.symmetric(
                      horizontal: 8,
                      vertical: 4,
                    ),
                    decoration: BoxDecoration(
                      color: theme.colorScheme.surfaceContainerHighest,
                      borderRadius: BorderRadius.circular(4),
                    ),
                    child: Text(
                      voiceNote.language.toUpperCase(),
                      style: theme.textTheme.labelSmall?.copyWith(
                        color: theme.colorScheme.onSurfaceVariant,
                      ),
                    ),
                  ),
                ],
              ),
            ],
          ),
        ),
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

  String _formatTime(DateTime timestamp) {
    // Just show time for grouped items
    final hour = timestamp.hour;
    final minute = timestamp.minute.toString().padLeft(2, '0');
    final period = hour >= 12 ? 'PM' : 'AM';
    final hour12 = hour == 0 ? 12 : (hour > 12 ? hour - 12 : hour);
    return '$hour12:$minute $period';
  }
}

/// Small metadata chip with icon and label.
class _MetadataChip extends StatelessWidget {
  final IconData icon;
  final String label;

  const _MetadataChip({required this.icon, required this.label});

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);

    return Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        Icon(icon, size: 14, color: theme.colorScheme.onSurfaceVariant),
        const SizedBox(width: 4),
        Text(
          label,
          style: theme.textTheme.bodySmall?.copyWith(
            color: theme.colorScheme.onSurfaceVariant,
          ),
        ),
      ],
    );
  }
}

/// Empty state when no voice notes exist.
class _EmptyView extends StatelessWidget {
  const _EmptyView();

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);

    return Center(
      child: Padding(
        padding: const EdgeInsets.all(32),
        child: Column(
          mainAxisAlignment: MainAxisAlignment.center,
          children: [
            Icon(
              Icons.history,
              size: 64,
              color: theme.colorScheme.onSurfaceVariant,
            ),
            const SizedBox(height: 16),
            Text('No voice notes yet', style: theme.textTheme.titleLarge),
            const SizedBox(height: 8),
            Text(
              'Your voice-note history will appear here.',
              textAlign: TextAlign.center,
              style: theme.textTheme.bodyMedium?.copyWith(
                color: theme.colorScheme.onSurfaceVariant,
              ),
            ),
          ],
        ),
      ),
    );
  }
}

/// Error view with retry button.
class _ErrorView extends StatelessWidget {
  final String message;
  final VoidCallback onRetry;

  const _ErrorView({required this.message, required this.onRetry});

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);

    return Center(
      child: Padding(
        padding: const EdgeInsets.all(32),
        child: Column(
          mainAxisAlignment: MainAxisAlignment.center,
          children: [
            Icon(Icons.error_outline, size: 64, color: theme.colorScheme.error),
            const SizedBox(height: 16),
            Text('Unable to load history', style: theme.textTheme.titleLarge),
            const SizedBox(height: 8),
            Text(
              message,
              textAlign: TextAlign.center,
              style: theme.textTheme.bodyMedium?.copyWith(
                color: theme.colorScheme.onSurfaceVariant,
              ),
            ),
            const SizedBox(height: 24),
            FilledButton.icon(
              onPressed: onRetry,
              icon: const Icon(Icons.refresh),
              label: const Text('Retry'),
            ),
          ],
        ),
      ),
    );
  }
}

/// Shown when search returns no results.
class _NoSearchResults extends StatelessWidget {
  final String query;

  const _NoSearchResults({required this.query});

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);

    return Center(
      child: Padding(
        padding: const EdgeInsets.all(32),
        child: Column(
          mainAxisAlignment: MainAxisAlignment.center,
          children: [
            Icon(
              Icons.search_off,
              size: 64,
              color: theme.colorScheme.onSurfaceVariant,
            ),
            const SizedBox(height: 16),
            Text('No results found', style: theme.textTheme.titleLarge),
            const SizedBox(height: 8),
            Text(
              'No voice notes match "$query"',
              textAlign: TextAlign.center,
              style: theme.textTheme.bodyMedium?.copyWith(
                color: theme.colorScheme.onSurfaceVariant,
              ),
            ),
          ],
        ),
      ),
    );
  }
}
