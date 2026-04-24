use std::path::PathBuf;

use oracy_backend::storage::{
    AcceptJobOutcome, NewEmbedding, NewSegment, NewTranscript, NewTranscriptVersion,
    NewTranscriptionJob, Storage, StorageError, TranscriptMaterialization,
};
use sqlx::Row;
use tempfile::TempDir;
use time::macros::datetime;
use tokio::time::{Duration, sleep};

#[tokio::test]
async fn accepted_jobs_replay_by_owner_and_reject_tuple_mismatches() {
    let (_tempdir, storage) = storage().await;
    let input = new_job("owner-a", "attempt-1", "hash-a");

    let created = match storage.accept_job(input.clone()).await.expect("accept job") {
        AcceptJobOutcome::Created(job) => job,
        other => panic!("expected created job, got {other:?}"),
    };
    assert_eq!(created.status, "queued");

    let replayed = match storage.accept_job(input.clone()).await.expect("replay job") {
        AcceptJobOutcome::Replayed(job) => job,
        other => panic!("expected replayed job, got {other:?}"),
    };
    assert_eq!(replayed.id, created.id);

    let different_owner = match storage
        .accept_job(new_job("owner-b", "attempt-1", "hash-a"))
        .await
    {
        Ok(AcceptJobOutcome::Created(job)) => job,
        other => panic!("expected owner-isolated creation, got {other:?}"),
    };
    assert_ne!(different_owner.id, created.id);
    assert!(
        storage
            .get_job("owner-b", &created.id)
            .await
            .expect("lookup")
            .is_none()
    );

    let conflict = storage
        .accept_job(new_job("owner-a", "attempt-1", "hash-b"))
        .await
        .expect("conflict outcome");
    assert_eq!(
        conflict,
        AcceptJobOutcome::Conflict(oracy_backend::storage::SubmissionConflict {
            job_id: created.id
        })
    );
}

#[tokio::test]
async fn racing_acceptance_resolves_unique_conflicts_as_replay_or_submission_conflict() {
    let (_tempdir, storage) = storage().await;
    let input = new_job("owner-a", "attempt-1", "hash-a");

    let replayed = accept_while_uncommitted_row_exists(
        &storage,
        "racing-replay-job",
        input.clone(),
        input.clone(),
    )
    .await
    .expect("racing replay should not return storage error");
    assert!(matches!(
        replayed,
        AcceptJobOutcome::Replayed(job) if job.id == "racing-replay-job"
    ));

    let conflict = accept_while_uncommitted_row_exists(
        &storage,
        "racing-conflict-job",
        new_job("owner-a", "attempt-2", "hash-a"),
        new_job("owner-a", "attempt-2", "hash-b"),
    )
    .await
    .expect("racing conflict should not return storage error");
    assert_eq!(
        conflict,
        AcceptJobOutcome::Conflict(oracy_backend::storage::SubmissionConflict {
            job_id: "racing-conflict-job".to_owned()
        })
    );

    assert_eq!(job_count_by_key(&storage, "owner-a", "attempt-1").await, 1);
    assert_eq!(job_count_by_key(&storage, "owner-a", "attempt-2").await, 1);
}

#[tokio::test]
async fn accepted_submission_tuple_is_immutable_in_storage() {
    let (_tempdir, storage) = storage().await;
    let created = match storage
        .accept_job(new_job("owner-a", "attempt-1", "hash-a"))
        .await
        .expect("accept job")
    {
        AcceptJobOutcome::Created(job) => job,
        other => panic!("expected created job, got {other:?}"),
    };

    let error = sqlx::query(
        r#"
        UPDATE transcription_jobs
        SET recorded_at = '2026-04-25T00:00:00Z'
        WHERE id = ?
        "#,
    )
    .bind(&created.id)
    .execute(storage.pool())
    .await
    .expect_err("accepted tuple update should fail");
    assert!(
        error
            .to_string()
            .contains("accepted submission tuple is immutable")
    );

    let unchanged = storage
        .get_job("owner-a", &created.id)
        .await
        .expect("lookup")
        .expect("job exists");
    assert_eq!(unchanged.recorded_at, created.recorded_at);
}

