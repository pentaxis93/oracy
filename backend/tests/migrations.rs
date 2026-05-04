use std::borrow::Cow;

use sqlx::migrate::Migrator;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use sqlx::{Row, SqlitePool};
use tempfile::TempDir;

static MIGRATOR: Migrator = sqlx::migrate!("./migrations");
const EMBEDDING_REGENERATION_MIGRATION: i64 = 20260504000000;

#[tokio::test]
async fn embedding_regeneration_migration_backfills_unembedded_current_voice_notes() {
    let tempdir = TempDir::new().expect("tempdir");
    let database_path = tempdir.path().join("oracy.sqlite");
    let pool = sqlite_pool(&database_path).await;
    migrator_before(EMBEDDING_REGENERATION_MIGRATION)
        .run(&pool)
        .await
        .expect("run pre-regeneration migrations");
    seed_current_voice_note(
        &pool,
        "owner-a",
        "missing-embedding-note",
        "version-missing",
        "needs embedding",
    )
    .await;
    seed_current_voice_note(
        &pool,
        "owner-a",
        "embedded-note",
        "version-embedded",
        "already embedded",
    )
    .await;
    seed_embedding(&pool, "owner-a", "embedded-note").await;
    seed_current_voice_note(&pool, "owner-a", "blank-note", "version-blank", "   ").await;

    MIGRATOR.run(&pool).await.expect("run full migrations");

    let rows = sqlx::query(
        r#"
        SELECT voice_note_id, api_key_id, voice_note_version_id, status, retry_count,
            max_retries, created_at, updated_at
        FROM embedding_regeneration_jobs
        ORDER BY voice_note_id
        "#,
    )
    .fetch_all(&pool)
    .await
    .expect("fetch regeneration jobs");
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    assert_eq!(
        row.get::<String, _>("voice_note_id"),
        "missing-embedding-note"
    );
    assert_eq!(row.get::<String, _>("api_key_id"), "owner-a");
    assert_eq!(
        row.get::<String, _>("voice_note_version_id"),
        "version-missing"
    );
    assert_eq!(row.get::<String, _>("status"), "queued");
    assert_eq!(row.get::<i64, _>("retry_count"), 0);
    assert_eq!(row.get::<i64, _>("max_retries"), 3);
    assert_eq!(
        row.get::<String, _>("created_at"),
        "2026-04-24T18:00:00.000000000Z"
    );
    assert_eq!(
        row.get::<String, _>("updated_at"),
        "2026-04-24T18:00:00.000000000Z"
    );
}

fn migrator_before(version: i64) -> Migrator {
    Migrator {
        migrations: Cow::Owned(
            MIGRATOR
                .iter()
                .filter(|migration| migration.version < version)
                .cloned()
                .collect(),
        ),
        ignore_missing: false,
        locking: true,
        no_tx: false,
    }
}

async fn sqlite_pool(database_path: &std::path::Path) -> SqlitePool {
    let options = SqliteConnectOptions::new()
        .filename(database_path)
        .create_if_missing(true)
        .foreign_keys(true)
        .journal_mode(SqliteJournalMode::Wal);
    SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await
        .expect("connect sqlite")
}

async fn seed_current_voice_note(
    pool: &SqlitePool,
    owner: &str,
    voice_note_id: &str,
    version_id: &str,
    text: &str,
) {
    sqlx::query(
        r#"
        INSERT INTO voice_notes (
            id, api_key_id, audio_duration_seconds, audio_format, audio_size_bytes,
            language, model, processing_time_ms, cost_cents, created_at, recorded_at, session_id
        )
        VALUES (
            ?, ?, 1.0, 'wav', 42, 'en', 'gpt-4o-mini-transcribe', 100, NULL,
            '2026-04-24T18:00:00.000000000Z',
            '2026-04-24T17:59:00.000000000Z',
            NULL
        )
        "#,
    )
    .bind(voice_note_id)
    .bind(owner)
    .execute(pool)
    .await
    .expect("insert voice note");
    sqlx::query(
        r#"
        INSERT INTO voice_note_versions (
            id, api_key_id, voice_note_id, version_number, text, created_at
        )
        VALUES (?, ?, ?, 1, ?, '2026-04-24T18:00:00.000000000Z')
        "#,
    )
    .bind(version_id)
    .bind(owner)
    .bind(voice_note_id)
    .bind(text)
    .execute(pool)
    .await
    .expect("insert voice-note version");
}

async fn seed_embedding(pool: &SqlitePool, owner: &str, voice_note_id: &str) {
    sqlx::query(
        r#"
        INSERT INTO embeddings (voice_note_id, api_key_id, model, vector, created_at)
        VALUES (?, ?, 'text-embedding-3-small', x'010203', '2026-04-24T18:00:00.000000000Z')
        "#,
    )
    .bind(voice_note_id)
    .bind(owner)
    .execute(pool)
    .await
    .expect("insert embedding");
}
