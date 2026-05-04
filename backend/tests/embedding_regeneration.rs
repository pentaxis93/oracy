use std::sync::{Arc, Mutex};

use oracy_backend::embedding::{
    EmbeddingEngine, EmbeddingFailure, EmbeddingInput, EmbeddingOutput,
};
use oracy_backend::embedding_regeneration::{
    EmbeddingRegenerationConfig, EmbeddingRegenerationOutcome,
    process_one_embedding_regeneration_job,
};
use oracy_backend::storage::{Storage, decode_embedding_vector};
use tempfile::TempDir;
use time::Duration;
use time::macros::datetime;
use tokio::sync::Barrier;
use tokio::time::{Duration as TokioDuration, sleep};

const NOTE_ID: &str = "01JS8D6E2S3T1J7H9J2Q2N4P5R";
const VERSION_ID: &str = "01JS9P1D6CK9M0N1P2Q3R4S5T6";

#[tokio::test]
async fn regeneration_worker_replaces_the_current_embedding_for_the_edited_version() {
    let (_tempdir, storage) = storage().await;
    insert_voice_note(&storage).await;
    storage
        .update_voice_note_text(
            "owner-a",
            NOTE_ID,
            "edited text",
            datetime!(2026-04-24 18:03:00 UTC),
        )
        .await
        .expect("update text");
    let engine = FakeEmbeddingEngine::success(vec![0.2, 0.4, 0.6]);

    let outcome = process_one_embedding_regeneration_job(
        &storage,
        &engine,
        EmbeddingRegenerationConfig::test(),
    )
    .await
    .expect("process regeneration");

    assert_eq!(
        outcome,
        EmbeddingRegenerationOutcome::Replaced {
            voice_note_id: NOTE_ID.to_owned()
        }
    );
    assert_eq!(
        engine.inputs(),
        vec![EmbeddingInput {
            text: "edited text".to_owned()
        }]
    );
    let embedding = storage
        .get_current_embedding("owner-a", NOTE_ID)
        .await
        .expect("embedding lookup")
        .expect("embedding exists");
    assert_eq!(
        decode_embedding_vector(&embedding.vector).expect("encoded vector"),
        vec![0.2, 0.4, 0.6]
    );
    assert!(
        storage
            .claim_next_embedding_regeneration_job(
                "next-lease",
                datetime!(2026-04-24 18:10:00 UTC),
                datetime!(2026-04-24 18:15:00 UTC),
            )
            .await
            .expect("claim next")
            .is_none()
    );
}

#[tokio::test]
async fn stale_regeneration_completion_does_not_replace_a_newer_current_embedding() {
    let (_tempdir, storage) = storage().await;
    insert_voice_note(&storage).await;
    let updated = storage
        .update_voice_note_text(
            "owner-a",
            NOTE_ID,
            "first edit",
            datetime!(2026-04-24 18:03:00 UTC),
        )
        .await
        .expect("first edit");
    let claimed = storage
        .claim_next_embedding_regeneration_job(
            "stale-lease",
            datetime!(2026-04-24 18:04:00 UTC),
            datetime!(2026-04-24 18:09:00 UTC),
        )
        .await
        .expect("claim")
        .expect("job exists");
    assert_eq!(
        claimed.voice_note_version_id,
        updated.unwrap_record().current_version_id
    );
    storage
        .update_voice_note_text(
            "owner-a",
            NOTE_ID,
            "second edit",
            datetime!(2026-04-24 18:05:00 UTC),
        )
        .await
        .expect("second edit");

    let replaced = storage
        .complete_embedding_regeneration_job_if_current(
            "owner-a",
            NOTE_ID,
            &claimed.voice_note_version_id,
            "stale-lease",
            oracy_backend::storage::NewEmbedding {
                model: "text-embedding-3-small".to_owned(),
                vector: oracy_backend::storage::encode_embedding_vector(&[0.9, 0.9, 0.9]),
                created_at: datetime!(2026-04-24 18:06:00 UTC),
            },
        )
        .await
        .expect("complete stale");

    assert!(!replaced);
    let embedding = storage
        .get_current_embedding("owner-a", NOTE_ID)
        .await
        .expect("embedding lookup")
        .expect("embedding exists");
    assert_eq!(
        decode_embedding_vector(&embedding.vector).expect("encoded vector"),
        vec![0.1, 0.1, 0.1]
    );
    let latest = storage
        .claim_next_embedding_regeneration_job(
            "latest-lease",
            datetime!(2026-04-24 18:10:00 UTC),
            datetime!(2026-04-24 18:15:00 UTC),
        )
        .await
        .expect("claim latest")
        .expect("latest job exists");
    assert_eq!(latest.text, "second edit");
}