#[tokio::test]
async fn transcript_materialization_is_transactional() {
    let (_tempdir, storage) = storage().await;
    let job = created_job(&storage, "owner-a", "attempt-1").await;
    mark_job_processing(&storage, &job.id).await;
    let mut materialization = materialization("transcript-a");
    materialization.segments[1].position = materialization.segments[0].position;

    storage
        .complete_job_with_transcript("owner-a", &job.id, materialization)
        .await
        .expect_err("duplicate segment position should fail");

    assert!(
        storage
            .get_transcript("owner-a", "transcript-a")
            .await
            .expect("transcript lookup")
            .is_none()
    );
    let job = storage
        .get_job("owner-a", &job.id)
        .await
        .expect("job lookup")
        .expect("job exists");
    assert_eq!(job.status, "processing");
    assert_eq!(job.transcript_id, None);
}

#[tokio::test]
async fn completed_transcripts_expose_current_version_ordered_segments_and_current_embedding() {
    let (_tempdir, storage) = storage().await;
    let job = created_job(&storage, "owner-a", "attempt-1").await;
    mark_job_processing(&storage, &job.id).await;
    storage
        .complete_job_with_transcript("owner-a", &job.id, materialization("transcript-a"))
        .await
        .expect("materialize transcript");

    sqlx::query(
        r#"
        INSERT INTO transcript_versions (
            id, api_key_id, transcript_id, version_number, transcript, created_at
        )
        VALUES ('version-2', 'owner-a', 'transcript-a', 2, 'edited text', '2026-04-24T18:01:00Z')
        "#,
    )
    .execute(storage.pool())
    .await
    .expect("insert edited version");

    let transcript = storage
        .get_transcript("owner-a", "transcript-a")
        .await
        .expect("transcript lookup")
        .expect("transcript exists");
    assert_eq!(transcript.current_version_id, "version-2");
    assert_eq!(transcript.transcript, "edited text");

    let segments = storage
        .list_segments("owner-a", "transcript-a")
        .await
        .expect("segments");
    assert_eq!(
        segments
            .iter()
            .map(|segment| (segment.position, segment.text.as_str()))
            .collect::<Vec<_>>(),
        vec![(0, "first segment"), (1, "second segment")]
    );

    let initial_embedding = storage
        .get_current_embedding("owner-a", "transcript-a")
        .await
        .expect("embedding lookup")
        .expect("embedding exists");
    assert_eq!(initial_embedding.vector, vec![1, 2, 3]);

    assert!(
        storage
            .replace_current_embedding(
                "owner-a",
                "transcript-a",
                NewEmbedding {
                    model: "embedding-v2".to_owned(),
                    vector: vec![4, 5, 6],
                    created_at: datetime!(2026-04-24 18:02:00 UTC),
                },
            )
            .await
            .expect("replace embedding")
    );
    assert!(
        !storage
            .replace_current_embedding(
                "owner-b",
                "transcript-a",
                NewEmbedding {
                    model: "wrong-owner".to_owned(),
                    vector: vec![9],
                    created_at: datetime!(2026-04-24 18:03:00 UTC),
                },
            )
            .await
            .expect("wrong-owner replacement is ignored")
    );

    let replaced = storage
        .get_current_embedding("owner-a", "transcript-a")
        .await
        .expect("embedding lookup")
        .expect("embedding exists");
    assert_eq!(replaced.model, "embedding-v2");
    assert_eq!(replaced.vector, vec![4, 5, 6]);
}

