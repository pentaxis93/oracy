use std::path::PathBuf;
use std::sync::Arc;

use oracy_backend::audio_hash::{AUDIO_CONTENT_HASH_ALGORITHM_ID, compose_audio_content_hash_hex};
use oracy_backend::storage::{
    AcceptJobOutcome, AcceptedChunk, CreateTagOutcome, FinalizeJobOutcome, NewEmbedding,
    NewOpenTranscriptionJob, NewSegment, NewSession, NewTag, NewTranscriptionJob, NewVoiceNote,
    NewVoiceNoteVersion, OpenJobOutcome, RenameTagOutcome, ReplaceVoiceNoteTagsOutcome,
    RetryOutcome, Storage, StorageError, StoreChunkOutcome, TransientJobFailure,
    UpdateVoiceNoteTextOutcome, VoiceNoteFilters, VoiceNoteMaterialization,
};
use sqlx::Row;
use std::time::Duration as StdDuration;
use tempfile::TempDir;
use time::Duration;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use time::macros::datetime;
use tokio::sync::Barrier;
use tokio::time::sleep;

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
async fn accepted_audio_hash_algorithm_survives_restart_with_rederived_hash() {
    let tempdir = TempDir::new().expect("tempdir");
    let database_path = tempdir.path().join("oracy.sqlite");
    let chunk_hashes = [
        "b18efa847f7a3fa48fe0aafd4a6250aa5129740e05126859377af20cedafdeee",
        "2725aea2d2dea736fbe38e41ecb518f6098cb68ac77362c5e34faafb356567c5",
        "b6f3c15844fe12f966ad90db59da8332a9a6d9dfd198ac83949be2045ec6dc1e",
    ];
    let composed_hash =
        compose_audio_content_hash_hex(chunk_hashes).expect("compose accepted audio hash");
    let accepted_job_id = {
        let storage = Storage::connect(&database_path)
            .await
            .expect("connect storage");
        insert_session_row(&storage, "owner-a", "session-a").await;
        let job = match storage
            .accept_job(new_job("owner-a", "attempt-1", &composed_hash))
            .await
            .expect("accept job")
        {
            AcceptJobOutcome::Created(job) => job,
            other => panic!("expected created job, got {other:?}"),
        };

        assert_eq!(
            job.audio_content_hash_algorithm,
            AUDIO_CONTENT_HASH_ALGORITHM_ID
        );
        job.id
    };

    let restarted = Storage::connect(&database_path)
        .await
        .expect("reconnect storage");
    let stored = restarted
        .get_job("owner-a", &accepted_job_id)
        .await
        .expect("read job")
        .expect("job survives restart");

    assert_eq!(stored.audio_sha256_hex, composed_hash);
    assert_eq!(
        stored.audio_content_hash_algorithm,
        AUDIO_CONTENT_HASH_ALGORITHM_ID
    );
    assert_eq!(
        compose_audio_content_hash_hex(chunk_hashes).expect("rederive accepted audio hash"),
        stored.audio_sha256_hex
    );
}

#[tokio::test]
async fn storage_owns_the_accepted_audio_hash_algorithm_pin() {
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

    assert_eq!(
        created.audio_content_hash_algorithm,
        AUDIO_CONTENT_HASH_ALGORITHM_ID
    );
}

#[tokio::test]
async fn keyword_search_resumes_equal_score_pages_by_created_at_and_id() {
    let (_tempdir, storage) = storage().await;
    for voice_note_id in ["voice-note-a", "voice-note-b", "voice-note-c"] {
        insert_searchable_voice_note(
            &storage,
            "owner-a",
            voice_note_id,
            &format!("version-{voice_note_id}"),
            "apollo exact",
            "2026-04-24T18:00:00.000000000Z",
        )
        .await;
    }

    let first_page = storage
        .search_voice_notes_keyword("owner-a", &VoiceNoteFilters::default(), "apollo", None, 2)
        .await
        .expect("first keyword page");
    assert_eq!(
        first_page
            .iter()
            .map(|row| row.record.id.as_str())
            .collect::<Vec<_>>(),
        ["voice-note-c", "voice-note-b"]
    );

    let cursor = first_page.last().expect("last result").cursor();
    let second_page = storage
        .search_voice_notes_keyword(
            "owner-a",
            &VoiceNoteFilters::default(),
            "apollo",
            Some(&cursor),
            2,
        )
        .await
        .expect("second keyword page");
    assert_eq!(
        second_page
            .iter()
            .map(|row| row.record.id.as_str())
            .collect::<Vec<_>>(),
        ["voice-note-a"]
    );
}

#[tokio::test]
async fn queued_jobs_are_claimed_by_a_processing_lease() {
    let (_tempdir, storage) = storage().await;
    let job = created_job(&storage, "owner-a", "attempt-1").await;

    let claimed = storage
        .claim_next_transcription_job(
            "worker-lease-a",
            datetime!(2026-04-24 18:00:05 UTC),
            datetime!(2026-04-24 18:05:05 UTC),
        )
        .await
        .expect("claim next job")
        .expect("queued job should be claimed");

    assert_eq!(claimed.id, job.id);
    assert_eq!(claimed.status, "processing");
    assert_eq!(
        claimed.processing_lease_token.as_deref(),
        Some("worker-lease-a")
    );
    assert_eq!(
        claimed.processing_lease_expires_at,
        Some(datetime!(2026-04-24 18:05:05 UTC))
    );

    assert!(
        storage
            .claim_next_transcription_job(
                "worker-lease-b",
                datetime!(2026-04-24 18:00:06 UTC),
                datetime!(2026-04-24 18:05:06 UTC),
            )
            .await
            .expect("second claim")
            .is_none()
    );
}

#[tokio::test]
async fn expired_processing_jobs_are_reclaimed_by_a_new_lease() {
    let (_tempdir, storage) = storage().await;
    let job = created_job(&storage, "owner-a", "attempt-1").await;
    storage
        .claim_next_transcription_job(
            "expired-lease",
            datetime!(2026-04-24 18:00:00 UTC),
            datetime!(2026-04-24 18:05:00 UTC),
        )
        .await
        .expect("initial claim");

    assert!(
        storage
            .claim_next_transcription_job(
                "too-early",
                datetime!(2026-04-24 18:04:59 UTC),
                datetime!(2026-04-24 18:09:59 UTC),
            )
            .await
            .expect("early reclaim")
            .is_none()
    );

    let reclaimed = storage
        .claim_next_transcription_job(
            "fresh-lease",
            datetime!(2026-04-24 18:05:01 UTC),
            datetime!(2026-04-24 18:10:01 UTC),
        )
        .await
        .expect("expired reclaim")
        .expect("expired processing job should be reclaimed");

    assert_eq!(reclaimed.id, job.id);
    assert_eq!(
        reclaimed.processing_lease_token.as_deref(),
        Some("fresh-lease")
    );
}

