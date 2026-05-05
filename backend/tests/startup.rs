use std::fs;
#[cfg(unix)]
use std::io::Read;
#[cfg(unix)]
use std::net::{SocketAddr, TcpListener, TcpStream};
#[cfg(unix)]
use std::process::{Child, Command, ExitStatus, Stdio};
#[cfg(unix)]
use std::thread;
#[cfg(unix)]
use std::time::{Duration, Instant};

use oracy_backend::bootstrap::{BootstrapError, load_runtime_from_env, load_runtime_from_path};
use tempfile::TempDir;

mod support;

#[tokio::test]
async fn startup_rejects_missing_oracy_config_env() {
    support::assert_ignored_test_passes_with_env_missing(
        "startup_rejects_missing_oracy_config_env_helper",
        "ORACY_CONFIG",
    );
}

#[tokio::test]
#[ignore = "helper subprocess only"]
async fn startup_rejects_missing_oracy_config_env_helper() {
    let error = load_runtime_from_env()
        .await
        .expect_err("missing env should fail");
    assert!(matches!(error, BootstrapError::MissingConfigEnv));
}

#[tokio::test]
async fn startup_rejects_missing_openai_api_key_env() {
    let tempdir = TempDir::new().expect("tempdir");
    let config_path = write_config(
        &tempdir,
        format!(
            r#"
accepted_audio_dir = "{}"

[[api_keys]]
api_key_id = "alpha"
key = "key-one"
"#,
            tempdir.path().display()
        ),
    );

    support::assert_ignored_test_passes_with_env(
        "startup_rejects_missing_openai_api_key_env_helper",
        &[("ORACY_CONFIG", config_path.as_os_str())],
        &["OPENAI_API_KEY"],
    );
}

#[tokio::test]
#[ignore = "helper subprocess only"]
async fn startup_rejects_missing_openai_api_key_env_helper() {
    let error = load_runtime_from_env()
        .await
        .expect_err("missing OpenAI API key should fail");
    assert!(error.to_string().contains("OPENAI_API_KEY is not set"));
}

#[tokio::test]
async fn startup_rejects_empty_openai_api_key_env() {
    let tempdir = TempDir::new().expect("tempdir");
    let config_path = write_config(
        &tempdir,
        format!(
            r#"
accepted_audio_dir = "{}"

[[api_keys]]
api_key_id = "alpha"
key = "key-one"
"#,
            tempdir.path().display()
        ),
    );

    support::assert_ignored_test_passes_with_env(
        "startup_rejects_empty_openai_api_key_env_helper",
        &[
            ("ORACY_CONFIG", config_path.as_os_str()),
            ("OPENAI_API_KEY", std::ffi::OsStr::new("")),
        ],
        &[],
    );
}

#[tokio::test]
#[ignore = "helper subprocess only"]
async fn startup_rejects_empty_openai_api_key_env_helper() {
    let error = load_runtime_from_env()
        .await
        .expect_err("empty OpenAI API key should fail");
    assert!(
        error
            .to_string()
            .contains("OPENAI_API_KEY must not be empty")
    );
}

#[tokio::test]
async fn startup_accepts_non_empty_openai_api_key_env() {
    let tempdir = TempDir::new().expect("tempdir");
    let config_path = write_config(
        &tempdir,
        format!(
            r#"
accepted_audio_dir = "{}"

[[api_keys]]
api_key_id = "alpha"
key = "key-one"
"#,
            tempdir.path().display()
        ),
    );

    support::assert_ignored_test_passes_with_env(
        "startup_accepts_non_empty_openai_api_key_env_helper",
        &[
            ("ORACY_CONFIG", config_path.as_os_str()),
            ("OPENAI_API_KEY", std::ffi::OsStr::new("test-openai-key")),
        ],
        &[],
    );
}

#[tokio::test]
async fn startup_defaults_operator_listener_to_loopback_metrics_port() {
    let tempdir = TempDir::new().expect("tempdir");
    let config_path = write_config(
        &tempdir,
        format!(
            r#"
accepted_audio_dir = "{}"

[[api_keys]]
api_key_id = "alpha"
key = "key-one"
"#,
            tempdir.path().display()
        ),
    );

    let (_, state) = load_runtime_from_path(&config_path)
        .await
        .expect("valid runtime");

    assert_eq!(
        state.operator_listen_addr,
        "127.0.0.1:9090".parse().expect("valid socket address")
    );
}