#[tokio::test]
async fn duplicate_completion_fails_without_orphaning_materialized_rows() {
    let (_tempdir, storage) = storage().await;
    let job = created_job(&storage, "owner-a", "attempt-1").await;
    mark_job_processing(&storage, &job.id).await;

    storage
        .complete_job_with_transcript("owner-a", &job.id, materialization("transcript-a"))
        .await
        .expect("first materialization succeeds");

    let error = storage
        .complete_job_with_transcript("owner-a", &job.id, materialization("transcript-b"))
        .await
        .expect_err("duplicate materialization should fail");
    assert!(matches!(
        error,
        StorageError::JobNotCompletable { job_id } if job_id == job.id
    ));

    let job = storage
        .get_job("owner-a", &job.id)
        .await
        .expect("job lookup")
        .expect("job exists");
    assert_eq!(job.status, "succeeded");
    assert_eq!(job.transcript_id.as_deref(), Some("transcript-a"));
    assert_eq!(row_count(&storage, "transcripts", "transcript-b").await, 0);
    assert_eq!(
        child_row_count(&storage, "transcript_versions", "transcript-b").await,
        0
    );
    assert_eq!(
        child_row_count(&storage, "segments", "transcript-b").await,
        0
    );
    assert_eq!(
        child_row_count(&storage, "embeddings", "transcript-b").await,
        0
    );
}

#[tokio::test]
async fn deleting_a_transcript_cascades_children_and_nulls_the_succeeded_job() {
    let (_tempdir, storage) = storage().await;
    let job = created_job(&storage, "owner-a", "attempt-1").await;
    mark_job_processing(&storage, &job.id).await;
    storage
        .complete_job_with_transcript("owner-a", &job.id, materialization("transcript-a"))
        .await
        .expect("materialize transcript");

    assert!(
        storage
            .delete_transcript("owner-a", "transcript-a")
            .await
            .expect("delete transcript")
    );
    assert!(
        storage
            .get_transcript("owner-a", "transcript-a")
            .await
            .expect("transcript lookup")
            .is_none()
    );
    assert!(
        storage
            .list_segments("owner-a", "transcript-a")
            .await
            .expect("segments")
            .is_empty()
    );
    assert!(
        storage
            .get_current_embedding("owner-a", "transcript-a")
            .await
            .expect("embedding lookup")
            .is_none()
    );

    let job = storage
        .get_job("owner-a", &job.id)
        .await
        .expect("job lookup")
        .expect("job survives");
    assert_eq!(job.status, "succeeded");
    assert_eq!(job.transcript_id, None);
}

async fn storage() -> (TempDir, Storage) {
    let tempdir = TempDir::new().expect("tempdir");
    let database_path = tempdir.path().join("oracy.sqlite");
    let storage = Storage::connect(&database_path)
        .await
        .expect("connect storage");
    (tempdir, storage)
}

async fn created_job(
    storage: &Storage,
    owner: &str,
    idempotency_key: &str,
) -> oracy_backend::storage::TranscriptionJobRecord {
    match storage
        .accept_job(new_job(owner, idempotency_key, "hash-a"))
        .await
        .expect("accept job")
    {
        AcceptJobOutcome::Created(job) => job,
        other => panic!("expected created job, got {other:?}"),
    }
}

async fn accept_while_uncommitted_row_exists(
    storage: &Storage,
    existing_job_id: &str,
    stored: NewTranscriptionJob,
    attempted: NewTranscriptionJob,
) -> Result<AcceptJobOutcome, oracy_backend::storage::StorageError> {
    let mut tx = storage.pool().begin().await.expect("begin transaction");
    sqlx::query(
        r#"
        INSERT INTO transcription_jobs (
            id, api_key_id, idempotency_key, audio_sha256_hex, recorded_at,
            session_id, language, accepted_audio_path, status, created_at,
            updated_at, retry_count, max_retries
        )
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, 'queued', ?, ?, 0, ?)
        "#,
    )
    .bind(existing_job_id)
    .bind(&stored.api_key_id)
    .bind(&stored.idempotency_key)
    .bind(&stored.audio_sha256_hex)
    .bind("2026-04-24T17:59:00Z")
    .bind(&stored.session_id)
    .bind(&stored.language)
    .bind(stored.accepted_audio_path.to_string_lossy().into_owned())
    .bind("2026-04-24T18:00:00Z")
    .bind("2026-04-24T18:00:00Z")
    .bind(stored.max_retries)
    .execute(&mut *tx)
    .await
    .expect("insert uncommitted accepted row");

    let racing_storage = storage.clone();
    let handle = tokio::spawn(async move { racing_storage.accept_job(attempted).await });
    sleep(Duration::from_millis(100)).await;
    tx.commit().await.expect("commit accepted row");
    handle.await.expect("accept task should not panic")
}

