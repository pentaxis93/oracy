import 'package:dio/dio.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:oracy/services/api_client.dart';
import 'package:oracy/services/transcription_service.dart';

/// Response from the voice notes list endpoint.
class VoiceNoteListResponse {
  final List<VoiceNoteResponse> voiceNotes;
  final String? nextCursor;

  const VoiceNoteListResponse({required this.voiceNotes, this.nextCursor});

  factory VoiceNoteListResponse.fromJson(Map<String, dynamic> json) {
    final items = json['items'] as List;
    return VoiceNoteListResponse(
      voiceNotes: items
          .map((e) => VoiceNoteResponse.fromJson(e as Map<String, dynamic>))
          .toList(),
      nextCursor: json['next_cursor'] as String?,
    );
  }

  bool get hasMore => nextCursor != null;
}

/// Service for fetching voice-note history from the API.
class HistoryService {
  final Dio _dio;

  HistoryService(this._dio);

  /// Fetch a page of voice notes.
  ///
  /// [limit] - Maximum items per page (default 20, max 100)
  Future<VoiceNoteListResponse> getVoiceNotes({
    String? cursor,
    int limit = 20,
    String? query,
  }) async {
    final response = await _dio.get(
      '/api/v1/voice-notes',
      queryParameters: {
        'limit': limit,
        'cursor': ?cursor,
        if (query != null && query.isNotEmpty) 'q': query,
      },
    );

    return VoiceNoteListResponse.fromJson(
      response.data as Map<String, dynamic>,
    );
  }

  /// Get a single voice note by ID.
  Future<VoiceNoteResponse> getVoiceNote(String id) async {
    final response = await _dio.get('/api/v1/voice-notes/$id');
    return VoiceNoteResponse.fromJson(response.data as Map<String, dynamic>);
  }
}

/// Provider for the history service.
final historyServiceProvider = Provider<HistoryService>((ref) {
  final dio = ref.watch(apiClientProvider);
  return HistoryService(dio);
});

/// State for the paginated voice-note list.
class VoiceNoteHistoryState {
  final List<VoiceNoteResponse> voiceNotes;
  final bool isLoading;
  final bool isLoadingMore;
  final String? error;
  final bool hasMore;
  final String? nextCursor;
  final String query;

  const VoiceNoteHistoryState({
    this.voiceNotes = const [],
    this.isLoading = false,
    this.isLoadingMore = false,
    this.error,
    this.hasMore = true,
    this.nextCursor,
    this.query = '',
  });

  VoiceNoteHistoryState copyWith({
    List<VoiceNoteResponse>? voiceNotes,
    bool? isLoading,
    bool? isLoadingMore,
    String? error,
    bool? hasMore,
    String? nextCursor,
    String? query,
  }) {
    return VoiceNoteHistoryState(
      voiceNotes: voiceNotes ?? this.voiceNotes,
      isLoading: isLoading ?? this.isLoading,
      isLoadingMore: isLoadingMore ?? this.isLoadingMore,
      error: error,
      hasMore: hasMore ?? this.hasMore,
      nextCursor: nextCursor ?? this.nextCursor,
      query: query ?? this.query,
    );
  }

  bool get isEmpty => voiceNotes.isEmpty && !isLoading;
}

/// Notifier for managing voice-note history with pagination.
class VoiceNoteHistoryNotifier extends Notifier<VoiceNoteHistoryState> {
  static const int _pageSize = 20;
  int _requestVersion = 0;

  HistoryService get _service => ref.read(historyServiceProvider);

  @override
  VoiceNoteHistoryState build() {
    return const VoiceNoteHistoryState();
  }

  /// Load the initial page of voice notes.
  Future<void> loadInitial() async {
    if (state.isLoading) return;

    final query = state.query;
    final requestVersion = ++_requestVersion;
    state = state.copyWith(isLoading: true, error: null);

    try {
      final response = await _service.getVoiceNotes(
        limit: _pageSize,
        query: query,
      );
      if (!_isCurrentRequest(requestVersion, query)) return;

      state = VoiceNoteHistoryState(
        voiceNotes: response.voiceNotes,
        isLoading: false,
        hasMore: response.hasMore,
        nextCursor: response.nextCursor,
        query: query,
      );
    } on DioException catch (e) {
      if (!_isCurrentRequest(requestVersion, query)) return;
      state = state.copyWith(isLoading: false, error: _mapError(e));
    } catch (e) {
      if (!_isCurrentRequest(requestVersion, query)) return;
      state = state.copyWith(
        isLoading: false,
        error: 'Failed to load voice notes: $e',
      );
    }
  }

  /// Load more voice notes (append to existing list).
  Future<void> loadMore() async {
    if (state.isLoading || state.isLoadingMore || !state.hasMore) return;

    final query = state.query;
    final cursor = state.nextCursor;
    final requestVersion = ++_requestVersion;
    final previousCount = state.voiceNotes.length;
    state = state.copyWith(isLoadingMore: true);

    try {
      final response = await _service.getVoiceNotes(
        cursor: cursor,
        limit: _pageSize,
        query: query,
      );
      if (!_isCurrentRequest(requestVersion, query) ||
          state.voiceNotes.length != previousCount) {
        return;
      }

      state = state.copyWith(
        voiceNotes: [...state.voiceNotes, ...response.voiceNotes],
        isLoadingMore: false,
        hasMore: response.hasMore,
        nextCursor: response.nextCursor,
      );
    } on DioException catch (e) {
      if (!_isCurrentRequest(requestVersion, query)) return;
      state = state.copyWith(isLoadingMore: false, error: _mapError(e));
    } catch (e) {
      if (!_isCurrentRequest(requestVersion, query)) return;
      state = state.copyWith(
        isLoadingMore: false,
        error: 'Failed to load more voice notes: $e',
      );
    }
  }

  /// Refresh the list (reload from beginning).
  Future<void> refresh() async {
    state = VoiceNoteHistoryState(query: state.query);
    await loadInitial();
  }

  /// Search voice notes using the same paginated collection contract as history.
  Future<void> search(String query) async {
    final normalizedQuery = query.trim();
    if (normalizedQuery == state.query && !state.isLoading) {
      return;
    }

    state = VoiceNoteHistoryState(query: normalizedQuery);
    await loadInitial();
  }

  bool _isCurrentRequest(int requestVersion, String query) {
    return requestVersion == _requestVersion && state.query == query;
  }

  String _mapError(DioException e) {
    if (e.response?.statusCode == 401) {
      return 'Please check your API key in Settings.';
    } else if (e.type == DioExceptionType.connectionError) {
      return 'Unable to connect. Check your internet connection.';
    } else if (e.type == DioExceptionType.connectionTimeout ||
        e.type == DioExceptionType.receiveTimeout) {
      return 'Request timed out. Please try again.';
    }
    return 'Failed to load voice notes. Please try again.';
  }
}

/// Provider for voice-note history state.
final voiceNoteHistoryProvider =
    NotifierProvider<VoiceNoteHistoryNotifier, VoiceNoteHistoryState>(
      VoiceNoteHistoryNotifier.new,
    );