#[tokio::test]
async fn active_processing_lease_can_be_renewed_without_changing_visible_update_time() {
    let (_tempdir, storage) = storage().await;
    let job = created_job(&storage, "owner-a", "attempt-renew-lease").await;
    let claimed = storage
        .claim_next_transcription_job(
            "active-lease",
            datetime!(2026-04-24 18:00:00 UTC),
            datetime!(2026-04-24 18:05:00 UTC),
        )
        .await
        .expect("claim job")
        .expect("job should be claimed");

    let renewed = storage
        .renew_processing_lease(
            "owner-a",
            &job.id,
            "active-lease",
            datetime!(2026-04-24 18:01:00 UTC),
            datetime!(2026-04-24 18:06:00 UTC),
        )
        .await
        .expect("renew lease");

    assert!(renewed);
    let after_renewal = storage
        .get_job("owner-a", &job.id)
        .await
        .expect("job lookup")
        .expect("job exists");
    assert_eq!(
        after_renewal.processing_lease_expires_at,
        Some(datetime!(2026-04-24 18:06:00 UTC))
    );
    assert_eq!(after_renewal.updated_at, claimed.updated_at);

    let stale = storage
        .renew_processing_lease(
            "owner-a",
            &job.id,
            "stale-lease",
            datetime!(2026-04-24 18:02:00 UTC),
            datetime!(2026-04-24 18:07:00 UTC),
        )
        .await
        .expect("stale renewal");
    assert!(!stale);
}

#[tokio::test]
async fn chunked_jobs_are_claimed_by_finalize_order_not_open_order() {
    let (_tempdir, storage) = storage().await;
    let opened_first = opened_job_at(
        &storage,
        "owner-a",
        "attempt-opened-first",
        datetime!(2026-04-24 18:00:00 UTC),
    )
    .await;
    let opened_second = opened_job_at(
        &storage,
        "owner-a",
        "attempt-opened-second",
        datetime!(2026-04-24 18:01:00 UTC),
    )
    .await;
    store_chunk(&storage, &opened_first.id, "hash-first").await;
    store_chunk(&storage, &opened_second.id, "hash-second").await;

    finalize_open_job_at(
        &storage,
        &opened_second.id,
        "accepted-hash-second",
        datetime!(2026-04-24 18:02:00 UTC),
    )
    .await;
    finalize_open_job_at(
        &storage,
        &opened_first.id,
        "accepted-hash-first",
        datetime!(2026-04-24 18:03:00 UTC),
    )
    .await;

    let first_claim = storage
        .claim_next_transcription_job(
            "lease-first",
            datetime!(2026-04-24 18:04:00 UTC),
            datetime!(2026-04-24 18:09:00 UTC),
        )
        .await
        .expect("claim first ready job")
        .expect("first ready job should be claimed");
    let second_claim = storage
        .claim_next_transcription_job(
            "lease-second",
            datetime!(2026-04-24 18:04:01 UTC),
            datetime!(2026-04-24 18:09:01 UTC),
        )
        .await
        .expect("claim second ready job")
        .expect("second ready job should be claimed");

    assert_eq!(first_claim.id, opened_second.id);
    assert_eq!(second_claim.id, opened_first.id);
}

#[tokio::test]
async fn retry_waiting_jobs_are_claimed_only_after_next_attempt_at() {
    let (_tempdir, storage) = storage().await;
    let job = created_job(&storage, "owner-a", "attempt-1").await;
    mark_job_retry_waiting(&storage, &job.id).await;

    assert!(
        storage
            .claim_next_transcription_job(
                "early-lease",
                datetime!(2026-04-24 18:04:59 UTC),
                datetime!(2026-04-24 18:09:59 UTC),
            )
            .await
            .expect("early claim")
            .is_none()
    );

    let claimed = storage
        .claim_next_transcription_job(
            "due-lease",
            datetime!(2026-04-24 18:05:00 UTC),
            datetime!(2026-04-24 18:10:00 UTC),
        )
        .await
        .expect("due claim")
        .expect("due retry should be claimed");

    assert_eq!(claimed.id, job.id);
    assert_eq!(claimed.status, "processing");
    assert_eq!(claimed.next_attempt_at, None);
}

#[tokio::test]
async fn leased_completion_requires_the_active_token_and_current_embedding() {
    let (_tempdir, storage) = storage().await;
    let job = created_job(&storage, "owner-a", "attempt-1").await;
    storage
        .claim_next_transcription_job(
            "active-lease",
            datetime!(2026-04-24 18:00:00 UTC),
            datetime!(2026-04-24 18:05:00 UTC),
        )
        .await
        .expect("claim job");
    let materialization = materialization("voice-note-a");

    let error = storage
        .complete_leased_job_with_voice_note(
            "owner-a",
            &job.id,
            "stale-lease",
            materialization.clone(),
        )
        .await
        .expect_err("stale lease should not complete");
    assert!(matches!(
        error,
        StorageError::JobNotCompletable { job_id } if job_id == job.id
    ));

    let mut missing_embedding = materialization.clone();
    missing_embedding.embedding = None;
    let error = storage
        .complete_leased_job_with_voice_note("owner-a", &job.id, "active-lease", missing_embedding)
        .await
        .expect_err("missing embedding should not complete");
    assert!(matches!(
        error,
        StorageError::JobNotCompletable { job_id } if job_id == job.id
    ));

    storage
        .complete_leased_job_with_voice_note("owner-a", &job.id, "active-lease", materialization)
        .await
        .expect("active lease materializes voice note");

    let completed = storage
        .get_job("owner-a", &job.id)
        .await
        .expect("job lookup")
        .expect("job exists");
    assert_eq!(completed.status, "succeeded");
    assert_eq!(completed.voice_note_id.as_deref(), Some("voice-note-a"));
    assert!(
        storage
            .get_current_embedding("owner-a", "voice-note-a")
            .await
            .expect("embedding lookup")
            .is_some()
    );
}