async fn mark_job_processing(storage: &Storage, job_id: &str) {
    let result = sqlx::query(
        r#"
        UPDATE transcription_jobs
        SET status = 'processing'
        WHERE api_key_id = 'owner-a' AND id = ?
        "#,
    )
    .bind(job_id)
    .execute(storage.pool())
    .await
    .expect("mark job processing");
    assert_eq!(result.rows_affected(), 1);
}

async fn job_count_by_key(storage: &Storage, owner: &str, idempotency_key: &str) -> i64 {
    sqlx::query(
        r#"
        SELECT COUNT(*) AS count
        FROM transcription_jobs
        WHERE api_key_id = ? AND idempotency_key = ?
        "#,
    )
    .bind(owner)
    .bind(idempotency_key)
    .fetch_one(storage.pool())
    .await
    .expect("count jobs")
    .get("count")
}

async fn row_count(storage: &Storage, table: &str, id: &str) -> i64 {
    let sql = format!("SELECT COUNT(*) AS count FROM {table} WHERE id = ?");
    sqlx::query(&sql)
        .bind(id)
        .fetch_one(storage.pool())
        .await
        .expect("count rows")
        .get("count")
}

async fn child_row_count(storage: &Storage, table: &str, transcript_id: &str) -> i64 {
    let sql = format!("SELECT COUNT(*) AS count FROM {table} WHERE transcript_id = ?");
    sqlx::query(&sql)
        .bind(transcript_id)
        .fetch_one(storage.pool())
        .await
        .expect("count child rows")
        .get("count")
}

fn new_job(owner: &str, idempotency_key: &str, hash: &str) -> NewTranscriptionJob {
    NewTranscriptionJob {
        api_key_id: owner.to_owned(),
        idempotency_key: idempotency_key.to_owned(),
        audio_sha256_hex: hash.to_owned(),
        recorded_at: datetime!(2026-04-24 17:59:00 UTC),
        session_id: Some("session-a".to_owned()),
        language: Some("en".to_owned()),
        accepted_audio_path: PathBuf::from("/var/lib/oracy/accepted-audio/job-a"),
        max_retries: 3,
        now: datetime!(2026-04-24 18:00:00 UTC),
    }
}

fn materialization(transcript_id: &str) -> TranscriptMaterialization {
    TranscriptMaterialization {
        transcript: NewTranscript {
            id: transcript_id.to_owned(),
            audio_duration_seconds: 12.5,
            audio_format: "wav".to_owned(),
            audio_size_bytes: 401_280,
            transcript_language: Some("en".to_owned()),
            model: "general-transcription-v1".to_owned(),
            processing_time_ms: 1_843,
            cost_cents: Some(1),
            created_at: datetime!(2026-04-24 18:00:30 UTC),
            recorded_at: datetime!(2026-04-24 17:59:00 UTC),
            session_id: Some("session-a".to_owned()),
        },
        initial_version: NewTranscriptVersion {
            id: format!("{transcript_id}-version-1"),
            transcript: "initial text".to_owned(),
            created_at: datetime!(2026-04-24 18:00:30 UTC),
        },
        segments: vec![
            NewSegment {
                id: format!("{transcript_id}-segment-1"),
                position: 0,
                start_ms: 0,
                end_ms: 1_000,
                text: "first segment".to_owned(),
            },
            NewSegment {
                id: format!("{transcript_id}-segment-2"),
                position: 1,
                start_ms: 1_000,
                end_ms: 2_000,
                text: "second segment".to_owned(),
            },
        ],
        embedding: NewEmbedding {
            model: "embedding-v1".to_owned(),
            vector: vec![1, 2, 3],
            created_at: datetime!(2026-04-24 18:00:31 UTC),
        },
    }
}
