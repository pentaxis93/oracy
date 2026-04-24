use std::fs;

use oracy_backend::bootstrap::{BootstrapError, load_runtime_from_env, load_runtime_from_path};
use tempfile::TempDir;

#[test]
fn startup_rejects_missing_oracy_config_env() {
    unsafe {
        std::env::remove_var("ORACY_CONFIG");
    }

    let error = load_runtime_from_env().expect_err("missing env should fail");
    assert!(error.to_string().contains("ORACY_CONFIG is not set"));
}

#[test]
fn startup_rejects_when_no_api_keys_are_configured() {
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

    let error = load_runtime_from_path(&config_path).expect_err("empty key list should fail");
    assert!(
        error
            .to_string()
            .contains("at least one api_keys entry is required")
    );
}

#[test]
fn startup_rejects_invalid_toml() {
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

    let error = load_runtime_from_path(&config_path).expect_err("invalid toml should fail");
    assert!(error.to_string().contains("failed to parse config file"));
}

#[test]
fn startup_rejects_duplicate_api_key_ids() {
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

    let error = load_runtime_from_path(&config_path).expect_err("duplicate ids should fail");
    assert!(error.to_string().contains("duplicate api_key_id"));
}

#[test]
fn startup_rejects_blank_api_key_ids() {
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

    let error = load_runtime_from_path(&config_path).expect_err("blank ids should fail");
    assert!(error.to_string().contains("api_key_id must not be blank"));
}

#[test]
fn startup_rejects_api_key_id_with_surrounding_whitespace() {
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

    let error =
        load_runtime_from_path(&config_path).expect_err("whitespace-padded ids should fail");
    match error {
        BootstrapError::InvalidConfiguration(message) => {
            assert!(message.contains("default "));
            assert!(message.contains("surrounding whitespace"));
        }
        other => panic!("expected invalid configuration error, got {other}"),
    }
}

#[test]
fn startup_rejects_blank_api_key_material() {
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

    let error = load_runtime_from_path(&config_path).expect_err("blank key material should fail");
    match error {
        BootstrapError::InvalidConfiguration(message) => {
            assert!(message.contains("alpha"));
            assert!(message.contains("must not be blank"));
            assert!(!message.contains("   "));
        }
        other => panic!("expected invalid configuration error, got {other}"),
    }
}

#[test]
fn startup_rejects_api_key_with_surrounding_whitespace() {
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

    let error =
        load_runtime_from_path(&config_path).expect_err("whitespace-padded keys should fail");
    match error {
        BootstrapError::InvalidConfiguration(message) => {
            assert!(message.contains("default"));
            assert!(message.contains("surrounding whitespace"));
            assert!(!message.contains("alpha-secret "));
        }
        other => panic!("expected invalid configuration error, got {other}"),
    }
}

#[test]
fn startup_rejects_api_key_with_non_ascii_bytes() {
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

    let error = load_runtime_from_path(&config_path).expect_err("non-ascii keys should fail");
    match error {
        BootstrapError::InvalidConfiguration(message) => {
            assert!(message.contains("alpha"));
            assert!(message.contains("visible ASCII"));
            assert!(!message.contains("sëcret"));
        }
        other => panic!("expected invalid configuration error, got {other}"),
    }
}

#[test]
fn startup_rejects_api_key_with_control_characters() {
    let tempdir = TempDir::new().expect("tempdir");
    let config_path = write_config(
        &tempdir,
        format!(
            "accepted_audio_dir = \"{}\"\n\n[[api_keys]]\napi_key_id = \"alpha\"\nkey = \"bad\\u0001key\"\n",
            tempdir.path().display()
        ),
    );

    let error =
        load_runtime_from_path(&config_path).expect_err("control-character keys should fail");
    match error {
        BootstrapError::InvalidConfiguration(message) => {
            assert!(message.contains("alpha"));
            assert!(message.contains("visible ASCII"));
            assert!(!message.contains("bad"));
        }
        other => panic!("expected invalid configuration error, got {other}"),
    }
}

#[test]
fn startup_rejects_duplicate_api_key_material() {
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

    let error =
        load_runtime_from_path(&config_path).expect_err("duplicate key material should fail");
    assert!(error.to_string().contains("duplicate api key material"));
}

#[test]
fn startup_rejects_missing_accepted_audio_directory() {
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

    let error = load_runtime_from_path(&config_path).expect_err("missing storage should fail");
    assert!(
        error
            .to_string()
            .contains("accepted audio directory does not exist")
    );
}

#[test]
fn startup_accepts_relative_accepted_audio_dir_resolved_against_config() {
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

    let (_, state) = load_runtime_from_path(&config_path).expect("relative path should resolve");

    assert_eq!(state.accepted_audio_dir, expected_dir);
}

#[test]
fn startup_rejects_relative_accepted_audio_dir_when_target_missing() {
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

    let error =
        load_runtime_from_path(&config_path).expect_err("missing relative path should fail");

    match error {
        BootstrapError::MissingAcceptedAudioDir(path) => assert_eq!(path, expected_dir),
        other => panic!("expected missing accepted audio dir error, got {other}"),
    }
}

#[test]
fn startup_rejects_when_accepted_audio_path_is_a_file() {
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

    let error = load_runtime_from_path(&config_path).expect_err("file path should fail");
    assert!(
        error
            .to_string()
            .contains("accepted audio path is not a directory")
    );
}

#[cfg(unix)]
#[test]
fn startup_rejects_unwritable_accepted_audio_directory() {
    use std::os::unix::fs::PermissionsExt;

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

    let error = load_runtime_from_path(&config_path).expect_err("unwritable dir should fail");
    assert!(
        error
            .to_string()
            .contains("accepted audio directory is not writable")
    );
}

fn write_config(tempdir: &TempDir, contents: String) -> std::path::PathBuf {
    let path = tempdir.path().join("oracy.toml");
    fs::write(&path, contents.trim_start()).expect("write config");
    path
}