#[tokio::test]
async fn transient_failures_retry_until_exhaustion_then_fail_terminally() {
    let (_tempdir, storage) = storage().await;
    let job = created_job(&storage, "owner-a", "attempt-1").await;
    storage
        .claim_next_transcription_job(
            "lease-1",
            datetime!(2026-04-24 18:00:00 UTC),
            datetime!(2026-04-24 18:05:00 UTC),
        )
        .await
        .expect("claim job");

    let first = storage
        .record_transient_job_failure(TransientJobFailure {
            api_key_id: "owner-a".to_owned(),
            job_id: job.id.clone(),
            lease_token: "lease-1".to_owned(),
            failure_code: "engine_error".to_owned(),
            failure_message: "engine temporarily failed".to_owned(),
            now: datetime!(2026-04-24 18:01:00 UTC),
            next_attempt_at: datetime!(2026-04-24 18:02:00 UTC),
        })
        .await
        .expect("record transient failure");
    let RetryOutcome::RetryWaiting(first) = first else {
        panic!("first transient failure should schedule retry");
    };
    assert_eq!(first.status, "retry_waiting");
    assert_eq!(first.retry_count, 1);
    assert_eq!(
        first.next_attempt_at,
        Some(datetime!(2026-04-24 18:02:00 UTC))
    );

    for (lease, now) in [
        ("lease-2", datetime!(2026-04-24 18:02:00 UTC)),
        ("lease-3", datetime!(2026-04-24 18:04:00 UTC)),
    ] {
        storage
            .claim_next_transcription_job(lease, now, now + Duration::seconds(300))
            .await
            .expect("claim retry")
            .expect("retry should be claimable");
        let outcome = storage
            .record_transient_job_failure(TransientJobFailure {
                api_key_id: "owner-a".to_owned(),
                job_id: job.id.clone(),
                lease_token: lease.to_owned(),
                failure_code: "engine_error".to_owned(),
                failure_message: "engine temporarily failed".to_owned(),
                now: now + Duration::seconds(30),
                next_attempt_at: now + Duration::seconds(60),
            })
            .await
            .expect("record retry failure");
        if lease == "lease-2" {
            assert!(matches!(outcome, RetryOutcome::RetryWaiting(_)));
        } else {
            let RetryOutcome::Failed(failed) = outcome else {
                panic!("third transient failure should exhaust retries");
            };
            assert_eq!(failed.status, "failed");
            assert_eq!(failed.failure_code.as_deref(), Some("engine_error"));
            assert_eq!(failed.retryable_by_client, Some(true));
        }
    }
}

#[tokio::test]
async fn successful_retry_clears_stale_failure_classification() {
    let (_tempdir, storage) = storage().await;
    let job = created_job(&storage, "owner-a", "attempt-1").await;
    storage
        .claim_next_transcription_job(
            "lease-1",
            datetime!(2026-04-24 18:00:00 UTC),
            datetime!(2026-04-24 18:05:00 UTC),
        )
        .await
        .expect("claim job");

    let first = storage
        .record_transient_job_failure(TransientJobFailure {
            api_key_id: "owner-a".to_owned(),
            job_id: job.id.clone(),
            lease_token: "lease-1".to_owned(),
            failure_code: "engine_error".to_owned(),
            failure_message: "engine temporarily failed".to_owned(),
            now: datetime!(2026-04-24 18:01:00 UTC),
            next_attempt_at: datetime!(2026-04-24 18:02:00 UTC),
        })
        .await
        .expect("record transient failure");
    assert!(matches!(first, RetryOutcome::RetryWaiting(_)));

    storage
        .claim_next_transcription_job(
            "lease-2",
            datetime!(2026-04-24 18:02:00 UTC),
            datetime!(2026-04-24 18:07:00 UTC),
        )
        .await
        .expect("claim retry")
        .expect("retry should be claimable");
    storage
        .complete_leased_job_with_voice_note(
            "owner-a",
            &job.id,
            "lease-2",
            materialization("voice-note-a"),
        )
        .await
        .expect("complete retry");

    let completed = storage
        .get_job("owner-a", &job.id)
        .await
        .expect("job lookup")
        .expect("job exists");
    assert_eq!(completed.status, "succeeded");
    assert_eq!(completed.retry_count, 1);
    assert_eq!(completed.failure_code, None);
    assert_eq!(completed.failure_message, None);
    assert_eq!(completed.retryable_by_client, None);
}

