import 'package:dio/dio.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:oracy/models/voice_note.dart';
import 'package:oracy/services/api_client.dart';

class HistoryService {
  final Dio _dio;

  HistoryService(this._dio);

  Future<VoiceNoteCollectionResponse> getVoiceNotes({
    String? cursor,
    int? limit,
    String? query,
  }) async {
    final queryParameters = <String, dynamic>{};
    if (cursor != null) {
      queryParameters['cursor'] = cursor;
    }
    if (limit != null) {
      queryParameters['limit'] = limit;
    }
    if (query != null && query.isNotEmpty) {
      queryParameters['q'] = query;
    }

    final response = await _dio.get(
      '/api/v1/voice-notes',
      queryParameters: queryParameters,
    );

    return VoiceNoteCollectionResponse.fromJson(
      response.data as Map<String, dynamic>,
    );
  }

  Future<VoiceNote> getVoiceNote(String voiceNoteId) async {
    final response = await _dio.get('/api/v1/voice-notes/$voiceNoteId');
    return VoiceNote.fromJson(response.data as Map<String, dynamic>);
  }
}

final historyServiceProvider = Provider<HistoryService>((ref) {
  final dio = ref.watch(apiClientProvider);
  return HistoryService(dio);
});

class VoiceNoteHistoryState {
  final List<VoiceNote> voiceNotes;
  final String? nextCursor;
  final bool isLoading;
  final bool isLoadingMore;
  final String? error;
  final String query;

  const VoiceNoteHistoryState({
    this.voiceNotes = const [],
    this.nextCursor,
    this.isLoading = false,
    this.isLoadingMore = false,
    this.error,
    this.query = '',
  });

  VoiceNoteHistoryState copyWith({
    List<VoiceNote>? voiceNotes,
    String? nextCursor,
    bool clearNextCursor = false,
    bool? isLoading,
    bool? isLoadingMore,
    String? error,
    bool clearError = false,
    String? query,
  }) {
    return VoiceNoteHistoryState(
      voiceNotes: voiceNotes ?? this.voiceNotes,
      nextCursor: clearNextCursor ? null : nextCursor ?? this.nextCursor,
      isLoading: isLoading ?? this.isLoading,
      isLoadingMore: isLoadingMore ?? this.isLoadingMore,
      error: clearError ? null : error,
      query: query ?? this.query,
    );
  }

  bool get hasMore => nextCursor != null;
  bool get isEmpty => voiceNotes.isEmpty && !isLoading;
}

class VoiceNoteHistoryNotifier extends Notifier<VoiceNoteHistoryState> {
  int _requestVersion = 0;

  HistoryService get _service => ref.read(historyServiceProvider);

  @override
  VoiceNoteHistoryState build() {
    return const VoiceNoteHistoryState();
  }

  Future<void> loadInitial() async {
    if (state.isLoading) return;

    final query = state.query;
    final requestVersion = ++_requestVersion;
    state = state.copyWith(isLoading: true, clearError: true);

    try {
      final response = await _service.getVoiceNotes(query: query);
      if (!_isCurrentRequest(requestVersion, query)) return;

      state = VoiceNoteHistoryState(
        voiceNotes: response.items,
        nextCursor: response.nextCursor,
        isLoading: false,
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

  Future<void> loadMore() async {
    if (state.isLoading || state.isLoadingMore || !state.hasMore) return;

    final query = state.query;
    final cursor = state.nextCursor;
    final itemCount = state.voiceNotes.length;
    final requestVersion = ++_requestVersion;
    state = state.copyWith(isLoadingMore: true);

    try {
      final response = await _service.getVoiceNotes(
        cursor: cursor,
        query: query,
      );
      if (!_isCurrentRequest(requestVersion, query) ||
          state.nextCursor != cursor ||
          state.voiceNotes.length != itemCount) {
        return;
      }

      state = state.copyWith(
        voiceNotes: [...state.voiceNotes, ...response.items],
        nextCursor: response.nextCursor,
        clearNextCursor: response.nextCursor == null,
        isLoadingMore: false,
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

  Future<void> refresh() async {
    state = VoiceNoteHistoryState(query: state.query);
    await loadInitial();
  }

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

final voiceNoteHistoryProvider =
    NotifierProvider<VoiceNoteHistoryNotifier, VoiceNoteHistoryState>(
      VoiceNoteHistoryNotifier.new,
    );