#[tokio::test]
async fn startup_accepts_custom_operator_listener() {
    let tempdir = TempDir::new().expect("tempdir");
    let config_path = write_config(
        &tempdir,
        format!(
            r#"
operator_listen_addr = "127.0.0.1:9099"
accepted_audio_dir = "{}"

[[api_keys]]
api_key_id = "alpha"
key = "key-one"
"#,
            tempdir.path().display()
        ),
    );

    let (_, state) = load_runtime_from_path(&config_path)
        .await
        .expect("valid runtime");

    assert_eq!(
        state.operator_listen_addr,
        "127.0.0.1:9099".parse().expect("valid socket address")
    );
}

#[tokio::test]
async fn startup_rejects_operator_listener_that_collides_with_public_listener() {
    assert_listener_pair_rejected("127.0.0.1:8088", "127.0.0.1:8088").await;
}

#[tokio::test]
async fn startup_rejects_ipv4_wildcard_listener_that_overlaps_concrete_operator_listener() {
    assert_listener_pair_rejected("0.0.0.0:8080", "127.0.0.1:8080").await;
}

#[tokio::test]
async fn startup_rejects_ipv6_wildcard_listener_that_overlaps_concrete_operator_listener() {
    assert_listener_pair_rejected("[::]:8080", "[::1]:8080").await;
}

#[tokio::test]
async fn startup_rejects_ipv6_wildcard_listener_that_overlaps_ipv4_operator_listener() {
    assert_listener_pair_rejected("[::]:8080", "127.0.0.1:8080").await;
}

#[tokio::test]
async fn startup_rejects_ipv4_mapped_ipv6_listener_that_overlaps_ipv4_listener() {
    assert_listener_pair_rejected("[::ffff:127.0.0.1]:8080", "127.0.0.1:8080").await;
    assert_listener_pair_rejected("127.0.0.1:8080", "[::ffff:127.0.0.1]:8080").await;
}

#[tokio::test]
async fn startup_rejects_ipv4_mapped_ipv6_wildcard_listener_that_overlaps_ipv4_wildcard_listener() {
    assert_listener_pair_rejected("[::ffff:0.0.0.0]:8080", "0.0.0.0:8080").await;
}

#[tokio::test]
async fn startup_accepts_ipv4_wildcard_and_ipv6_concrete_listeners_on_same_port() {
    assert_listener_pair_accepted("0.0.0.0:8080", "[::1]:8080").await;
}

#[tokio::test]
async fn startup_accepts_ipv4_mapped_ipv6_and_ipv4_listeners_on_different_ports() {
    assert_listener_pair_accepted("[::ffff:127.0.0.1]:8080", "127.0.0.1:8081").await;
}

#[tokio::test]
async fn startup_accepts_ipv4_mapped_ipv6_and_ipv6_loopback_listeners_on_same_port() {
    assert_listener_pair_accepted("[::ffff:127.0.0.1]:8080", "[::1]:8080").await;
}

#[tokio::test]
async fn startup_accepts_loopback_listeners_on_different_ports() {
    assert_listener_pair_accepted("127.0.0.1:8080", "127.0.0.1:8081").await;
}

#[tokio::test]
async fn startup_accepts_wildcard_listeners_on_different_ports() {
    assert_listener_pair_accepted("0.0.0.0:8080", "0.0.0.0:8081").await;
}

async fn assert_listener_pair_rejected(listen_addr: &str, operator_listen_addr: &str) {
    let tempdir = TempDir::new().expect("tempdir");
    let config_path = write_listener_config(&tempdir, listen_addr, operator_listen_addr);

    let error = load_runtime_from_path(&config_path)
        .await
        .expect_err("overlapping public and operator listeners should fail");

    assert!(
        error
            .to_string()
            .contains("operator_listen_addr must not overlap listen_addr")
    );
}

async fn assert_listener_pair_accepted(listen_addr: &str, operator_listen_addr: &str) {
    let tempdir = TempDir::new().expect("tempdir");
    let config_path = write_listener_config(&tempdir, listen_addr, operator_listen_addr);

    load_runtime_from_path(&config_path)
        .await
        .expect("non-overlapping public and operator listeners should be accepted");
}

