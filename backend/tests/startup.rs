use std::fs;

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
