PRAGMA foreign_keys = ON;

CREATE VIRTUAL TABLE voice_note_versions_fts USING fts5(
    api_key_id UNINDEXED,
    voice_note_id UNINDEXED,
    text,
    content='voice_note_versions',
    content_rowid='rowid'
);

INSERT INTO voice_note_versions_fts (rowid, api_key_id, voice_note_id, text)
SELECT rowid, api_key_id, voice_note_id, text
FROM voice_note_versions;

CREATE TRIGGER voice_note_versions_fts_insert
AFTER INSERT ON voice_note_versions
BEGIN
    INSERT INTO voice_note_versions_fts (rowid, api_key_id, voice_note_id, text)
    VALUES (NEW.rowid, NEW.api_key_id, NEW.voice_note_id, NEW.text);
END;

CREATE TRIGGER voice_note_versions_fts_delete
AFTER DELETE ON voice_note_versions
BEGIN
    INSERT INTO voice_note_versions_fts (
        voice_note_versions_fts, rowid, api_key_id, voice_note_id, text
    )
    VALUES ('delete', OLD.rowid, OLD.api_key_id, OLD.voice_note_id, OLD.text);
END;

CREATE TRIGGER voice_note_versions_fts_update
AFTER UPDATE ON voice_note_versions
BEGIN
    INSERT INTO voice_note_versions_fts (
        voice_note_versions_fts, rowid, api_key_id, voice_note_id, text
    )
    VALUES ('delete', OLD.rowid, OLD.api_key_id, OLD.voice_note_id, OLD.text);

    INSERT INTO voice_note_versions_fts (rowid, api_key_id, voice_note_id, text)
    VALUES (NEW.rowid, NEW.api_key_id, NEW.voice_note_id, NEW.text);
END;