fn write_listener_config(
    tempdir: &TempDir,
    listen_addr: &str,
    operator_listen_addr: &str,
) -> std::path::PathBuf {
    write_config(
        tempdir,
        format!(
            r#"
listen_addr = "{listen_addr}"
operator_listen_addr = "{operator_listen_addr}"
accepted_audio_dir = "{}"

[[api_keys]]
api_key_id = "alpha"
key = "key-one"
"#,
            tempdir.path().display()
        ),
    )
}

#[tokio::test]
#[ignore = "helper subprocess only"]
async fn startup_accepts_non_empty_openai_api_key_env_helper() {
    load_runtime_from_env()
        .await
        .expect("non-empty OpenAI API key should allow startup");
}

#[tokio::test]
async fn startup_error_diagnostics_do_not_emit_openai_api_key_value() {
    let tempdir = TempDir::new().expect("tempdir");
    let config_path = write_config(
        &tempdir,
        r#"
accepted_audio_dir = "missing"

[[api_keys]]
api_key_id = "alpha"
key = "key-one"
"#
        .to_owned(),
    );

    support::assert_ignored_test_passes_with_env(
        "startup_error_diagnostics_do_not_emit_openai_api_key_value_helper",
        &[
            ("ORACY_CONFIG", config_path.as_os_str()),
            (
                "OPENAI_API_KEY",
                std::ffi::OsStr::new("sentinel-openai-key-value"),
            ),
        ],
        &[],
    );
}

#[tokio::test]
#[ignore = "helper subprocess only"]
async fn startup_error_diagnostics_do_not_emit_openai_api_key_value_helper() {
    let error = load_runtime_from_env()
        .await
        .expect_err("invalid runtime should fail");
    let diagnostic = error.to_string();

    assert!(!diagnostic.contains("sentinel-openai-key-value"));
}

#[tokio::test]
async fn startup_rejects_when_no_api_keys_are_configured() {
    let tempdir = TempDir::new().expect("tempdir");
    let config_path = write_config(
        &tempdir,
        format!(
            r#"
accepted_audio_dir = "{}"
api_keys = []
"#,
            tempdir.path().display()
        ),
    );

    let error = load_runtime_from_path(&config_path)
        .await
        .expect_err("empty key list should fail");
    assert!(
        error
            .to_string()
            .contains("at least one api_keys entry is required")
    );
}

#[tokio::test]
async fn startup_rejects_invalid_toml() {
    let tempdir = TempDir::new().expect("tempdir");
    let config_path = write_config(
        &tempdir,
        format!(
            r#"
accepted_audio_dir = "{}"
bogus = true

[[api_keys]]
api_key_id = "alpha"
key = "key-one"
"#,
            tempdir.path().display()
        ),
    );

    let error = load_runtime_from_path(&config_path)
        .await
        .expect_err("invalid toml should fail");
    assert!(error.to_string().contains("failed to parse config file"));
}

#[tokio::test]
async fn startup_rejects_duplicate_api_key_ids() {
    let tempdir = TempDir::new().expect("tempdir");
    let config_path = write_config(
        &tempdir,
        format!(
            r#"
accepted_audio_dir = "{}"

[[api_keys]]
api_key_id = "alpha"
key = "key-one"

[[api_keys]]
api_key_id = "alpha"
key = "key-two"
"#,
            tempdir.path().display()
        ),
    );

    let error = load_runtime_from_path(&config_path)
        .await
        .expect_err("duplicate ids should fail");
    assert!(error.to_string().contains("duplicate api_key_id"));
}

#[tokio::test]
async fn startup_rejects_blank_api_key_ids() {
    let tempdir = TempDir::new().expect("tempdir");
    let config_path = write_config(
        &tempdir,
        format!(
            r#"
accepted_audio_dir = "{}"

[[api_keys]]
api_key_id = "   "
key = "key-one"
"#,
            tempdir.path().display()
        ),
    );

    let error = load_runtime_from_path(&config_path)
        .await
        .expect_err("blank ids should fail");
    assert!(error.to_string().contains("api_key_id must not be blank"));
}

