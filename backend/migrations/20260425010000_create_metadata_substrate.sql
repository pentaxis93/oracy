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

CREATE TABLE transcript_tags (
    api_key_id TEXT NOT NULL,
    transcript_id TEXT NOT NULL,
    tag_id TEXT NOT NULL,
    PRIMARY KEY (api_key_id, transcript_id, tag_id),
    FOREIGN KEY (api_key_id, transcript_id) REFERENCES transcripts(api_key_id, id) ON DELETE CASCADE,
    FOREIGN KEY (api_key_id, tag_id) REFERENCES tags(api_key_id, id) ON DELETE CASCADE
);

CREATE INDEX transcript_tags_tag_idx
    ON transcript_tags (api_key_id, tag_id, transcript_id);

CREATE TRIGGER transcripts_session_owner_insert
BEFORE INSERT ON transcripts
WHEN NEW.session_id IS NOT NULL
BEGIN
    SELECT RAISE(ABORT, 'transcript session must belong to same owner')
    WHERE NOT EXISTS (
        SELECT 1 FROM sessions
        WHERE api_key_id = NEW.api_key_id AND id = NEW.session_id
    );
END;

CREATE TRIGGER transcripts_session_owner_update
BEFORE UPDATE OF api_key_id, session_id ON transcripts
WHEN NEW.session_id IS NOT NULL
BEGIN
    SELECT RAISE(ABORT, 'transcript session must belong to same owner')
    WHERE NOT EXISTS (
        SELECT 1 FROM sessions
        WHERE api_key_id = NEW.api_key_id AND id = NEW.session_id
    );
END;

CREATE TRIGGER sessions_delete_null_transcripts
AFTER DELETE ON sessions
BEGIN
    UPDATE transcripts
    SET session_id = NULL
    WHERE api_key_id = OLD.api_key_id AND session_id = OLD.id;
END;
