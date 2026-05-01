import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:oracy/services/api_client.dart';
import 'package:oracy/services/history_service.dart';
import 'package:oracy/services/permission_service.dart';
import 'package:oracy/services/recording_service.dart';
import 'package:oracy/services/transcription_service.dart';
import 'package:oracy/services/upload_queue_service.dart';

/// Creates a test app with Riverpod provider scope.
///
/// Usage:
/// ```dart
/// await tester.pumpWidget(
///   createTestApp(
///     child: MyWidget(),
///   ),
/// );
/// ```
Widget createTestApp({
  required Widget child,
  ThemeMode themeMode = ThemeMode.light,
}) {
  return ProviderScope(
    child: MaterialApp(
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
      themeMode: themeMode,
      home: child,
    ),
  );
}

/// Mock implementation of SecureStorageService for testing.
class MockSecureStorage extends SecureStorageService {
  String? _apiKey;

  MockSecureStorage({String? apiKey}) : _apiKey = apiKey;

  @override
  Future<String?> getApiKey() async => _apiKey;

  @override
  Future<void> setApiKey(String apiKey) async {
    _apiKey = apiKey;
  }

  @override
  Future<void> deleteApiKey() async {
    _apiKey = null;
  }

  @override
  Future<bool> hasApiKey() async => _apiKey != null && _apiKey!.isNotEmpty;
}

/// Mock recording notifier for testing.
class MockRecordingNotifier extends RecordingNotifier {
  RecordingInfo _mockState;
  int startCount = 0;

  MockRecordingNotifier([RecordingInfo? initialState])
    : _mockState =
          initialState ?? const RecordingInfo(state: RecordingState.idle);

  @override
  RecordingInfo build() => _mockState;

  /// Set the mock state for testing.
  void setMockState(RecordingInfo newState) {
    _mockState = newState;
    state = newState;
  }

  @override
  Future<void> startRecording() async {
    startCount++;
    state = RecordingInfo(
      state: RecordingState.recording,
      filePath: '/mock/recording.m4a',
      duration: Duration.zero,
    );
  }

  @override
  Future<String?> stopRecording() async {
    state = const RecordingInfo(
      state: RecordingState.completed,
      filePath: '/mock/recording.m4a',
      duration: Duration(seconds: 5),
    );
    return '/mock/recording.m4a';
  }

  @override
  Future<void> cancelRecording() async {
    state = const RecordingInfo(state: RecordingState.idle);
  }

  @override
  void reset() {
    state = const RecordingInfo(state: RecordingState.idle);
  }
}

/// Mock permission service for recording-entrypoint tests.
class MockPermissionService extends PermissionService {
  MicrophonePermissionStatus status;
  MicrophonePermissionStatus requestedStatus;
  int checkCount = 0;
  int requestCount = 0;
  int openSettingsCount = 0;

  MockPermissionService({
    this.status = MicrophonePermissionStatus.granted,
    MicrophonePermissionStatus? requestedStatus,
  }) : requestedStatus = requestedStatus ?? status;

  @override
  Future<MicrophonePermissionStatus> checkMicrophonePermission() async {
    checkCount++;
    return status;
  }

  @override
  Future<MicrophonePermissionStatus> requestMicrophonePermission() async {
    requestCount++;
    status = requestedStatus;
    return status;
  }

  @override
  Future<bool> openSettings() async {
    openSettingsCount++;
    return true;
  }
}

/// Mock transcription notifier for testing.
class MockTranscriptionNotifier extends TranscriptionNotifier {
  TranscriptionState _mockState;

  MockTranscriptionNotifier([TranscriptionState? initialState])
    : _mockState = initialState ?? const TranscriptionIdle();

  @override
  TranscriptionState build() => _mockState;

  /// Set the mock state for testing.
  void setMockState(TranscriptionState newState) {
    _mockState = newState;
    state = newState;
  }

  @override
  Future<void> transcribe(String filePath, {String? language}) async {
    state = const TranscriptionUploading(progress: 0.5);
    await Future.delayed(const Duration(milliseconds: 10));
    state = const TranscriptionProcessing();
    await Future.delayed(const Duration(milliseconds: 10));
    state = TranscriptionSuccess(createMockVoiceNote());
  }

  @override
  Future<bool> retry({String? language}) async {
    await transcribe('/mock/file.m4a', language: language);
    return true;
  }