#[tokio::test]
async fn startup_rejects_api_key_id_with_surrounding_whitespace() {
    let tempdir = TempDir::new().expect("tempdir");
    let config_path = write_config(
        &tempdir,
        format!(
            r#"
accepted_audio_dir = "{}"

[[api_keys]]
api_key_id = "default "
key = "key-one"
"#,
            tempdir.path().display()
        ),
    );

    let error = load_runtime_from_path(&config_path)
        .await
        .expect_err("whitespace-padded ids should fail");
    match error {
        BootstrapError::InvalidConfiguration(message) => {
            assert!(message.contains("default "));
            assert!(message.contains("surrounding whitespace"));
        }
        other => panic!("expected invalid configuration error, got {other}"),
    }
}

#[tokio::test]
async fn startup_rejects_blank_api_key_material() {
    let tempdir = TempDir::new().expect("tempdir");
    let config_path = write_config(
        &tempdir,
        format!(
            r#"
accepted_audio_dir = "{}"

[[api_keys]]
api_key_id = "alpha"
key = "   "
"#,
            tempdir.path().display()
        ),
    );

    let error = load_runtime_from_path(&config_path)
        .await
        .expect_err("blank key material should fail");
    match error {
        BootstrapError::InvalidConfiguration(message) => {
            assert!(message.contains("alpha"));
            assert!(message.contains("must not be blank"));
            assert!(!message.contains("   "));
        }
        other => panic!("expected invalid configuration error, got {other}"),
    }
}

#[tokio::test]
async fn startup_rejects_api_key_with_surrounding_whitespace() {
    let tempdir = TempDir::new().expect("tempdir");
    let config_path = write_config(
        &tempdir,
        format!(
            r#"
accepted_audio_dir = "{}"

[[api_keys]]
api_key_id = "default"
key = "alpha-secret "
"#,
            tempdir.path().display()
        ),
    );

    let error = load_runtime_from_path(&config_path)
        .await
        .expect_err("whitespace-padded keys should fail");
    match error {
        BootstrapError::InvalidConfiguration(message) => {
            assert!(message.contains("default"));
            assert!(message.contains("surrounding whitespace"));
            assert!(!message.contains("alpha-secret "));
        }
        other => panic!("expected invalid configuration error, got {other}"),
    }
}

#[tokio::test]
async fn startup_rejects_api_key_with_non_ascii_bytes() {
    let tempdir = TempDir::new().expect("tempdir");
    let config_path = write_config(
        &tempdir,
        format!(
            r#"
accepted_audio_dir = "{}"

[[api_keys]]
api_key_id = "alpha"
key = "sëcret"
"#,
            tempdir.path().display()
        ),
    );

    let error = load_runtime_from_path(&config_path)
        .await
        .expect_err("non-ascii keys should fail");
    match error {
        BootstrapError::InvalidConfiguration(message) => {
            assert!(message.contains("alpha"));
            assert!(message.contains("visible ASCII"));
            assert!(!message.contains("sëcret"));
        }
        other => panic!("expected invalid configuration error, got {other}"),
    }
}

#[tokio::test]
async fn startup_rejects_api_key_with_control_characters() {
    let tempdir = TempDir::new().expect("tempdir");
    let config_path = write_config(
        &tempdir,
        format!(
            "accepted_audio_dir = \"{}\"\n\n[[api_keys]]\napi_key_id = \"alpha\"\nkey = \"bad\\u0001key\"\n",
            tempdir.path().display()
        ),
    );

    let error = load_runtime_from_path(&config_path)
        .await
        .expect_err("control-character keys should fail");
    match error {
        BootstrapError::InvalidConfiguration(message) => {
            assert!(message.contains("alpha"));
            assert!(message.contains("visible ASCII"));
            assert!(!message.contains("bad"));
        }
        other => panic!("expected invalid configuration error, got {other}"),
    }
}