#[tokio::test]
async fn expired_regeneration_worker_does_not_replace_reclaimed_embedding() {
    let (_tempdir, storage) = storage().await;
    insert_voice_note(&storage).await;
    storage
        .update_voice_note_text(
            "owner-a",
            NOTE_ID,
            "edited text",
            datetime!(2026-04-24 18:03:00 UTC),
        )
        .await
        .expect("update text");
    let stale_started = Arc::new(Barrier::new(2));
    let stale_release = Arc::new(Barrier::new(2));
    let stale_engine = BlockingEmbeddingEngine {
        output: EmbeddingOutput {
            model: "text-embedding-3-small".to_owned(),
            vector: vec![0.9, 0.9, 0.9],
        },
        started: stale_started.clone(),
        release: stale_release.clone(),
    };
    let stale_storage = storage.clone();
    let stale_handle = tokio::spawn(async move {
        process_one_embedding_regeneration_job(
            &stale_storage,
            &stale_engine,
            EmbeddingRegenerationConfig {
                lease_duration: Duration::milliseconds(1),
                ..EmbeddingRegenerationConfig::test()
            },
        )
        .await
    });

    stale_started.wait().await;
    sleep(TokioDuration::from_millis(20)).await;
    let fresh_engine = FakeEmbeddingEngine::success(vec![0.2, 0.4, 0.6]);

    let fresh_outcome = process_one_embedding_regeneration_job(
        &storage,
        &fresh_engine,
        EmbeddingRegenerationConfig::test(),
    )
    .await
    .expect("fresh worker completes reclaimed job");

    assert_eq!(
        fresh_outcome,
        EmbeddingRegenerationOutcome::Replaced {
            voice_note_id: NOTE_ID.to_owned()
        }
    );
    stale_release.wait().await;
    let stale_outcome = stale_handle
        .await
        .expect("stale worker joins")
        .expect("stale worker completes");
    assert_eq!(
        stale_outcome,
        EmbeddingRegenerationOutcome::Stale {
            voice_note_id: NOTE_ID.to_owned()
        }
    );
    let embedding = storage
        .get_current_embedding("owner-a", NOTE_ID)
        .await
        .expect("embedding lookup")
        .expect("embedding exists");
    assert_eq!(
        decode_embedding_vector(&embedding.vector).expect("encoded vector"),
        vec![0.2, 0.4, 0.6]
    );
}

#[tokio::test]
async fn transient_regeneration_failures_retry_without_removing_the_previous_embedding() {
    let (_tempdir, storage) = storage().await;
    insert_voice_note(&storage).await;
    storage
        .update_voice_note_text(
            "owner-a",
            NOTE_ID,
            "edited text",
            datetime!(2026-04-24 18:03:00 UTC),
        )
        .await
        .expect("update text");
    let engine = FakeEmbeddingEngine::transient();

    let outcome = process_one_embedding_regeneration_job(
        &storage,
        &engine,
        EmbeddingRegenerationConfig::test(),
    )
    .await
    .expect("process regeneration");

    assert_eq!(
        outcome,
        EmbeddingRegenerationOutcome::RetryWaiting {
            voice_note_id: NOTE_ID.to_owned()
        }
    );
    let embedding = storage
        .get_current_embedding("owner-a", NOTE_ID)
        .await
        .expect("embedding lookup")
        .expect("embedding exists");
    assert_eq!(
        decode_embedding_vector(&embedding.vector).expect("encoded vector"),
        vec![0.1, 0.1, 0.1]
    );
    assert!(
        storage
            .claim_next_embedding_regeneration_job(
                "early-lease",
                datetime!(2026-04-24 18:03:30 UTC),
                datetime!(2026-04-24 18:08:30 UTC),
            )
            .await
            .expect("early claim")
            .is_none()
    );
}

