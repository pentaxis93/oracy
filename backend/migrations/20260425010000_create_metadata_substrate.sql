PRAGMA foreign_keys = ON;

CREATE TABLE sessions (
    id TEXT PRIMARY KEY,
    api_key_id TEXT NOT NULL,
    name TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE UNIQUE INDEX sessions_owner_id_idx
    ON sessions (api_key_id, id);

CREATE INDEX sessions_list_idx
    ON sessions (api_key_id, created_at DESC, id DESC);

CREATE TABLE tags (
    id TEXT PRIMARY KEY,
    api_key_id TEXT NOT NULL,
    name TEXT NOT NULL,
    name_folded TEXT NOT NULL,
    created_at TEXT NOT NULL,
    UNIQUE (api_key_id, name_folded)
);

CREATE UNIQUE INDEX tags_owner_id_idx
    ON tags (api_key_id, id);

CREATE INDEX tags_list_idx
    ON tags (api_key_id, created_at DESC, id DESC);

CREATE TABLE voice_note_tags (
    api_key_id TEXT NOT NULL,
    voice_note_id TEXT NOT NULL,
    tag_id TEXT NOT NULL,
    PRIMARY KEY (api_key_id, voice_note_id, tag_id),
    FOREIGN KEY (api_key_id, voice_note_id) REFERENCES voice_notes(api_key_id, id) ON DELETE CASCADE,
    FOREIGN KEY (api_key_id, tag_id) REFERENCES tags(api_key_id, id) ON DELETE CASCADE
);

CREATE INDEX voice_note_tags_tag_idx
    ON voice_note_tags (api_key_id, tag_id, voice_note_id);

CREATE TRIGGER transcription_jobs_session_owner_insert
BEFORE INSERT ON transcription_jobs
WHEN NEW.session_id IS NOT NULL
BEGIN
    SELECT RAISE(ABORT, 'job session must belong to same owner')
    WHERE NOT EXISTS (
        SELECT 1 FROM sessions
        WHERE api_key_id = NEW.api_key_id AND id = NEW.session_id
    );
END;

CREATE TRIGGER voice_notes_session_owner_insert
BEFORE INSERT ON voice_notes
WHEN NEW.session_id IS NOT NULL
BEGIN
    SELECT RAISE(ABORT, 'voice note session must belong to same owner')
    WHERE NOT EXISTS (
        SELECT 1 FROM sessions
        WHERE api_key_id = NEW.api_key_id AND id = NEW.session_id
    );
END;

CREATE TRIGGER voice_notes_session_owner_update
BEFORE UPDATE OF api_key_id, session_id ON voice_notes
WHEN NEW.session_id IS NOT NULL
BEGIN
    SELECT RAISE(ABORT, 'voice note session must belong to same owner')
    WHERE NOT EXISTS (
        SELECT 1 FROM sessions
        WHERE api_key_id = NEW.api_key_id AND id = NEW.session_id
    );
END;

CREATE TRIGGER sessions_delete_null_voice_notes
AFTER DELETE ON sessions
BEGIN
    UPDATE voice_notes
    SET session_id = NULL
    WHERE api_key_id = OLD.api_key_id AND session_id = OLD.id;
END;