#[tokio::test]
async fn startup_rejects_duplicate_api_key_material() {
    let tempdir = TempDir::new().expect("tempdir");
    let config_path = write_config(
        &tempdir,
        format!(
            r#"
accepted_audio_dir = "{}"

[[api_keys]]
api_key_id = "alpha"
key = "shared-key"

[[api_keys]]
api_key_id = "beta"
key = "shared-key"
"#,
            tempdir.path().display()
        ),
    );

    let error = load_runtime_from_path(&config_path)
        .await
        .expect_err("duplicate key material should fail");
    assert!(error.to_string().contains("duplicate api key material"));
}

#[tokio::test]
async fn startup_rejects_missing_accepted_audio_directory() {
    let tempdir = TempDir::new().expect("tempdir");
    let missing_dir = tempdir.path().join("missing");
    let config_path = write_config(
        &tempdir,
        format!(
            r#"
accepted_audio_dir = "{}"

[[api_keys]]
api_key_id = "alpha"
key = "key-one"
"#,
            missing_dir.display()
        ),
    );

    let error = load_runtime_from_path(&config_path)
        .await
        .expect_err("missing storage should fail");
    assert!(
        error
            .to_string()
            .contains("accepted audio directory does not exist")
    );
}

#[tokio::test]
async fn startup_rejects_missing_database_path() {
    let tempdir = TempDir::new().expect("tempdir");
    let config_path = write_config_without_database_path(
        &tempdir,
        format!(
            r#"
accepted_audio_dir = "{}"

[[api_keys]]
api_key_id = "alpha"
key = "key-one"
"#,
            tempdir.path().display()
        ),
    );

    let error = load_runtime_from_path(&config_path)
        .await
        .expect_err("missing database path should fail");
    assert!(error.to_string().contains("missing field `database_path`"));
}

#[tokio::test]
async fn startup_rejects_database_path_with_missing_parent() {
    let tempdir = TempDir::new().expect("tempdir");
    let missing_parent = tempdir.path().join("missing");
    let config_path = write_config(
        &tempdir,
        format!(
            r#"
accepted_audio_dir = "{}"
database_path = "{}"

[[api_keys]]
api_key_id = "alpha"
key = "key-one"
"#,
            tempdir.path().display(),
            missing_parent.join("oracy.sqlite").display()
        ),
    );

    let error = load_runtime_from_path(&config_path)
        .await
        .expect_err("missing database parent should fail");
    assert!(
        error
            .to_string()
            .contains("database parent directory does not exist")
    );
}

#[tokio::test]
async fn startup_rejects_database_path_that_is_a_directory() {
    let tempdir = TempDir::new().expect("tempdir");
    let database_dir = tempdir.path().join("oracy.sqlite");
    fs::create_dir(&database_dir).expect("create database dir");
    let config_path = write_config(
        &tempdir,
        format!(
            r#"
accepted_audio_dir = "{}"
database_path = "{}"

[[api_keys]]
api_key_id = "alpha"
key = "key-one"
"#,
            tempdir.path().display(),
            database_dir.display()
        ),
    );

    let error = load_runtime_from_path(&config_path)
        .await
        .expect_err("directory database path should fail");
    assert!(error.to_string().contains("database path is a directory"));
}

#[cfg(unix)]
#[tokio::test]
async fn startup_rejects_unwritable_database_parent_directory() {
    use std::os::unix::fs::PermissionsExt;

    if let Some(reason) = support::skip_reason_for_unwritable_directory_test() {
        eprintln!("{reason}");
        return;
    }

    let tempdir = TempDir::new().expect("tempdir");
    let database_parent = tempdir.path().join("database");
    fs::create_dir(&database_parent).expect("create database parent");
    let mut permissions = fs::metadata(&database_parent)
        .expect("metadata")
        .permissions();
    permissions.set_mode(0o500);
    fs::set_permissions(&database_parent, permissions).expect("chmod");
    let config_path = write_config(
        &tempdir,
        format!(
            r#"
accepted_audio_dir = "{}"
database_path = "{}"

[[api_keys]]
api_key_id = "alpha"
key = "key-one"
"#,
            tempdir.path().display(),
            database_parent.join("oracy.sqlite").display()
        ),
    );

    let error = load_runtime_from_path(&config_path)
        .await
        .expect_err("unwritable database parent should fail");
    assert!(
        error
            .to_string()
            .contains("database parent directory is not writable")
    );
}