#[tokio::test]
async fn storage_rejects_restart_when_stored_audio_hash_algorithm_drifted() {
    let (tempdir, storage) = storage().await;
    insert_session_row(&storage, "owner-a", "session-a").await;
    match storage
        .accept_job(new_job("owner-a", "attempt-1", "hash-a"))
        .await
        .expect("accept job")
    {
        AcceptJobOutcome::Created(_) => {}
        other => panic!("expected created job, got {other:?}"),
    };

    sqlx::query("DROP TRIGGER transcription_jobs_accepted_tuple_immutable")
        .execute(storage.pool())
        .await
        .expect("drop immutability trigger for drift fixture");
    sqlx::query(
        r#"
        UPDATE transcription_jobs
        SET audio_content_hash_algorithm = 'sha256:chunk-sha256-raw-concat:v2'
        WHERE api_key_id = 'owner-a' AND idempotency_key = 'attempt-1'
        "#,
    )
    .execute(storage.pool())
    .await
    .expect("write drifted algorithm fixture");
    drop(storage);

    let error = Storage::connect(&tempdir.path().join("oracy.sqlite"))
        .await
        .expect_err("algorithm drift should reject startup");

    assert!(error.to_string().contains("audio content hash algorithm"));
    assert!(error.to_string().contains(AUDIO_CONTENT_HASH_ALGORITHM_ID));
    assert!(
        error
            .to_string()
            .contains("sha256:chunk-sha256-raw-concat:v2")
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
async fn racing_open_resolves_unique_conflicts_as_replay_or_submission_conflict() {
    let (_tempdir, storage) = storage().await;
    insert_session_row(&storage, "owner-a", "session-a").await;
    let input = new_open_job("owner-a", "attempt-open-1");

    let replayed = open_while_uncommitted_row_exists(
        &storage,
        "racing-open-replay-job",
        input.clone(),
        input.clone(),
    )
    .await
    .expect("racing open replay should not return storage error");
    assert!(matches!(
        replayed,
        OpenJobOutcome::ReplayedOpen(job) if job.id == "racing-open-replay-job"
    ));

    let mut mismatched = new_open_job("owner-a", "attempt-open-2");
    mismatched.chunk_count = 2;
    let conflict = open_while_uncommitted_row_exists(
        &storage,
        "racing-open-conflict-job",
        new_open_job("owner-a", "attempt-open-2"),
        mismatched,
    )
    .await
    .expect("racing open conflict should not return storage error");
    assert_eq!(
        conflict,
        OpenJobOutcome::Conflict(oracy_backend::storage::SubmissionConflict {
            job_id: "racing-open-conflict-job".to_owned()
        })
    );

    assert_eq!(
        job_count_by_key(&storage, "owner-a", "attempt-open-1").await,
        1
    );
    assert_eq!(
        job_count_by_key(&storage, "owner-a", "attempt-open-2").await,
        1
    );
}

#[tokio::test]
async fn abandonment_candidates_are_accepting_chunks_jobs_created_before_the_cutoff() {
    let (_tempdir, storage) = storage().await;
    let old_alpha = opened_job_at(
        &storage,
        "owner-a",
        "attempt-old-alpha",
        datetime!(2026-04-24 17:00:00 UTC),
    )
    .await;
    let queued_old = opened_job_at(
        &storage,
        "owner-a",
        "attempt-queued-old",
        datetime!(2026-04-24 17:15:00 UTC),
    )
    .await;
    mark_open_job_queued(&storage, &queued_old.id).await;
    let old_beta = opened_job_at(
        &storage,
        "owner-b",
        "attempt-old-beta",
        datetime!(2026-04-24 17:30:00 UTC),
    )
    .await;
    opened_job_at(
        &storage,
        "owner-a",
        "attempt-at-cutoff",
        datetime!(2026-04-24 18:00:00 UTC),
    )
    .await;
    opened_job_at(
        &storage,
        "owner-a",
        "attempt-recent",
        datetime!(2026-04-24 18:30:00 UTC),
    )
    .await;

    let candidates = storage
        .list_accepting_chunks_jobs_eligible_for_abandonment(datetime!(2026-04-24 18:00:00 UTC))
        .await
        .expect("abandonment candidates");

    assert_eq!(
        candidates
            .iter()
            .map(|job| (job.api_key_id.as_str(), job.id.as_str()))
            .collect::<Vec<_>>(),
        vec![
            ("owner-a", old_alpha.id.as_str()),
            ("owner-b", old_beta.id.as_str())
        ]
    );
}

#[tokio::test]
async fn accepting_chunks_jobs_can_be_abandoned_once_with_terminal_failure_metadata() {
    let (_tempdir, storage) = storage().await;
    let job = opened_job_at(
        &storage,
        "owner-a",
        "attempt-abandon-once",
        datetime!(2026-04-24 17:00:00 UTC),
    )
    .await;

    let abandoned = storage
        .abandon_accepting_chunks_job(
            "owner-a",
            &job.id,
            datetime!(2026-04-25 17:00:00 UTC),
            datetime!(2026-04-25 18:00:00 UTC),
        )
        .await
        .expect("abandon job")
        .expect("job abandoned");
    assert_eq!(abandoned.status, "failed");
    assert_eq!(
        abandoned.failure_code.as_deref(),
        Some("submission_abandoned")
    );
    assert_eq!(abandoned.retryable_by_client, Some(true));
    assert!(
        abandoned
            .failure_message
            .as_deref()
            .is_some_and(|message| { message.contains("abandonment window") })
    );
    assert_eq!(abandoned.updated_at, datetime!(2026-04-25 18:00:00 UTC));

    let duplicate = storage
        .abandon_accepting_chunks_job(
            "owner-a",
            &job.id,
            datetime!(2026-04-25 17:01:00 UTC),
            datetime!(2026-04-25 18:01:00 UTC),
        )
        .await
        .expect("duplicate abandon");
    assert_eq!(duplicate, None);
}

#[tokio::test]
async fn abandonment_transition_requires_created_at_before_cutoff() {
    let (_tempdir, storage) = storage().await;
    let recent = opened_job_at(
        &storage,
        "owner-a",
        "attempt-abandon-recent",
        datetime!(2026-04-25 17:30:00 UTC),
    )
    .await;

    let outcome = storage
        .abandon_accepting_chunks_job(
            "owner-a",
            &recent.id,
            datetime!(2026-04-25 17:00:00 UTC),
            datetime!(2026-04-25 18:00:00 UTC),
        )
        .await
        .expect("abandon recent job");

    assert_eq!(outcome, None);
    assert_eq!(
        storage
            .get_job("owner-a", &recent.id)
            .await
            .expect("get job")
            .expect("job exists")
            .status,
        "accepting_chunks"
    );
}

#[tokio::test]
async fn abandoned_jobs_remain_addressable_by_idempotency_replay() {
    let (_tempdir, storage) = storage().await;
    let job = opened_job_at(
        &storage,
        "owner-a",
        "attempt-abandoned-replay",
        datetime!(2026-04-24 17:00:00 UTC),
    )
    .await;
    storage
        .abandon_accepting_chunks_job(
            "owner-a",
            &job.id,
            datetime!(2026-04-25 17:00:00 UTC),
            datetime!(2026-04-25 18:00:00 UTC),
        )
        .await
        .expect("abandon job")
        .expect("job abandoned");

    let replay = storage
        .open_job(new_open_job("owner-a", "attempt-abandoned-replay"))
        .await
        .expect("replay open");

    assert!(matches!(
        replay,
        OpenJobOutcome::ReplayedFinalized(replayed)
            if replayed.id == job.id
                && replayed.status == "failed"
                && replayed.failure_code.as_deref() == Some("submission_abandoned")
    ));
}

#[tokio::test]
async fn racing_same_hash_chunk_push_resolves_as_idempotent_replay() {
    let (_tempdir, storage) = storage().await;
    let job = opened_job(&storage, "owner-a", "attempt-racing-same-chunk").await;
    let chunk = accepted_chunk(&job.id, "chunk-hash-a");

    let outcome = store_chunk_while_uncommitted_chunk_exists(&storage, chunk.clone(), chunk)
        .await
        .expect("racing same-hash chunk should not return storage error");

    assert_eq!(outcome, StoreChunkOutcome::Replayed);
    assert_eq!(chunk_count_by_job(&storage, "owner-a", &job.id).await, 1);
}

#[tokio::test]
async fn racing_different_hash_chunk_push_resolves_as_conflict() {
    let (_tempdir, storage) = storage().await;
    let job = opened_job(&storage, "owner-a", "attempt-racing-conflicting-chunk").await;

    let outcome = store_chunk_while_uncommitted_chunk_exists(
        &storage,
        accepted_chunk(&job.id, "chunk-hash-a"),
        accepted_chunk(&job.id, "chunk-hash-b"),
    )
    .await
    .expect("racing conflicting chunk should not return storage error");

    assert_eq!(outcome, StoreChunkOutcome::Conflict);
    assert_eq!(chunk_count_by_job(&storage, "owner-a", &job.id).await, 1);
}

#[tokio::test]
async fn racing_finalize_resolves_as_idempotent_replay() {
    let (_tempdir, storage) = storage().await;
    let job = opened_job(&storage, "owner-a", "attempt-racing-finalize").await;
    storage
        .store_chunk(accepted_chunk(&job.id, "chunk-hash-a"))
        .await
        .expect("store accepted chunk");

    let outcome =
        finalize_while_uncommitted_finalize_exists(&storage, &job.id, "accepted-audio-hash")
            .await
            .expect("racing finalize should not return storage error");

    assert!(matches!(
        outcome,
        FinalizeJobOutcome::Replayed(job) if job.status == "queued"
            && job.audio_sha256_hex == "accepted-audio-hash"
    ));
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
        SET audio_content_hash_algorithm = 'sha256:chunk-sha256-raw-concat:v2'
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
    assert_eq!(
        unchanged.audio_content_hash_algorithm,
        created.audio_content_hash_algorithm
    );
}

#[tokio::test]
async fn voice_note_materialization_is_transactional() {
    let (_tempdir, storage) = storage().await;
    let job = created_job(&storage, "owner-a", "attempt-1").await;
    mark_job_processing(&storage, &job.id).await;
    let mut materialization = materialization("voice-note-a");
    materialization.segments[1].position = materialization.segments[0].position;

    storage
        .complete_job_with_voice_note("owner-a", &job.id, materialization)
        .await
        .expect_err("duplicate segment position should fail");

    assert!(
        storage
            .get_voice_note("owner-a", "voice-note-a")
            .await
            .expect("voice note lookup")
            .is_none()
    );
    let job = storage
        .get_job("owner-a", &job.id)
        .await
        .expect("job lookup")
        .expect("job exists");
    assert_eq!(job.status, "processing");
    assert_eq!(job.voice_note_id, None);
}

#[tokio::test]
async fn completed_voice_notes_expose_current_version_ordered_segments_and_current_embedding() {
    let (_tempdir, storage) = storage().await;
    let job = created_job(&storage, "owner-a", "attempt-1").await;
    mark_job_processing(&storage, &job.id).await;
    storage
        .complete_job_with_voice_note("owner-a", &job.id, materialization("voice-note-a"))
        .await
        .expect("materialize voice note");

    sqlx::query(
        r#"
        INSERT INTO voice_note_versions (
            id, api_key_id, voice_note_id, version_number, text, created_at
        )
        VALUES ('version-2', 'owner-a', 'voice-note-a', 2, 'edited text', '2026-04-24T18:01:00Z')
        "#,
    )
    .execute(storage.pool())
    .await
    .expect("insert edited version");

    let voice_note = storage
        .get_voice_note("owner-a", "voice-note-a")
        .await
        .expect("voice note lookup")
        .expect("voice note exists");
    assert_eq!(voice_note.current_version_id, "version-2");
    assert_eq!(voice_note.text, "edited text");

    let segments = storage
        .list_segments("owner-a", "voice-note-a")
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
        .get_current_embedding("owner-a", "voice-note-a")
        .await
        .expect("embedding lookup")
        .expect("embedding exists");
    assert_eq!(initial_embedding.vector, vec![1, 2, 3]);

    assert!(
        storage
            .replace_current_embedding(
                "owner-a",
                "voice-note-a",
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
                "voice-note-a",
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
        .get_current_embedding("owner-a", "voice-note-a")
        .await
        .expect("embedding lookup")
        .expect("embedding exists");
    assert_eq!(replaced.model, "embedding-v2");
    assert_eq!(replaced.vector, vec![4, 5, 6]);
}

#[tokio::test]
async fn voice_note_text_replacement_appends_one_current_version_without_mutating_segments() {
    let (_tempdir, storage) = storage().await;
    let job = created_job(&storage, "owner-a", "attempt-1").await;
    mark_job_processing(&storage, &job.id).await;
    storage
        .complete_job_with_voice_note("owner-a", &job.id, materialization("voice-note-a"))
        .await
        .expect("materialize voice note");

    let outcome = storage
        .update_voice_note_text(
            "owner-a",
            "voice-note-a",
            "edited text",
            datetime!(2026-04-24 18:01:00 UTC),
        )
        .await
        .expect("replace voice note text");

    let UpdateVoiceNoteTextOutcome::Updated(updated) = outcome else {
        panic!("expected updated voice note, got {outcome:?}");
    };
    assert_eq!(updated.text, "edited text");
    assert_ne!(updated.current_version_id, "voice-note-a-version-1");

    let versions = storage
        .list_voice_note_versions("owner-a", "voice-note-a", None, 10)
        .await
        .expect("version history");
    assert_eq!(versions.len(), 2);
    assert_eq!(versions[0].id, updated.current_version_id);
    assert_eq!(versions[0].text, "edited text");
    assert_eq!(versions[1].id, "voice-note-a-version-1");
    assert_eq!(versions[1].text, "initial text");

    let segments = storage
        .list_segments("owner-a", "voice-note-a")
        .await
        .expect("segments");
    assert_eq!(
        segments
            .iter()
            .map(|segment| (segment.position, segment.text.as_str()))
            .collect::<Vec<_>>(),
        vec![(0, "first segment"), (1, "second segment")]
    );
}

#[tokio::test]
async fn duplicate_completion_fails_without_orphaning_materialized_rows() {
    let (_tempdir, storage) = storage().await;
    let job = created_job(&storage, "owner-a", "attempt-1").await;
    mark_job_processing(&storage, &job.id).await;

    storage
        .complete_job_with_voice_note("owner-a", &job.id, materialization("voice-note-a"))
        .await
        .expect("first materialization succeeds");

    let error = storage
        .complete_job_with_voice_note("owner-a", &job.id, materialization("voice-note-b"))
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
    assert_eq!(job.voice_note_id.as_deref(), Some("voice-note-a"));
    assert_eq!(row_count(&storage, "voice_notes", "voice-note-b").await, 0);
    assert_eq!(
        child_row_count(&storage, "voice_note_versions", "voice-note-b").await,
        0
    );
    assert_eq!(
        child_row_count(&storage, "segments", "voice-note-b").await,
        0
    );
    assert_eq!(
        child_row_count(&storage, "embeddings", "voice-note-b").await,
        0
    );
}

#[tokio::test]
async fn retry_waiting_jobs_are_not_eligible_for_voice_note_completion() {
    let (_tempdir, storage) = storage().await;
    let job = created_job(&storage, "owner-a", "attempt-1").await;
    mark_job_retry_waiting(&storage, &job.id).await;

    let error = storage
        .complete_job_with_voice_note("owner-a", &job.id, materialization("voice-note-a"))
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
    assert_eq!(job.voice_note_id, None);
    assert_eq!(row_count(&storage, "voice_notes", "voice-note-a").await, 0);
    assert_eq!(
        child_row_count(&storage, "voice_note_versions", "voice-note-a").await,
        0
    );
    assert_eq!(
        child_row_count(&storage, "segments", "voice-note-a").await,
        0
    );
    assert_eq!(
        child_row_count(&storage, "embeddings", "voice-note-a").await,
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
async fn deleting_a_voice_note_cascades_children_and_nulls_the_succeeded_job() {
    let (_tempdir, storage) = storage().await;
    let job = created_job(&storage, "owner-a", "attempt-1").await;
    mark_job_processing(&storage, &job.id).await;
    storage
        .complete_job_with_voice_note("owner-a", &job.id, materialization("voice-note-a"))
        .await
        .expect("materialize voice note");

    assert!(
        storage
            .delete_voice_note("owner-a", "voice-note-a")
            .await
            .expect("delete voice note")
    );
    assert!(
        storage
            .get_voice_note("owner-a", "voice-note-a")
            .await
            .expect("voice note lookup")
            .is_none()
    );
    assert!(
        storage
            .list_segments("owner-a", "voice-note-a")
            .await
            .expect("segments")
            .is_empty()
    );
    assert!(
        storage
            .get_current_embedding("owner-a", "voice-note-a")
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
    assert_eq!(job.voice_note_id, None);

    let replayed = match storage
        .accept_job(new_job("owner-a", "attempt-1", "hash-a"))
        .await
        .expect("replay deleted voice note job")
    {
        AcceptJobOutcome::Replayed(job) => job,
        other => panic!("expected replayed job, got {other:?}"),
    };
    assert_eq!(replayed.id, job.id);
    assert_eq!(replayed.status, "succeeded");
    assert_eq!(replayed.voice_note_id, None);
}

#[tokio::test]
async fn voice_note_child_rows_must_match_the_parent_voice_note_owner() {
    let (_tempdir, storage) = storage().await;
    insert_voice_note_only(&storage, "owner-a", "voice-note-a").await;

    sqlx::query(
        r#"
        INSERT INTO voice_note_versions (
            id, api_key_id, voice_note_id, version_number, text, created_at
        )
        VALUES (
            'owner-mismatched-version', 'owner-b', 'voice-note-a', 1,
            'cross-owner version', '2026-04-24T18:00:30.000000000Z'
        )
        "#,
    )
    .execute(storage.pool())
    .await
    .expect_err("mismatched voice note version owner should fail");

    sqlx::query(
        r#"
        INSERT INTO segments (
            id, api_key_id, voice_note_id, position, start_ms, end_ms, text
        )
        VALUES (
            'owner-mismatched-segment', 'owner-b', 'voice-note-a', 0, 0, 1000,
            'cross-owner segment'
        )
        "#,
    )
    .execute(storage.pool())
    .await
    .expect_err("mismatched segment owner should fail");

    sqlx::query(
        r#"
        INSERT INTO embeddings (voice_note_id, api_key_id, model, vector, created_at)
        VALUES (
            'voice-note-a', 'owner-b', 'embedding-v1', x'010203',
            '2026-04-24T18:00:31.000000000Z'
        )
        "#,
    )
    .execute(storage.pool())
    .await
    .expect_err("mismatched embedding owner should fail");
}

#[tokio::test]
async fn completed_job_voice_note_link_must_match_the_job_owner() {
    let (_tempdir, storage) = storage().await;
    insert_voice_note_only(&storage, "owner-a", "voice-note-a").await;
    let job = created_job(&storage, "owner-b", "attempt-1").await;

    sqlx::query(
        r#"
        UPDATE transcription_jobs
        SET voice_note_id = 'voice-note-a'
        WHERE id = ?
        "#,
    )
    .bind(&job.id)
    .execute(storage.pool())
    .await
    .expect_err("mismatched job voice note owner should fail");
}

#[tokio::test]
async fn tags_are_owner_scoped_case_insensitive_latest_spelling_and_many_to_many_with_voice_notes()
{
    let (_tempdir, storage) = storage().await;
    insert_voice_note_only(&storage, "owner-a", "voice-note-a").await;
    insert_voice_note_only(&storage, "owner-a", "voice-note-b").await;
    insert_voice_note_only(&storage, "owner-b", "voice-note-c").await;

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
    assert_eq!(replayed.name, "meeting");

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
            .replace_voice_note_tags("owner-a", "voice-note-a", std::slice::from_ref(&meeting.id))
            .await
            .expect("tag voice note"),
        ReplaceVoiceNoteTagsOutcome::Replaced
    );
    assert_eq!(
        storage
            .replace_voice_note_tags("owner-a", "voice-note-b", std::slice::from_ref(&meeting.id))
            .await
            .expect("tag second voice note"),
        ReplaceVoiceNoteTagsOutcome::Replaced
    );
    assert_eq!(
        storage
            .replace_voice_note_tags("owner-b", "voice-note-c", std::slice::from_ref(&meeting.id))
            .await
            .expect("wrong-owner tag is rejected"),
        ReplaceVoiceNoteTagsOutcome::NotFound
    );

    assert_eq!(
        storage
            .list_voice_note_tags("owner-a", "voice-note-a")
            .await
            .expect("list voice note tags"),
        vec![replayed.clone()]
    );

    assert!(
        storage
            .delete_tag("owner-a", &meeting.id)
            .await
            .expect("delete tag")
    );
    assert!(
        storage
            .list_voice_note_tags("owner-a", "voice-note-a")
            .await
            .expect("tag associations removed")
            .is_empty()
    );
    assert_eq!(row_count(&storage, "voice_notes", "voice-note-a").await, 1);
    assert_eq!(row_count(&storage, "voice_notes", "voice-note-b").await, 1);
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
        assert_eq!(replayed.name, replayed_name);
    }
}

#[tokio::test]
async fn duplicate_voice_note_tag_ids_are_rejected_without_mutating_prior_tags() {
    let (_tempdir, storage) = storage().await;
    insert_voice_note_only(&storage, "owner-a", "voice-note-a").await;
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
            .replace_voice_note_tags("owner-a", "voice-note-a", std::slice::from_ref(&notes.id))
            .await
            .expect("set prior tag"),
        ReplaceVoiceNoteTagsOutcome::Replaced
    );

    let outcome = storage
        .replace_voice_note_tags(
            "owner-a",
            "voice-note-a",
            &[meeting.id.clone(), meeting.id.clone()],
        )
        .await
        .expect("duplicate-input outcome");

    assert_eq!(outcome, ReplaceVoiceNoteTagsOutcome::DuplicateTagIds);
    assert_eq!(
        storage
            .list_voice_note_tags("owner-a", "voice-note-a")
            .await
            .expect("list voice note tags"),
        vec![notes]
    );
}

#[tokio::test]
async fn voice_note_tag_replacement_collapses_missing_voice_note_and_tag_to_not_found() {
    let (_tempdir, storage) = storage().await;
    insert_voice_note_only(&storage, "owner-a", "voice-note-a").await;
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
            .replace_voice_note_tags("owner-a", "voice-note-missing", &[meeting.id])
            .await
            .expect("missing voice note outcome"),
        ReplaceVoiceNoteTagsOutcome::NotFound
    );
    assert_eq!(
        storage
            .replace_voice_note_tags("owner-a", "voice-note-a", &["tag-missing".to_owned()])
            .await
            .expect("missing tag outcome"),
        ReplaceVoiceNoteTagsOutcome::NotFound
    );
}

#[tokio::test]
async fn complete_job_with_voice_note_nulls_deleted_accepted_session_without_mutating_job_tuple() {
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
        .complete_job_with_voice_note(
            "owner-a",
            &job.id,
            materialization("voice-note-deleted-session"),
        )
        .await
        .expect("materialize voice note after session deletion");

    assert_eq!(
        storage
            .get_voice_note("owner-a", "voice-note-deleted-session")
            .await
            .expect("voice note lookup")
            .expect("voice note exists")
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
async fn sessions_are_identities_that_null_voice_notes_without_mutating_replay_tuples() {
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
        .complete_job_with_voice_note("owner-a", &job.id, materialization("voice-note-session"))
        .await
        .expect("materialize voice note");

    assert_eq!(
        storage
            .get_voice_note("owner-a", "voice-note-session")
            .await
            .expect("voice note lookup")
            .expect("voice note exists")
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
            .get_voice_note("owner-a", "voice-note-session")
            .await
            .expect("voice note lookup")
            .expect("voice note exists")
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
    insert_voice_note_only(&storage, "owner-a", "voice-note-a").await;
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
        .replace_voice_note_tags("owner-a", "voice-note-a", std::slice::from_ref(&meeting.id))
        .await
        .expect("tag voice note");

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
            .list_voice_note_tags("owner-a", "voice-note-a")
            .await
            .expect("list voice note tags")
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

async fn opened_job(
    storage: &Storage,
    owner: &str,
    idempotency_key: &str,
) -> oracy_backend::storage::TranscriptionJobRecord {
    opened_job_at(
        storage,
        owner,
        idempotency_key,
        datetime!(2026-04-24 18:00:00 UTC),
    )
    .await
}

async fn opened_job_at(
    storage: &Storage,
    owner: &str,
    idempotency_key: &str,
    now: OffsetDateTime,
) -> oracy_backend::storage::TranscriptionJobRecord {
    let session_id = if owner == "owner-a" {
        "session-a".to_owned()
    } else {
        format!("{owner}-session-a")
    };
    insert_session_row(storage, owner, &session_id).await;
    let mut input = new_open_job(owner, idempotency_key);
    input.session_id = Some(session_id);
    input.now = now;
    match storage.open_job(input).await.expect("open job") {
        OpenJobOutcome::Created(job) => job,
        other => panic!("expected opened job, got {other:?}"),
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
    sleep(StdDuration::from_millis(100)).await;
    tx.commit().await.expect("commit accepted row");
    handle.await.expect("accept task should not panic")
}

async fn open_while_uncommitted_row_exists(
    storage: &Storage,
    existing_job_id: &str,
    stored: NewOpenTranscriptionJob,
    attempted: NewOpenTranscriptionJob,
) -> Result<OpenJobOutcome, oracy_backend::storage::StorageError> {
    let mut tx = storage.pool().begin().await.expect("begin transaction");
    sqlx::query(
        r#"
        INSERT INTO transcription_jobs (
            id, api_key_id, idempotency_key, recorded_at, session_id,
            language, status, created_at, updated_at, retry_count,
            max_retries, chunk_count, audio_format
        )
        VALUES (?, ?, ?, ?, ?, ?, 'accepting_chunks', ?, ?, 0, ?, ?, ?)
        "#,
    )
    .bind(existing_job_id)
    .bind(&stored.api_key_id)
    .bind(&stored.idempotency_key)
    .bind("2026-04-24T17:59:00Z")
    .bind(&stored.session_id)
    .bind(&stored.language)
    .bind("2026-04-24T18:00:00Z")
    .bind("2026-04-24T18:00:00Z")
    .bind(stored.max_retries)
    .bind(stored.chunk_count)
    .bind(&stored.audio_format)
    .execute(&mut *tx)
    .await
    .expect("insert uncommitted open row");

    let racing_storage = storage.clone();
    let handle = tokio::spawn(async move { racing_storage.open_job(attempted).await });
    sleep(StdDuration::from_millis(100)).await;
    tx.commit().await.expect("commit open row");
    handle.await.expect("open task should not panic")
}

async fn store_chunk_while_uncommitted_chunk_exists(
    storage: &Storage,
    stored: AcceptedChunk,
    attempted: AcceptedChunk,
) -> Result<StoreChunkOutcome, oracy_backend::storage::StorageError> {
    let mut tx = storage.pool().begin().await.expect("begin transaction");
    insert_chunk(&mut tx, &stored).await;

    let racing_storage = storage.clone();
    let handle = tokio::spawn(async move { racing_storage.store_chunk(attempted).await });
    sleep(StdDuration::from_millis(100)).await;
    assert!(
        !handle.is_finished(),
        "racing chunk push should wait on the held write"
    );
    tx.commit().await.expect("commit accepted chunk row");
    handle.await.expect("store chunk task should not panic")
}

async fn finalize_while_uncommitted_finalize_exists(
    storage: &Storage,
    job_id: &str,
    audio_sha256_hex: &str,
) -> Result<FinalizeJobOutcome, oracy_backend::storage::StorageError> {
    let mut tx = storage.pool().begin().await.expect("begin transaction");
    sqlx::query(
        r#"
        UPDATE transcription_jobs
        SET audio_sha256_hex = ?,
            accepted_audio_path = ?,
            transcription_model = ?,
            status = 'queued',
            updated_at = ?
        WHERE api_key_id = 'owner-a' AND id = ?
        "#,
    )
    .bind(audio_sha256_hex)
    .bind("/var/lib/oracy/accepted-audio/racing-finalized.wav")
    .bind("gpt-4o-mini-transcribe")
    .bind("2026-04-24T18:00:05Z")
    .bind(job_id)
    .execute(&mut *tx)
    .await
    .expect("update uncommitted finalized row");

    let racing_storage = storage.clone();
    let job_id = job_id.to_owned();
    let audio_sha256_hex = audio_sha256_hex.to_owned();
    let handle = tokio::spawn(async move {
        racing_storage
            .finalize_job(
                "owner-a",
                &job_id,
                &audio_sha256_hex,
                std::path::Path::new("/var/lib/oracy/accepted-audio/racing-finalized.wav"),
                "gpt-4o-mini-transcribe",
                datetime!(2026-04-24 18:00:05 UTC),
            )
            .await
    });
    sleep(StdDuration::from_millis(100)).await;
    assert!(
        !handle.is_finished(),
        "racing finalize should wait on the held write"
    );
    tx.commit().await.expect("commit finalized row");
    handle.await.expect("finalize task should not panic")
}

async fn insert_chunk(tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>, chunk: &AcceptedChunk) {
    sqlx::query(
        r#"
        INSERT INTO transcription_job_chunks (
            api_key_id, job_id, chunk_index, chunk_sha256_hex,
            chunk_path, chunk_size_bytes, accepted_at
        )
        VALUES (?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&chunk.api_key_id)
    .bind(&chunk.job_id)
    .bind(chunk.chunk_index)
    .bind(&chunk.chunk_sha256_hex)
    .bind(chunk.chunk_path.to_string_lossy().into_owned())
    .bind(chunk.chunk_size_bytes)
    .bind("2026-04-24T18:00:01Z")
    .execute(&mut **tx)
    .await
    .expect("insert uncommitted chunk row");
}

async fn insert_voice_note_only(storage: &Storage, owner: &str, voice_note_id: &str) {
    sqlx::query(
        r#"
        INSERT INTO voice_notes (
            id, api_key_id, audio_duration_seconds, audio_format, audio_size_bytes,
            language, model, processing_time_ms, cost_cents,
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
    .bind(voice_note_id)
    .bind(owner)
    .execute(storage.pool())
    .await
    .expect("insert voice note");
}

async fn insert_searchable_voice_note(
    storage: &Storage,
    owner: &str,
    voice_note_id: &str,
    version_id: &str,
    text: &str,
    created_at: &str,
) {
    sqlx::query(
        r#"
        INSERT INTO voice_notes (
            id, api_key_id, audio_duration_seconds, audio_format, audio_size_bytes,
            language, model, processing_time_ms, cost_cents,
            created_at, recorded_at, session_id
        )
        VALUES (
            ?, ?, 12.5, 'wav', 401280, 'en', 'general-transcription-v1', 1843, 1,
            ?, '2026-04-24T17:59:00.000000000Z', NULL
        )
        "#,
    )
    .bind(voice_note_id)
    .bind(owner)
    .bind(created_at)
    .execute(storage.pool())
    .await
    .expect("insert searchable voice note");

    sqlx::query(
        r#"
        INSERT INTO voice_note_versions (
            id, api_key_id, voice_note_id, version_number, text, created_at
        )
        VALUES (?, ?, ?, 1, ?, ?)
        "#,
    )
    .bind(version_id)
    .bind(owner)
    .bind(voice_note_id)
    .bind(text)
    .bind(created_at)
    .execute(storage.pool())
    .await
    .expect("insert searchable voice note version");
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

async fn mark_open_job_queued(storage: &Storage, job_id: &str) {
    let result = sqlx::query(
        r#"
        UPDATE transcription_jobs
        SET audio_sha256_hex = 'queued-open-job-hash',
            accepted_audio_path = '/var/lib/oracy/accepted-audio/queued-open-job.wav',
            status = 'queued'
        WHERE api_key_id = 'owner-a' AND id = ?
        "#,
    )
    .bind(job_id)
    .execute(storage.pool())
    .await
    .expect("mark open job queued");
    assert_eq!(result.rows_affected(), 1);
}

async fn store_chunk(storage: &Storage, job_id: &str, hash: &str) {
    let outcome = storage
        .store_chunk(accepted_chunk(job_id, hash))
        .await
        .expect("store chunk");
    assert_eq!(outcome, StoreChunkOutcome::Stored);
}

async fn finalize_open_job_at(
    storage: &Storage,
    job_id: &str,
    audio_sha256_hex: &str,
    now: OffsetDateTime,
) {
    let outcome = storage
        .finalize_job(
            "owner-a",
            job_id,
            audio_sha256_hex,
            std::path::Path::new("/var/lib/oracy/accepted-audio/finalized.wav"),
            "gpt-4o-mini-transcribe",
            now,
        )
        .await
        .expect("finalize open job");
    assert!(matches!(outcome, FinalizeJobOutcome::Accepted(_)));
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

async fn chunk_count_by_job(storage: &Storage, owner: &str, job_id: &str) -> i64 {
    sqlx::query(
        r#"
        SELECT COUNT(*) AS count
        FROM transcription_job_chunks
        WHERE api_key_id = ? AND job_id = ?
        "#,
    )
    .bind(owner)
    .bind(job_id)
    .fetch_one(storage.pool())
    .await
    .expect("count chunks")
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

async fn child_row_count(storage: &Storage, table: &str, voice_note_id: &str) -> i64 {
    let sql = format!("SELECT COUNT(*) AS count FROM {table} WHERE voice_note_id = ?");
    sqlx::query(&sql)
        .bind(voice_note_id)
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

fn new_open_job(owner: &str, idempotency_key: &str) -> NewOpenTranscriptionJob {
    NewOpenTranscriptionJob {
        api_key_id: owner.to_owned(),
        idempotency_key: idempotency_key.to_owned(),
        recorded_at: datetime!(2026-04-24 17:59:00 UTC),
        session_id: Some("session-a".to_owned()),
        language: Some("en".to_owned()),
        chunk_count: 1,
        audio_format: "wav".to_owned(),
        max_retries: 3,
        now: datetime!(2026-04-24 18:00:00 UTC),
    }
}

fn accepted_chunk(job_id: &str, hash: &str) -> AcceptedChunk {
    AcceptedChunk {
        api_key_id: "owner-a".to_owned(),
        job_id: job_id.to_owned(),
        chunk_index: 0,
        chunk_sha256_hex: hash.to_owned(),
        chunk_path: PathBuf::from(format!(
            "/var/lib/oracy/accepted-audio/{job_id}/chunks/0.chunk"
        )),
        chunk_size_bytes: 12,
        accepted_at: datetime!(2026-04-24 18:00:01 UTC),
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

fn materialization(voice_note_id: &str) -> VoiceNoteMaterialization {
    VoiceNoteMaterialization {
        voice_note: NewVoiceNote {
            id: voice_note_id.to_owned(),
            audio_duration_seconds: 12.5,
            audio_format: "wav".to_owned(),
            audio_size_bytes: 401_280,
            language: Some("en".to_owned()),
            model: "general-transcription-v1".to_owned(),
            processing_time_ms: 1_843,
            cost_cents: Some(1),
            created_at: datetime!(2026-04-24 18:00:30 UTC),
            recorded_at: datetime!(2026-04-24 17:59:00 UTC),
        },
        initial_version: NewVoiceNoteVersion {
            id: format!("{voice_note_id}-version-1"),
            text: "initial text".to_owned(),
            created_at: datetime!(2026-04-24 18:00:30 UTC),
        },
        segments: vec![
            NewSegment {
                id: format!("{voice_note_id}-segment-1"),
                position: 0,
                start_ms: 0,
                end_ms: 1_000,
                text: "first segment".to_owned(),
            },
            NewSegment {
                id: format!("{voice_note_id}-segment-2"),
                position: 1,
                start_ms: 1_000,
                end_ms: 2_000,
                text: "second segment".to_owned(),
            },
        ],
        embedding: Some(NewEmbedding {
            model: "embedding-v1".to_owned(),
            vector: vec![1, 2, 3],
            created_at: datetime!(2026-04-24 18:00:31 UTC),
        }),
    }
}

fn timestamp(value: &str) -> OffsetDateTime {
    OffsetDateTime::parse(value, &Rfc3339).expect("valid RFC3339 timestamp")
}
