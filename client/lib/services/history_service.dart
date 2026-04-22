import 'package:dio/dio.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:oracy/services/api_client.dart';
import 'package:oracy/services/transcription_service.dart';

/// Response from the transcripts list endpoint.
class TranscriptListResponse {
  final List<TranscriptResponse> transcripts;
  final int total;
  final int offset;
  final int limit;

  const TranscriptListResponse({
    required this.transcripts,
    required this.total,
    required this.offset,
    required this.limit,
  });

  factory TranscriptListResponse.fromJson(Map<String, dynamic> json) {
    return TranscriptListResponse(
      transcripts: (json['transcripts'] as List)
          .map((e) => TranscriptResponse.fromJson(e as Map<String, dynamic>))
          .toList(),
      total: json['total'] as int,
      offset: json['offset'] as int,
      limit: json['limit'] as int,
    );
  }

  bool get hasMore => offset + transcripts.length < total;
}

/// Service for fetching transcript history from the API.
class HistoryService {
  final Dio _dio;

  HistoryService(this._dio);

  /// Fetch a page of transcripts.
  ///
  /// [offset] - Number of items to skip (for pagination)
  /// [limit] - Maximum items per page (default 20, max 100)
  Future<TranscriptListResponse> getTranscripts({
    int offset = 0,
    int limit = 20,
  }) async {
    final response = await _dio.get(
      '/api/v1/transcripts',
      queryParameters: {'offset': offset, 'limit': limit},
    );

    return TranscriptListResponse.fromJson(
      response.data as Map<String, dynamic>,
    );
  }

  /// Get a single transcript by ID.
  Future<TranscriptResponse> getTranscript(String id) async {
    final response = await _dio.get('/api/v1/transcripts/$id');
    return TranscriptResponse.fromJson(response.data as Map<String, dynamic>);
  }
}

/// Provider for the history service.
final historyServiceProvider = Provider<HistoryService>((ref) {
  final dio = ref.watch(apiClientProvider);
  return HistoryService(dio);
});

/// State for the paginated transcript list.
class TranscriptHistoryState {
  final List<TranscriptResponse> transcripts;
  final int total;
  final bool isLoading;
  final bool isLoadingMore;
  final String? error;
  final bool hasMore;

  const TranscriptHistoryState({
    this.transcripts = const [],
    this.total = 0,
    this.isLoading = false,
    this.isLoadingMore = false,
    this.error,
    this.hasMore = true,
  });

  TranscriptHistoryState copyWith({
    List<TranscriptResponse>? transcripts,
    int? total,
    bool? isLoading,
    bool? isLoadingMore,
    String? error,
    bool? hasMore,
  }) {
    return TranscriptHistoryState(
      transcripts: transcripts ?? this.transcripts,
      total: total ?? this.total,
      isLoading: isLoading ?? this.isLoading,
      isLoadingMore: isLoadingMore ?? this.isLoadingMore,
      error: error,
      hasMore: hasMore ?? this.hasMore,
    );
  }

  bool get isEmpty => transcripts.isEmpty && !isLoading;
}

/// Notifier for managing transcript history with pagination.
class TranscriptHistoryNotifier extends Notifier<TranscriptHistoryState> {
  static const int _pageSize = 20;

  HistoryService get _service => ref.read(historyServiceProvider);

  @override
  TranscriptHistoryState build() {
    return const TranscriptHistoryState();
  }

  /// Load the initial page of transcripts.
  Future<void> loadInitial() async {
    if (state.isLoading) return;

    state = state.copyWith(isLoading: true, error: null);

    try {
      final response = await _service.getTranscripts(
        offset: 0,
        limit: _pageSize,
      );

      state = TranscriptHistoryState(
        transcripts: response.transcripts,
        total: response.total,
        isLoading: false,
        hasMore: response.hasMore,
      );
    } on DioException catch (e) {
      state = state.copyWith(isLoading: false, error: _mapError(e));
    } catch (e) {
      state = state.copyWith(
        isLoading: false,
        error: 'Failed to load transcripts: $e',
      );
    }
  }

  /// Load more transcripts (append to existing list).
  Future<void> loadMore() async {
    if (state.isLoading || state.isLoadingMore || !state.hasMore) return;

    state = state.copyWith(isLoadingMore: true);

    try {
      final response = await _service.getTranscripts(
        offset: state.transcripts.length,
        limit: _pageSize,
      );

      state = state.copyWith(
        transcripts: [...state.transcripts, ...response.transcripts],
        total: response.total,
        isLoadingMore: false,
        hasMore: response.hasMore,
      );
    } on DioException catch (e) {
      state = state.copyWith(isLoadingMore: false, error: _mapError(e));
    } catch (e) {
      state = state.copyWith(
        isLoadingMore: false,
        error: 'Failed to load more transcripts: $e',
      );
    }
  }

  /// Refresh the list (reload from beginning).
  Future<void> refresh() async {
    state = const TranscriptHistoryState();
    await loadInitial();
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
    return 'Failed to load transcripts. Please try again.';
  }
}

/// Provider for transcript history state.
final transcriptHistoryProvider =
    NotifierProvider<TranscriptHistoryNotifier, TranscriptHistoryState>(
      TranscriptHistoryNotifier.new,
    );