  @override
  void reset() {
    state = const TranscriptionIdle();
  }
}

/// Mock history notifier for testing.
class MockHistoryNotifier extends VoiceNoteHistoryNotifier {
  final List<VoiceNoteResponse> _mockVoiceNotes;
  final bool _hasMore;

  MockHistoryNotifier({
    List<VoiceNoteResponse>? voiceNotes,
    bool hasMore = false,
  }) : _mockVoiceNotes = voiceNotes ?? [],
       _hasMore = hasMore;

  @override
  VoiceNoteHistoryState build() => VoiceNoteHistoryState(
    voiceNotes: _mockVoiceNotes,
    hasMore: _hasMore,
    nextCursor: _hasMore ? 'mock-next-page' : null,
  );

  @override
  Future<void> loadInitial() async {
    state = state.copyWith(isLoading: true);
    await Future.delayed(const Duration(milliseconds: 10));
    state = VoiceNoteHistoryState(
      voiceNotes: _mockVoiceNotes,
      hasMore: _hasMore,
      nextCursor: _hasMore ? 'mock-next-page' : null,
    );
  }

  @override
  Future<void> loadMore() async {
    state = state.copyWith(isLoadingMore: false, hasMore: false);
  }

  @override
  Future<void> refresh() async {
    await loadInitial();
  }
}

/// Creates a mock VoiceNoteResponse for testing.
VoiceNoteResponse createMockVoiceNote({
  String? id,
  String? text,
  double? audioDurationSeconds,
  int? costCents,
  DateTime? createdAt,
  String? language,
}) {
  return VoiceNoteResponse(
    id: id ?? 'mock-id-123',
    text: text ?? 'This is a mock voice note for testing purposes.',
    audioDurationSeconds: audioDurationSeconds ?? 30.0,
    audioFormat: 'm4a',
    audioSizeBytes: 50000,
    language: language ?? 'en',
    model: 'whisper-1',
    processingTimeMs: 1500,
    costCents: costCents ?? 1,
    createdAt: createdAt ?? DateTime.now(),
  );
}

/// Creates a list of mock voice notes for testing.
List<VoiceNoteResponse> createMockVoiceNoteList({int count = 5}) {
  return List.generate(
    count,
    (index) => createMockVoiceNote(
      id: 'mock-id-$index',
      text: 'Mock voice note number ${index + 1}.',
      createdAt: DateTime.now().subtract(Duration(days: index)),
    ),
  );
}

/// Helper extension for pumping and settling widgets in tests.
extension WidgetTesterExtensions on WidgetTester {
  /// Pumps the widget and waits for all animations and async operations.
  Future<void> pumpAndSettle100() async {
    await pumpAndSettle(const Duration(milliseconds: 100));
  }
}

/// Provider overrides for testing. Use these with ProviderScope.overrides.
///
/// Example:
/// ```dart
/// await tester.pumpWidget(
///   ProviderScope(
///     overrides: [
///       secureStorageOverride('test-api-key'),
///       recordingOverride(),
///     ],
///     child: MaterialApp(home: MyWidget()),
///   ),
/// );
/// ```

/// Override for a mock secure storage with an API key.
dynamic secureStorageOverride([String? apiKey]) {
  return secureStorageProvider.overrideWith(
    (_) => MockSecureStorage(apiKey: apiKey),
  );
}

/// Override for mock recording state.
dynamic recordingOverride([RecordingInfo? initialState]) {
  return recordingProvider.overrideWith(
    () => MockRecordingNotifier(initialState),
  );
}

/// Override for mock microphone permissions.
dynamic permissionOverride(MockPermissionService service) {
  return permissionServiceProvider.overrideWith((_) => service);
}

/// Override for mock transcription state.
dynamic transcriptionOverride([TranscriptionState? initialState]) {
  return transcriptionProvider.overrideWith(
    () => MockTranscriptionNotifier(initialState),
  );
}

/// Override for mock history state.
dynamic historyOverride({
  List<VoiceNoteResponse>? voiceNotes,
  bool hasMore = false,
}) {
  return voiceNoteHistoryProvider.overrideWith(
    () => MockHistoryNotifier(voiceNotes: voiceNotes, hasMore: hasMore),
  );
}

/// Override for mock pending upload count stream.
dynamic pendingUploadCountOverride([int count = 0]) {
  return pendingUploadCountProvider.overrideWith((ref) => Stream.value(count));
}