async fn storage() -> (TempDir, Storage) {
    let tempdir = TempDir::new().expect("tempdir");
    let storage = Storage::connect(&tempdir.path().join("oracy.sqlite"))
        .await
        .expect("connect storage");
    (tempdir, storage)
}

async fn insert_voice_note(storage: &Storage) {
    sqlx::query(
        r#"
        INSERT INTO voice_notes (
            id, api_key_id, audio_duration_seconds, audio_format, audio_size_bytes,
            language, model, processing_time_ms, cost_cents, created_at, recorded_at, session_id
        )
        VALUES (?, 'owner-a', 1.0, 'wav', 42, 'en', 'gpt-4o-mini-transcribe', 100, NULL,
            '2026-04-24T18:00:00.000000000Z', '2026-04-24T17:59:00.000000000Z', NULL)
        "#,
    )
    .bind(NOTE_ID)
    .execute(storage.pool())
    .await
    .expect("insert voice note");
    sqlx::query(
        r#"
        INSERT INTO voice_note_versions (
            id, api_key_id, voice_note_id, version_number, text, created_at
        )
        VALUES (?, 'owner-a', ?, 1, 'initial text', '2026-04-24T18:00:00.000000000Z')
        "#,
    )
    .bind(VERSION_ID)
    .bind(NOTE_ID)
    .execute(storage.pool())
    .await
    .expect("insert version");
    sqlx::query(
        r#"
        INSERT INTO embeddings (voice_note_id, api_key_id, model, vector, created_at)
        VALUES (?, 'owner-a', 'text-embedding-3-small', ?, '2026-04-24T18:00:00.000000000Z')
        "#,
    )
    .bind(NOTE_ID)
    .bind(oracy_backend::storage::encode_embedding_vector(&[
        0.1, 0.1, 0.1,
    ]))
    .execute(storage.pool())
    .await
    .expect("insert embedding");
}

trait UpdatedRecord {
    fn unwrap_record(self) -> Box<oracy_backend::storage::VoiceNoteRecord>;
}

impl UpdatedRecord for oracy_backend::storage::UpdateVoiceNoteTextOutcome {
    fn unwrap_record(self) -> Box<oracy_backend::storage::VoiceNoteRecord> {
        match self {
            oracy_backend::storage::UpdateVoiceNoteTextOutcome::Updated(record) => record,
            oracy_backend::storage::UpdateVoiceNoteTextOutcome::NotFound => {
                panic!("voice note should exist")
            }
        }
    }
}

#[derive(Clone)]
struct FakeEmbeddingEngine {
    output: Result<EmbeddingOutput, EmbeddingFailure>,
    inputs: Arc<Mutex<Vec<EmbeddingInput>>>,
}

impl FakeEmbeddingEngine {
    fn success(vector: Vec<f32>) -> Self {
        Self {
            output: Ok(EmbeddingOutput {
                model: "text-embedding-3-small".to_owned(),
                vector,
            }),
            inputs: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn transient() -> Self {
        Self {
            output: Err(EmbeddingFailure::Transient {
                failure_code: "engine_rate_limited".to_owned(),
                message: "slow down".to_owned(),
                retry_after_seconds: Some(60),
            }),
            inputs: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn inputs(&self) -> Vec<EmbeddingInput> {
        self.inputs.lock().expect("inputs").clone()
    }
}

impl EmbeddingEngine for FakeEmbeddingEngine {
    async fn embed(&self, input: EmbeddingInput) -> Result<EmbeddingOutput, EmbeddingFailure> {
        self.inputs.lock().expect("inputs").push(input);
        self.output.clone()
    }
}

struct BlockingEmbeddingEngine {
    output: EmbeddingOutput,
    started: Arc<Barrier>,
    release: Arc<Barrier>,
}

impl EmbeddingEngine for BlockingEmbeddingEngine {
    async fn embed(&self, _input: EmbeddingInput) -> Result<EmbeddingOutput, EmbeddingFailure> {
        self.started.wait().await;
        self.release.wait().await;
        Ok(self.output.clone())
    }
}
