class VoiceNoteCollectionResponse {
  final List<VoiceNote> items;
  final String? nextCursor;

  const VoiceNoteCollectionResponse({
    required this.items,
    required this.nextCursor,
  });

  factory VoiceNoteCollectionResponse.fromJson(Map<String, dynamic> json) {
    return VoiceNoteCollectionResponse(
      items: (json['items'] as List)
          .map((e) => VoiceNote.fromJson(e as Map<String, dynamic>))
          .toList(),
      nextCursor: json['next_cursor'] as String?,
    );
  }

  bool get hasMore => nextCursor != null;
}

class VoiceNote {
  final String id;
  final String currentVersionId;
  final String text;
  final double audioDurationSeconds;
  final String audioFormat;
  final int audioSizeBytes;
  final String language;
  final String model;
  final int processingTimeMs;
  final int? costCents;
  final DateTime createdAt;
  final DateTime recordedAt;
  final String? sessionId;
  final List<VoiceNoteTag> tags;

  const VoiceNote({
    required this.id,
    required this.currentVersionId,
    required this.text,
    required this.audioDurationSeconds,
    required this.audioFormat,
    required this.audioSizeBytes,
    required this.language,
    required this.model,
    required this.processingTimeMs,
    required this.costCents,
    required this.createdAt,
    required this.recordedAt,
    required this.sessionId,
    required this.tags,
  });

  factory VoiceNote.fromJson(Map<String, dynamic> json) {
    return VoiceNote(
      id: json['id'] as String,
      currentVersionId: json['current_version_id'] as String,
      text: json['text'] as String,
      audioDurationSeconds: (json['audio_duration_seconds'] as num).toDouble(),
      audioFormat: json['audio_format'] as String,
      audioSizeBytes: json['audio_size_bytes'] as int,
      language: json['language'] as String,
      model: json['model'] as String,
      processingTimeMs: json['processing_time_ms'] as int,
      costCents: json['cost_cents'] as int?,
      createdAt: DateTime.parse(json['created_at'] as String),
      recordedAt: DateTime.parse(json['recorded_at'] as String),
      sessionId: json['session_id'] as String?,
      tags: (json['tags'] as List)
          .map((e) => VoiceNoteTag.fromJson(e as Map<String, dynamic>))
          .toList(),
    );
  }
}

class VoiceNoteTag {
  final String id;
  final String name;
  final DateTime createdAt;

  const VoiceNoteTag({
    required this.id,
    required this.name,
    required this.createdAt,
  });

  factory VoiceNoteTag.fromJson(Map<String, dynamic> json) {
    return VoiceNoteTag(
      id: json['id'] as String,
      name: json['name'] as String,
      createdAt: DateTime.parse(json['created_at'] as String),
    );
  }
}
