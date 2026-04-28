use std::path::PathBuf;
use std::sync::Arc;

use oracy_backend::storage::{
    AcceptJobOutcome, CreateTagOutcome, NewEmbedding, NewSegment, NewSession, NewTag,
    NewTranscript, NewTranscriptVersion, NewTranscriptionJob, RenameTagOutcome,
    ReplaceTranscriptTagsOutcome, Storage, StorageError, TranscriptMaterialization,
};
use sqlx::Row;
use tempfile::TempDir;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use time::macros::datetime;
use tokio::sync::Barrier;
use tokio::time::{Duration, sleep};

#[tokio::test]
async fn accepted_jobs_replay_by_owner_and_reject_tuple_mismatches() {
    let (_tempdir, storage) = storage().await;
    insert_session_row(&storage, "owner-a", "session-a").await;
    insert_session_row(&storage, "owner-b", "session-b").await;
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

    let mut owner_b_input = new_job("owner-b", "attempt-1", "hash-a");
    owner_b_input.session_id = Some("session-b".to_owned());
    let different_owner = match storage.accept_job(owner_b_input).await {
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
async fn new_jobs_reject_missing_session_at_acceptance() {
    let (_tempdir, storage) = storage().await;

    let outcome = storage
        .accept_job(new_job("owner-a", "attempt-missing-session", "hash-a"))
        .await
        .expect("session validation outcome");

    assert_eq!(outcome, AcceptJobOutcome::SessionNotFound);
    assert_eq!(
        job_count_by_key(&storage, "owner-a", "attempt-missing-session").await,
        0
    );
}

#[tokio::test]
async fn new_jobs_reject_other_owner_session_at_acceptance() {
    let (_tempdir, storage) = storage().await;
    insert_session_row(&storage, "owner-b", "session-a").await;

    let outcome = storage
        .accept_job(new_job("owner-a", "attempt-other-owner-session", "hash-a"))
        .await
        .expect("session validation outcome");

    assert_eq!(outcome, AcceptJobOutcome::SessionNotFound);
    assert_eq!(
        job_count_by_key(&storage, "owner-a", "attempt-other-owner-session").await,
        0
    );
}

#[tokio::test]
async fn raw_job_rows_must_reference_owner_scoped_sessions() {
    let (_tempdir, storage) = storage().await;
    insert_session_row(&storage, "owner-b", "session-a").await;

    let error = sqlx::query(
        r#"
        INSERT INTO transcription_jobs (
            id, api_key_id, idempotency_key, audio_sha256_hex, recorded_at,
            session_id, language, accepted_audio_path, status, created_at,
            updated_at, retry_count, max_retries
        )
        VALUES (
            'job-invalid-session', 'owner-a', 'attempt-invalid-session',
            'hash-a', '2026-04-24T17:59:00.000000000Z', 'session-a', 'en',
            '/var/lib/oracy/accepted-audio/job-invalid-session', 'queued',
            '2026-04-24T18:00:00.000000000Z',
            '2026-04-24T18:00:00.000000000Z', 0, 3
        )
        "#,
    )
    .execute(storage.pool())
    .await
    .expect_err("mismatched job session owner should fail");

    assert!(
        error
            .to_string()
            .contains("job session must belong to same owner")
    );
}

#[tokio::test]
async fn racing_acceptance_resolves_unique_conflicts_as_replay_or_submission_conflict() {
    let (_tempdir, storage) = storage().await;
    insert_session_row(&storage, "owner-a", "session-a").await;
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
    insert_session_row(&storage, "owner-a", "session-a").await;
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
async fn retry_waiting_jobs_are_not_eligible_for_transcript_completion() {
    let (_tempdir, storage) = storage().await;
    let job = created_job(&storage, "owner-a", "attempt-1").await;
    mark_job_retry_waiting(&storage, &job.id).await;

    let error = storage
        .complete_job_with_transcript("owner-a", &job.id, materialization("transcript-a"))
        .await
        .expect_err("retry-waiting completion should fail");
    assert!(matches!(
        error,
        StorageError::JobNotCompletable { job_id } if job_id == job.id
    ));

    let job = storage
        .get_job("owner-a", &job.id)
        .await
        .expect("job lookup")
        .expect("job exists");
    assert_eq!(job.status, "retry_waiting");
    assert_eq!(job.transcript_id, None);
    assert_eq!(row_count(&storage, "transcripts", "transcript-a").await, 0);
    assert_eq!(
        child_row_count(&storage, "transcript_versions", "transcript-a").await,
        0
    );
    assert_eq!(
        child_row_count(&storage, "segments", "transcript-a").await,
        0
    );
    assert_eq!(
        child_row_count(&storage, "embeddings", "transcript-a").await,
        0
    );
}

#[tokio::test]
async fn persisted_timestamps_order_and_filter_chronologically_under_sql_text_comparisons() {
    let (_tempdir, storage) = storage().await;
    insert_session_row(&storage, "owner-a", "session-a").await;
    let cases = [
        ("after-fraction", timestamp("2026-04-24T18:00:00.1Z")),
        ("whole-second", timestamp("2026-04-24T18:00:00Z")),
        (
            "before-fraction",
            timestamp("2026-04-24T17:59:59.999999999Z"),
        ),
        (
            "after-nanosecond",
            timestamp("2026-04-24T18:00:00.000000001Z"),
        ),
    ];

    for (idempotency_key, now) in cases {
        let mut input = new_job("owner-a", idempotency_key, idempotency_key);
        input.now = now;
        storage.accept_job(input).await.expect("accept job");
    }

    let ordered_keys: Vec<String> = sqlx::query_scalar(
        r#"
        SELECT idempotency_key
        FROM transcription_jobs
        WHERE api_key_id = 'owner-a'
        ORDER BY created_at ASC
        "#,
    )
    .fetch_all(storage.pool())
    .await
    .expect("order jobs by stored timestamp");
    assert_eq!(
        ordered_keys,
        vec![
            "before-fraction",
            "whole-second",
            "after-nanosecond",
            "after-fraction"
        ]
    );

    let range_keys: Vec<String> = sqlx::query_scalar(
        r#"
        SELECT idempotency_key
        FROM transcription_jobs
        WHERE api_key_id = 'owner-a'
            AND created_at BETWEEN ? AND ?
        ORDER BY created_at ASC
        "#,
    )
    .bind("2026-04-24T18:00:00.000000000Z")
    .bind("2026-04-24T18:00:00.100000000Z")
    .fetch_all(storage.pool())
    .await
    .expect("filter jobs by stored timestamp range");
    assert_eq!(
        range_keys,
        vec!["whole-second", "after-nanosecond", "after-fraction"]
    );

    let mut equivalent_whole = new_job("owner-a", "equivalent-whole", "equivalent-whole");
    equivalent_whole.now = timestamp("2026-04-24T18:01:00Z");
    storage
        .accept_job(equivalent_whole)
        .await
        .expect("accept whole-second job");

    let mut equivalent_fraction = new_job("owner-a", "equivalent-fraction", "equivalent-fraction");
    equivalent_fraction.now = timestamp("2026-04-24T18:01:00.000000000Z");
    storage
        .accept_job(equivalent_fraction)
        .await
        .expect("accept equivalent fractional job");

    let equivalent_values: Vec<String> = sqlx::query_scalar(
        r#"
        SELECT created_at
        FROM transcription_jobs
        WHERE api_key_id = 'owner-a'
            AND idempotency_key IN ('equivalent-whole', 'equivalent-fraction')
        ORDER BY idempotency_key ASC
        "#,
    )
    .fetch_all(storage.pool())
    .await
    .expect("read equivalent stored timestamps");
    assert_eq!(equivalent_values[0], equivalent_values[1]);
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

#[tokio::test]
async fn transcript_child_rows_must_match_the_parent_transcript_owner() {
    let (_tempdir, storage) = storage().await;
    insert_transcript_only(&storage, "owner-a", "transcript-a").await;

    sqlx::query(
        r#"
        INSERT INTO transcript_versions (
            id, api_key_id, transcript_id, version_number, transcript, created_at
        )
        VALUES (
            'owner-mismatched-version', 'owner-b', 'transcript-a', 1,
            'cross-owner version', '2026-04-24T18:00:30.000000000Z'
        )
        "#,
    )
    .execute(storage.pool())
    .await
    .expect_err("mismatched transcript version owner should fail");

    sqlx::query(
        r#"
        INSERT INTO segments (
            id, api_key_id, transcript_id, position, start_ms, end_ms, text
        )
        VALUES (
            'owner-mismatched-segment', 'owner-b', 'transcript-a', 0, 0, 1000,
            'cross-owner segment'
        )
        "#,
    )
    .execute(storage.pool())
    .await
    .expect_err("mismatched segment owner should fail");

    sqlx::query(
        r#"
        INSERT INTO embeddings (transcript_id, api_key_id, model, vector, created_at)
        VALUES (
            'transcript-a', 'owner-b', 'embedding-v1', x'010203',
            '2026-04-24T18:00:31.000000000Z'
        )
        "#,
    )
    .execute(storage.pool())
    .await
    .expect_err("mismatched embedding owner should fail");
}

#[tokio::test]
async fn completed_job_transcript_link_must_match_the_job_owner() {
    let (_tempdir, storage) = storage().await;
    insert_transcript_only(&storage, "owner-a", "transcript-a").await;
    let job = created_job(&storage, "owner-b", "attempt-1").await;

    sqlx::query(
        r#"
        UPDATE transcription_jobs
        SET transcript_id = 'transcript-a'
        WHERE id = ?
        "#,
    )
    .bind(&job.id)
    .execute(storage.pool())
    .await
    .expect_err("mismatched job transcript owner should fail");
}

#[tokio::test]
async fn tags_are_owner_scoped_case_insensitive_and_many_to_many_with_transcripts() {
    let (_tempdir, storage) = storage().await;
    insert_transcript_only(&storage, "owner-a", "transcript-a").await;
    insert_transcript_only(&storage, "owner-a", "transcript-b").await;
    insert_transcript_only(&storage, "owner-b", "transcript-c").await;

    let meeting = match storage
        .create_tag(new_tag("owner-a", "tag-meeting", "Meeting"))
        .await
        .expect("create tag")
    {
        CreateTagOutcome::Created(tag) => tag,
        other => panic!("expected created tag, got {other:?}"),
    };
    let replayed = match storage
        .create_tag(new_tag("owner-a", "tag-meeting-lower", "meeting"))
        .await
        .expect("reuse case-insensitive tag")
    {
        CreateTagOutcome::Existing(tag) => tag,
        other => panic!("expected existing tag, got {other:?}"),
    };
    assert_eq!(replayed.id, meeting.id);
    assert_eq!(replayed.name, "Meeting");

    let other_owner = match storage
        .create_tag(new_tag("owner-b", "tag-meeting-owner-b", "meeting"))
        .await
        .expect("create owner-isolated tag")
    {
        CreateTagOutcome::Created(tag) => tag,
        other => panic!("expected owner-isolated tag, got {other:?}"),
    };
    assert_ne!(other_owner.id, meeting.id);

    assert_eq!(
        storage
            .replace_transcript_tags("owner-a", "transcript-a", std::slice::from_ref(&meeting.id))
            .await
            .expect("tag transcript"),
        ReplaceTranscriptTagsOutcome::Replaced
    );
    assert_eq!(
        storage
            .replace_transcript_tags("owner-a", "transcript-b", std::slice::from_ref(&meeting.id))
            .await
            .expect("tag second transcript"),
        ReplaceTranscriptTagsOutcome::Replaced
    );
    assert_eq!(
        storage
            .replace_transcript_tags("owner-b", "transcript-c", std::slice::from_ref(&meeting.id))
            .await
            .expect("wrong-owner tag is rejected"),
        ReplaceTranscriptTagsOutcome::NotFound
    );

    assert_eq!(
        storage
            .list_transcript_tags("owner-a", "transcript-a")
            .await
            .expect("list transcript tags"),
        vec![meeting.clone()]
    );

    assert!(
        storage
            .delete_tag("owner-a", &meeting.id)
            .await
            .expect("delete tag")
    );
    assert!(
        storage
            .list_transcript_tags("owner-a", "transcript-a")
            .await
            .expect("tag associations removed")
            .is_empty()
    );
    assert_eq!(row_count(&storage, "transcripts", "transcript-a").await, 1);
    assert_eq!(row_count(&storage, "transcripts", "transcript-b").await, 1);
}

#[tokio::test]
async fn unicode_case_equivalent_tag_names_share_one_owner_scoped_identity() {
    let (_tempdir, storage) = storage().await;

    for (created_name, replayed_name, created_id, replayed_id) in [
        ("Straße", "STRASSE", "tag-strasse-a", "tag-strasse-b"),
        ("Teſt", "test", "tag-long-s-a", "tag-long-s-b"),
    ] {
        let created = match storage
            .create_tag(new_tag("owner-a", created_id, created_name))
            .await
            .expect("create unicode tag")
        {
            CreateTagOutcome::Created(tag) => tag,
            other => panic!("expected created tag, got {other:?}"),
        };

        let replayed = match storage
            .create_tag(new_tag("owner-a", replayed_id, replayed_name))
            .await
            .expect("reuse unicode case-equivalent tag")
        {
            CreateTagOutcome::Existing(tag) => tag,
            other => panic!("expected existing tag, got {other:?}"),
        };

        assert_eq!(replayed.id, created.id);
        assert_eq!(replayed.name, created_name);
    }
}

#[tokio::test]
async fn duplicate_transcript_tag_ids_are_rejected_without_mutating_prior_tags() {
    let (_tempdir, storage) = storage().await;
    insert_transcript_only(&storage, "owner-a", "transcript-a").await;
    let meeting = match storage
        .create_tag(new_tag("owner-a", "tag-meeting", "Meeting"))
        .await
        .expect("create meeting tag")
    {
        CreateTagOutcome::Created(tag) => tag,
        other => panic!("expected created tag, got {other:?}"),
    };
    let notes = match storage
        .create_tag(new_tag("owner-a", "tag-notes", "Notes"))
        .await
        .expect("create notes tag")
    {
        CreateTagOutcome::Created(tag) => tag,
        other => panic!("expected created tag, got {other:?}"),
    };
    assert_eq!(
        storage
            .replace_transcript_tags("owner-a", "transcript-a", std::slice::from_ref(&notes.id))
            .await
            .expect("set prior tag"),
        ReplaceTranscriptTagsOutcome::Replaced
    );

    let outcome = storage
        .replace_transcript_tags(
            "owner-a",
            "transcript-a",
            &[meeting.id.clone(), meeting.id.clone()],
        )
        .await
        .expect("duplicate-input outcome");

    assert_eq!(outcome, ReplaceTranscriptTagsOutcome::DuplicateTagIds);
    assert_eq!(
        storage
            .list_transcript_tags("owner-a", "transcript-a")
            .await
            .expect("list transcript tags"),
        vec![notes]
    );
}

#[tokio::test]
async fn transcript_tag_replacement_collapses_missing_transcript_and_tag_to_not_found() {
    let (_tempdir, storage) = storage().await;
    insert_transcript_only(&storage, "owner-a", "transcript-a").await;
    let meeting = match storage
        .create_tag(new_tag("owner-a", "tag-meeting", "Meeting"))
        .await
        .expect("create meeting tag")
    {
        CreateTagOutcome::Created(tag) => tag,
        other => panic!("expected created tag, got {other:?}"),
    };

    assert_eq!(
        storage
            .replace_transcript_tags("owner-a", "transcript-missing", &[meeting.id])
            .await
            .expect("missing transcript outcome"),
        ReplaceTranscriptTagsOutcome::NotFound
    );
    assert_eq!(
        storage
            .replace_transcript_tags("owner-a", "transcript-a", &["tag-missing".to_owned()])
            .await
            .expect("missing tag outcome"),
        ReplaceTranscriptTagsOutcome::NotFound
    );
}

#[tokio::test]
async fn complete_job_with_transcript_nulls_deleted_accepted_session_without_mutating_job_tuple() {
    let (_tempdir, storage) = storage().await;
    let input = new_job("owner-a", "attempt-deleted-session", "hash-a");
    let job = created_job(&storage, "owner-a", "attempt-deleted-session").await;
    mark_job_processing(&storage, &job.id).await;

    assert!(
        storage
            .delete_session("owner-a", "session-a")
            .await
            .expect("delete session")
    );

    storage
        .complete_job_with_transcript(
            "owner-a",
            &job.id,
            materialization("transcript-deleted-session"),
        )
        .await
        .expect("materialize transcript after session deletion");

    assert_eq!(
        storage
            .get_transcript("owner-a", "transcript-deleted-session")
            .await
            .expect("transcript lookup")
            .expect("transcript exists")
            .session_id,
        None
    );

    let completed_job = storage
        .get_job("owner-a", &job.id)
        .await
        .expect("job lookup")
        .expect("job exists");
    assert_eq!(completed_job.session_id.as_deref(), Some("session-a"));

    let replayed = match storage.accept_job(input).await.expect("replay job") {
        AcceptJobOutcome::Replayed(job) => job,
        other => panic!("expected replayed job, got {other:?}"),
    };
    assert_eq!(replayed.id, job.id);
    assert_eq!(replayed.session_id.as_deref(), Some("session-a"));
}

#[tokio::test]
async fn sessions_are_identities_that_null_transcripts_without_mutating_replay_tuples() {
    let (_tempdir, storage) = storage().await;
    let session = storage
        .create_session(new_session("owner-a", "session-a", "Planning"))
        .await
        .expect("create session");
    let same_name = storage
        .create_session(new_session("owner-a", "session-b", "Planning"))
        .await
        .expect("create same-name session");
    assert_ne!(same_name.id, session.id);
    assert_eq!(same_name.name, session.name);

    let input = new_job("owner-a", "attempt-session", "hash-session");
    let job = match storage.accept_job(input.clone()).await.expect("accept job") {
        AcceptJobOutcome::Created(job) => job,
        other => panic!("expected created job, got {other:?}"),
    };
    mark_job_processing(&storage, &job.id).await;
    storage
        .complete_job_with_transcript("owner-a", &job.id, materialization("transcript-session"))
        .await
        .expect("materialize transcript");

    assert_eq!(
        storage
            .get_transcript("owner-a", "transcript-session")
            .await
            .expect("transcript lookup")
            .expect("transcript exists")
            .session_id
            .as_deref(),
        Some("session-a")
    );

    assert!(
        storage
            .delete_session("owner-a", "session-a")
            .await
            .expect("delete session")
    );
    assert_eq!(
        storage
            .get_transcript("owner-a", "transcript-session")
            .await
            .expect("transcript lookup")
            .expect("transcript exists")
            .session_id,
        None
    );

    let job_after_delete = storage
        .get_job("owner-a", &job.id)
        .await
        .expect("job lookup")
        .expect("job exists");
    assert_eq!(job_after_delete.session_id.as_deref(), Some("session-a"));

    let replayed = match storage.accept_job(input).await.expect("replay job") {
        AcceptJobOutcome::Replayed(job) => job,
        other => panic!("expected replayed job, got {other:?}"),
    };
    assert_eq!(replayed.id, job.id);
    assert_eq!(replayed.session_id.as_deref(), Some("session-a"));
}

#[tokio::test]
async fn tag_renames_preserve_latest_spelling_and_reject_case_insensitive_collisions() {
    let (_tempdir, storage) = storage().await;
    insert_transcript_only(&storage, "owner-a", "transcript-a").await;
    let meeting = match storage
        .create_tag(new_tag("owner-a", "tag-meeting", "Meeting"))
        .await
        .expect("create meeting tag")
    {
        CreateTagOutcome::Created(tag) => tag,
        other => panic!("expected created tag, got {other:?}"),
    };
    let notes = match storage
        .create_tag(new_tag("owner-a", "tag-notes", "Notes"))
        .await
        .expect("create notes tag")
    {
        CreateTagOutcome::Created(tag) => tag,
        other => panic!("expected created tag, got {other:?}"),
    };
    storage
        .replace_transcript_tags("owner-a", "transcript-a", std::slice::from_ref(&meeting.id))
        .await
        .expect("tag transcript");

    let renamed = match storage
        .rename_tag("owner-a", &meeting.id, "MEETING")
        .await
        .expect("rename tag")
    {
        RenameTagOutcome::Renamed(tag) => tag,
        other => panic!("expected renamed tag, got {other:?}"),
    };
    assert_eq!(renamed.id, meeting.id);
    assert_eq!(renamed.name, "MEETING");
    assert_eq!(
        storage
            .list_transcript_tags("owner-a", "transcript-a")
            .await
            .expect("list transcript tags")
            .first()
            .expect("tag exists")
            .name,
        "MEETING"
    );

    assert_eq!(
        storage
            .rename_tag("owner-a", &meeting.id, "notes")
            .await
            .expect("conflicting rename"),
        RenameTagOutcome::Conflict
    );
    assert_eq!(
        storage
            .get_tag("owner-a", &notes.id)
            .await
            .expect("tag lookup")
            .expect("notes tag exists")
            .name,
        "Notes"
    );
}

#[tokio::test]
async fn unicode_case_equivalent_tag_rename_targets_conflict() {
    let (_tempdir, storage) = storage().await;
    let strasse = match storage
        .create_tag(new_tag("owner-a", "tag-strasse", "Straße"))
        .await
        .expect("create unicode tag")
    {
        CreateTagOutcome::Created(tag) => tag,
        other => panic!("expected created tag, got {other:?}"),
    };
    let notes = match storage
        .create_tag(new_tag("owner-a", "tag-notes", "Notes"))
        .await
        .expect("create notes tag")
    {
        CreateTagOutcome::Created(tag) => tag,
        other => panic!("expected created tag, got {other:?}"),
    };

    assert_eq!(
        storage
            .rename_tag("owner-a", &notes.id, "STRASSE")
            .await
            .expect("unicode conflicting rename"),
        RenameTagOutcome::Conflict
    );
    assert_eq!(
        storage
            .get_tag("owner-a", &strasse.id)
            .await
            .expect("tag lookup")
            .expect("unicode tag exists")
            .name,
        "Straße"
    );
}

#[tokio::test]
async fn concurrent_tag_renames_to_one_unused_case_folded_name_return_success_and_conflict() {
    let (_tempdir, storage) = storage().await;

    for iteration in 0..25 {
        let alpha = match storage
            .create_tag(new_tag(
                "owner-a",
                &format!("tag-alpha-{iteration}"),
                &format!("Alpha {iteration}"),
            ))
            .await
            .expect("create alpha tag")
        {
            CreateTagOutcome::Created(tag) => tag,
            other => panic!("expected created tag, got {other:?}"),
        };
        let beta = match storage
            .create_tag(new_tag(
                "owner-a",
                &format!("tag-beta-{iteration}"),
                &format!("Beta {iteration}"),
            ))
            .await
            .expect("create beta tag")
        {
            CreateTagOutcome::Created(tag) => tag,
            other => panic!("expected created tag, got {other:?}"),
        };

        let barrier = Arc::new(Barrier::new(3));
        let alpha_storage = storage.clone();
        let alpha_barrier = Arc::clone(&barrier);
        let alpha_id = alpha.id.clone();
        let alpha_handle = tokio::spawn(async move {
            alpha_barrier.wait().await;
            alpha_storage
                .rename_tag("owner-a", &alpha_id, &format!("Shared {iteration}"))
                .await
        });

        let beta_storage = storage.clone();
        let beta_barrier = Arc::clone(&barrier);
        let beta_id = beta.id.clone();
        let beta_handle = tokio::spawn(async move {
            beta_barrier.wait().await;
            beta_storage
                .rename_tag("owner-a", &beta_id, &format!("SHARED {iteration}"))
                .await
        });

        barrier.wait().await;
        let outcomes = [
            alpha_handle
                .await
                .expect("alpha rename task should not panic"),
            beta_handle
                .await
                .expect("beta rename task should not panic"),
        ];

        assert_eq!(
            outcomes
                .iter()
                .filter(|outcome| matches!(outcome, Ok(RenameTagOutcome::Renamed(_))))
                .count(),
            1
        );
        assert_eq!(
            outcomes
                .iter()
                .filter(|outcome| matches!(outcome, Ok(RenameTagOutcome::Conflict)))
                .count(),
            1
        );
    }
}

#[tokio::test]
async fn missing_tag_rename_returns_not_found_before_name_conflict() {
    let (_tempdir, storage) = storage().await;
    storage
        .create_tag(new_tag("owner-a", "tag-notes", "Notes"))
        .await
        .expect("create notes tag");

    let outcome = storage
        .rename_tag("owner-a", "tag-missing", "notes")
        .await
        .expect("rename missing tag");

    assert_eq!(outcome, RenameTagOutcome::NotFound);
}

#[tokio::test]
async fn other_owner_tag_rename_returns_not_found_before_name_conflict() {
    let (_tempdir, storage) = storage().await;
    storage
        .create_tag(new_tag("owner-a", "tag-notes", "Notes"))
        .await
        .expect("create notes tag");
    let other_owner = match storage
        .create_tag(new_tag("owner-b", "tag-other-owner", "Other"))
        .await
        .expect("create other-owner tag")
    {
        CreateTagOutcome::Created(tag) => tag,
        other => panic!("expected created tag, got {other:?}"),
    };

    let outcome = storage
        .rename_tag("owner-a", &other_owner.id, "notes")
        .await
        .expect("rename other-owner tag");

    assert_eq!(outcome, RenameTagOutcome::NotFound);
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
    insert_session_row(storage, owner, "session-a").await;
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

async fn insert_transcript_only(storage: &Storage, owner: &str, transcript_id: &str) {
    sqlx::query(
        r#"
        INSERT INTO transcripts (
            id, api_key_id, audio_duration_seconds, audio_format, audio_size_bytes,
            transcript_language, model, processing_time_ms, cost_cents,
            created_at, recorded_at, session_id
        )
        VALUES (
            ?, ?, 12.5, 'wav', 401280, 'en', 'general-transcription-v1', 1843, 1,
            '2026-04-24T18:00:30.000000000Z',
            '2026-04-24T17:59:00.000000000Z',
            NULL
        )
        "#,
    )
    .bind(transcript_id)
    .bind(owner)
    .execute(storage.pool())
    .await
    .expect("insert transcript");
}

async fn insert_session_row(storage: &Storage, owner: &str, session_id: &str) {
    sqlx::query(
        r#"
        INSERT OR IGNORE INTO sessions (id, api_key_id, name, created_at)
        VALUES (?, ?, 'Session A', '2026-04-24T17:58:00.000000000Z')
        "#,
    )
    .bind(session_id)
    .bind(owner)
    .execute(storage.pool())
    .await
    .expect("insert session");
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

async fn mark_job_retry_waiting(storage: &Storage, job_id: &str) {
    let result = sqlx::query(
        r#"
        UPDATE transcription_jobs
        SET status = 'retry_waiting',
            next_attempt_at = '2026-04-24T18:05:00Z'
        WHERE api_key_id = 'owner-a' AND id = ?
        "#,
    )
    .bind(job_id)
    .execute(storage.pool())
    .await
    .expect("mark job retry waiting");
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

fn new_tag(owner: &str, id: &str, name: &str) -> NewTag {
    NewTag {
        id: id.to_owned(),
        api_key_id: owner.to_owned(),
        name: name.to_owned(),
        created_at: datetime!(2026-04-24 18:00:45 UTC),
    }
}

fn new_session(owner: &str, id: &str, name: &str) -> NewSession {
    NewSession {
        id: id.to_owned(),
        api_key_id: owner.to_owned(),
        name: name.to_owned(),
        created_at: datetime!(2026-04-24 18:00:40 UTC),
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

fn timestamp(value: &str) -> OffsetDateTime {
    OffsetDateTime::parse(value, &Rfc3339).expect("valid RFC3339 timestamp")
}
