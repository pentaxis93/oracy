import 'package:flutter_test/flutter_test.dart';
import 'package:oracy/models/voice_note.dart';

Map<String, dynamic> _voiceNoteJson({
  Object? costCents = 1,
  Object? language = 'en',
}) {
  return {
    'id': '01JS8D6E2S3T1J7H9J2Q2N4P5R',
    'current_version_id': '01JS9P1D6CK9M0N1P2Q3R4S5T6',
    'text': 'Hello, this is a voice note.',
    'audio_duration_seconds': 12.5,
    'audio_format': 'm4a',
    'audio_size_bytes': 401280,
    'language': language,
    'model': 'gpt-4o-mini-transcribe',
    'processing_time_ms': 1843,
    'cost_cents': costCents,
    'created_at': '2026-04-21T18:31:19Z',
    'recorded_at': '2026-04-21T18:29:55Z',
    'session_id': null,
    'tags': [
      {
        'id': '01JS9P0Q0THR2X3E4A5B6C7D8E',
        'name': 'Meeting',
        'created_at': '2026-04-21T18:31:30Z',
      },
    ],
  };
}

void main() {
  test(
    'Given a v0.1.0 voice note JSON object, When parsed, Then all voice-note fields are available',
    () {
      final voiceNote = VoiceNote.fromJson(_voiceNoteJson());

      expect(voiceNote.id, '01JS8D6E2S3T1J7H9J2Q2N4P5R');
      expect(voiceNote.currentVersionId, '01JS9P1D6CK9M0N1P2Q3R4S5T6');
      expect(voiceNote.text, 'Hello, this is a voice note.');
      expect(voiceNote.audioDurationSeconds, 12.5);
      expect(voiceNote.audioFormat, 'm4a');
      expect(voiceNote.audioSizeBytes, 401280);
      expect(voiceNote.language, 'en');
      expect(voiceNote.model, 'gpt-4o-mini-transcribe');
      expect(voiceNote.processingTimeMs, 1843);
      expect(voiceNote.costCents, 1);
      expect(voiceNote.createdAt, DateTime.parse('2026-04-21T18:31:19Z'));
      expect(voiceNote.recordedAt, DateTime.parse('2026-04-21T18:29:55Z'));
      expect(voiceNote.sessionId, isNull);
      expect(voiceNote.tags.single.id, '01JS9P0Q0THR2X3E4A5B6C7D8E');
      expect(voiceNote.tags.single.name, 'Meeting');
      expect(
        voiceNote.tags.single.createdAt,
        DateTime.parse('2026-04-21T18:31:30Z'),
      );
    },
  );

  test(
    'Given cost_cents is null, When a voice note is parsed, Then the nullable cost is preserved',
    () {
      final voiceNote = VoiceNote.fromJson(_voiceNoteJson(costCents: null));

      expect(voiceNote.costCents, isNull);
    },
  );

  test(
    'Given language is null, When a voice note is parsed, Then the nullable language is preserved',
    () {
      final voiceNote = VoiceNote.fromJson(_voiceNoteJson(language: null));

      expect(voiceNote.language, isNull);
    },
  );

  test(
    'Given a voice-note collection envelope, When parsed, Then items and nullable next_cursor are preserved',
    () {
      final response = VoiceNoteCollectionResponse.fromJson({
        'items': [_voiceNoteJson()],
        'next_cursor': null,
      });

      expect(response.items.single.id, '01JS8D6E2S3T1J7H9J2Q2N4P5R');
      expect(response.nextCursor, isNull);
      expect(response.hasMore, isFalse);
    },
  );
}