#[tokio::test]
async fn startup_creates_database_file_and_runs_migrations() {
    let tempdir = TempDir::new().expect("tempdir");
    let database_path = tempdir.path().join("oracy.sqlite");
    let config_path = write_config(
        &tempdir,
        format!(
            r#"
accepted_audio_dir = "{}"
database_path = "{}"

[[api_keys]]
api_key_id = "alpha"
key = "key-one"
"#,
            tempdir.path().display(),
            database_path.display()
        ),
    );

    let (_, state) = load_runtime_from_path(&config_path)
        .await
        .expect("valid runtime");

    assert!(database_path.exists());
    let table_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'transcription_jobs'",
    )
    .fetch_one(state.storage.pool())
    .await
    .expect("query migrated schema");
    assert_eq!(table_count, 1);
}

#[tokio::test]
async fn startup_accepts_shipped_deployment_example_config_after_operator_paths_are_bound() {
    let tempdir = TempDir::new().expect("tempdir");
    let state_dir = tempdir.path().join("state");
    let accepted_audio_dir = state_dir.join("accepted-audio");
    fs::create_dir_all(&accepted_audio_dir).expect("create accepted audio dir");

    let example = fs::read_to_string("../deploy/examples/oracy.toml")
        .expect("read shipped deployment example config");
    let configured = example
        .replace("0.0.0.0:8080", "127.0.0.1:0")
        .replace("0.0.0.0:9090", "127.0.0.1:1")
        .replace("/var/lib/oracy", &state_dir.display().to_string())
        .replace("operator-issued-secret", "key-one");
    let config_path = tempdir.path().join("oracy.toml");
    fs::write(&config_path, configured).expect("write operator-bound config");

    load_runtime_from_path(&config_path)
        .await
        .expect("deployment example should load once operator paths exist");
}

#[cfg(unix)]
#[test]
fn backend_process_exits_zero_after_sigterm() {
    let tempdir = TempDir::new().expect("tempdir");
    let accepted_audio_dir = tempdir.path().join("accepted-audio");
    fs::create_dir(&accepted_audio_dir).expect("create accepted audio dir");
    let listen_port = unused_loopback_port();
    let operator_port = unused_loopback_port();
    assert_ne!(listen_port, operator_port);
    let config_path = write_config(
        &tempdir,
        format!(
            r#"
listen_addr = "127.0.0.1:{listen_port}"
operator_listen_addr = "127.0.0.1:{operator_port}"
accepted_audio_dir = "{}"

[[api_keys]]
api_key_id = "alpha"
key = "key-one"
"#,
            accepted_audio_dir.display()
        ),
    );

    let mut child = Command::new(env!("CARGO_BIN_EXE_oracy-backend"))
        .env("ORACY_CONFIG", &config_path)
        .env("OPENAI_API_KEY", "test-openai-key")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn backend");

    wait_for_tcp_listener(&mut child, listen_port);
    wait_for_tcp_listener(&mut child, operator_port);

    let signal_status = Command::new("kill")
        .arg("-TERM")
        .arg(child.id().to_string())
        .status()
        .expect("send SIGTERM");
    assert!(signal_status.success(), "kill -TERM should succeed");

    let status = wait_for_child_exit(&mut child, Duration::from_secs(5)).unwrap_or_else(|| {
        panic_with_child_output(&mut child, "backend did not exit after SIGTERM")
    });

    assert!(
        status.success(),
        "SIGTERM should trigger graceful shutdown with exit code 0, got {status}"
    );
}

#[tokio::test]
async fn startup_accepts_relative_accepted_audio_dir_resolved_against_config() {
    let tempdir = TempDir::new().expect("tempdir");
    let expected_dir = tempdir.path().join("accepted-audio");
    fs::create_dir(&expected_dir).expect("create accepted audio dir");
    let config_path = write_config(
        &tempdir,
        r#"
accepted_audio_dir = "accepted-audio"

[[api_keys]]
api_key_id = "alpha"
key = "key-one"
"#
        .to_owned(),
    );

    let (_, state) = load_runtime_from_path(&config_path)
        .await
        .expect("relative path should resolve");

    assert_eq!(state.accepted_audio_dir, expected_dir);
}

#[tokio::test]
async fn startup_rejects_relative_accepted_audio_dir_when_target_missing() {
    let tempdir = TempDir::new().expect("tempdir");
    let expected_dir = tempdir.path().join("accepted-audio");
    let config_path = write_config(
        &tempdir,
        r#"
accepted_audio_dir = "accepted-audio"

[[api_keys]]
api_key_id = "alpha"
key = "key-one"
"#
        .to_owned(),
    );

    let error = load_runtime_from_path(&config_path)
        .await
        .expect_err("missing relative path should fail");

    match error {
        BootstrapError::MissingAcceptedAudioDir(path) => assert_eq!(path, expected_dir),
        other => panic!("expected missing accepted audio dir error, got {other}"),
    }
}

#[cfg(unix)]
#[tokio::test]
async fn startup_resolves_relative_accepted_audio_dir_through_config_symlink() {
    use std::os::unix::fs::symlink;

    let tempdir = TempDir::new().expect("tempdir");
    let release_root = tempdir.path().join("releases").join("v1");
    let real_config_dir = release_root.join("config");
    let real_accepted_audio_dir = release_root.join("accepted-audio");
    fs::create_dir_all(&real_config_dir).expect("create real config dir");
    fs::create_dir(&real_accepted_audio_dir).expect("create accepted audio dir");

    let symlink_config_dir = tempdir.path().join("current");
    symlink(&real_config_dir, &symlink_config_dir).expect("create config dir symlink");

    let real_config_path = real_config_dir.join("oracy.toml");
    fs::write(
        &real_config_path,
        r#"
accepted_audio_dir = "../accepted-audio"
database_path = "../oracy.sqlite"

[[api_keys]]
api_key_id = "alpha"
key = "key-one"
"#
        .trim_start(),
    )
    .expect("write config");

    let symlinked_config_path = symlink_config_dir.join("oracy.toml");
    let (_, state) = load_runtime_from_path(&symlinked_config_path)
        .await
        .expect("relative path should resolve through symlink target");

    let expected_dir =
        fs::canonicalize(&symlink_config_dir).expect("canonicalize symlinked config dir");
    let expected_dir = expected_dir.join("../accepted-audio");

    assert_eq!(state.accepted_audio_dir, expected_dir);
    assert_eq!(
        fs::canonicalize(&state.accepted_audio_dir).expect("canonicalize resolved audio dir"),
        real_accepted_audio_dir
    );
}

#[cfg(unix)]
#[tokio::test]
async fn startup_resolves_relative_accepted_audio_dir_from_real_config_file() {
    use std::os::unix::fs::symlink;

    let tempdir = TempDir::new().expect("tempdir");
    let release_root = tempdir.path().join("releases").join("v1");
    let deploy_root = tempdir.path().join("deploy");
    let real_accepted_audio_dir = release_root.join("accepted-audio");
    let decoy_accepted_audio_dir = deploy_root.join("accepted-audio");
    fs::create_dir_all(&release_root).expect("create release dir");
    fs::create_dir_all(&deploy_root).expect("create deploy dir");
    fs::create_dir(&real_accepted_audio_dir).expect("create release accepted audio dir");
    fs::create_dir(&decoy_accepted_audio_dir).expect("create deploy accepted audio dir");

    let real_config_path = release_root.join("oracy.toml");
    fs::write(
        &real_config_path,
        r#"
accepted_audio_dir = "accepted-audio"
database_path = "oracy.sqlite"

[[api_keys]]
api_key_id = "alpha"
key = "key-one"
"#
        .trim_start(),
    )
    .expect("write real config");

    let symlinked_config_path = deploy_root.join("current.toml");
    symlink(&real_config_path, &symlinked_config_path).expect("create config file symlink");

    let (_, state) = load_runtime_from_path(&symlinked_config_path)
        .await
        .expect("relative path should resolve through real config file");

    assert_eq!(state.accepted_audio_dir, real_accepted_audio_dir);
    assert_ne!(state.accepted_audio_dir, decoy_accepted_audio_dir);
}

#[tokio::test]
async fn startup_rejects_when_accepted_audio_path_is_a_file() {
    let tempdir = TempDir::new().expect("tempdir");
    let file_path = tempdir.path().join("accepted-audio");
    fs::write(&file_path, "not a directory").expect("write file");
    let config_path = write_config(
        &tempdir,
        format!(
            r#"
accepted_audio_dir = "{}"

[[api_keys]]
api_key_id = "alpha"
key = "key-one"
"#,
            file_path.display()
        ),
    );

    let error = load_runtime_from_path(&config_path)
        .await
        .expect_err("file path should fail");
    assert!(
        error
            .to_string()
            .contains("accepted audio path is not a directory")
    );
}

#[cfg(unix)]
#[tokio::test]
async fn startup_rejects_unwritable_accepted_audio_directory() {
    use std::os::unix::fs::PermissionsExt;

    if let Some(reason) = support::skip_reason_for_unwritable_directory_test() {
        eprintln!("{reason}");
        return;
    }

    let tempdir = TempDir::new().expect("tempdir");
    let audio_dir = tempdir.path().join("accepted-audio");
    fs::create_dir(&audio_dir).expect("create dir");
    let mut permissions = fs::metadata(&audio_dir).expect("metadata").permissions();
    permissions.set_mode(0o500);
    fs::set_permissions(&audio_dir, permissions).expect("chmod");

    let config_path = write_config(
        &tempdir,
        format!(
            r#"
accepted_audio_dir = "{}"

[[api_keys]]
api_key_id = "alpha"
key = "key-one"
"#,
            audio_dir.display()
        ),
    );

    let error = load_runtime_from_path(&config_path)
        .await
        .expect_err("unwritable dir should fail");
    match error {
        BootstrapError::AcceptedAudioDirNotWritable { path, .. } => assert_eq!(path, audio_dir),
        other => panic!("expected unwritable accepted audio dir error, got {other}"),
    }
}

fn write_config(tempdir: &TempDir, contents: String) -> std::path::PathBuf {
    let path = tempdir.path().join("oracy.toml");
    let mut contents = contents.trim_start().to_owned();
    if !contents.contains("database_path") {
        let database_path = tempdir.path().join("oracy.sqlite");
        contents = contents.replacen(
            '\n',
            &format!("\ndatabase_path = \"{}\"\n", database_path.display()),
            1,
        );
    }
    fs::write(&path, contents).expect("write config");
    path
}

fn write_config_without_database_path(tempdir: &TempDir, contents: String) -> std::path::PathBuf {
    let path = tempdir.path().join("oracy.toml");
    fs::write(&path, contents.trim_start()).expect("write config");
    path
}

#[cfg(unix)]
fn unused_loopback_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .expect("bind unused port")
        .local_addr()
        .expect("local addr")
        .port()
}

#[cfg(unix)]
fn wait_for_tcp_listener(child: &mut Child, port: u16) {
    let deadline = Instant::now() + Duration::from_secs(5);
    let addr: SocketAddr = format!("127.0.0.1:{port}")
        .parse()
        .expect("loopback socket address");

    loop {
        if TcpStream::connect_timeout(&addr, Duration::from_millis(50)).is_ok() {
            return;
        }
        if let Some(status) = child.try_wait().expect("poll child") {
            panic_with_child_output(
                child,
                &format!("backend exited before listener {addr} became ready: {status}"),
            );
        }
        if Instant::now() >= deadline {
            panic_with_child_output(
                child,
                &format!("backend listener {addr} did not become ready"),
            );
        }
        thread::sleep(Duration::from_millis(25));
    }
}

#[cfg(unix)]
fn wait_for_child_exit(child: &mut Child, timeout: Duration) -> Option<ExitStatus> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait().expect("poll child") {
            return Some(status);
        }
        if Instant::now() >= deadline {
            return None;
        }
        thread::sleep(Duration::from_millis(25));
    }
}

#[cfg(unix)]
fn panic_with_child_output(child: &mut Child, message: &str) -> ! {
    let _ = child.kill();
    let _ = child.wait();
    let mut stdout = String::new();
    let mut stderr = String::new();
    if let Some(mut pipe) = child.stdout.take() {
        let _ = pipe.read_to_string(&mut stdout);
    }
    if let Some(mut pipe) = child.stderr.take() {
        let _ = pipe.read_to_string(&mut stderr);
    }
    panic!("{message}\nstdout:\n{stdout}\nstderr:\n{stderr}");
}
