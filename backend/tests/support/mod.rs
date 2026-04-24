use std::process::Command;

#[cfg(unix)]
const CAP_DAC_OVERRIDE_MASK: u64 = 1 << 1;

pub fn assert_ignored_test_passes_with_env_missing(test_name: &str, env_name: &str) {
    let output = Command::new(std::env::current_exe().expect("resolve current test binary"))
        .arg("--ignored")
        .arg("--exact")
        .arg(test_name)
        .arg("--nocapture")
        .env(env_name, "/tmp/oracy-config-should-have-been-removed")
        .env_remove(env_name)
        .output()
        .expect("spawn helper test");

    assert!(
        output.status.success(),
        "helper test {test_name} failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[cfg(unix)]
pub fn skip_reason_for_unwritable_directory_test() -> Option<String> {
    match std::fs::read_to_string("/proc/self/status") {
        Ok(status) => {
            skip_reason_for_unwritable_directory_test_from_status(Some(&status)).map(str::to_owned)
        }
        Err(error) => Some(format!(
            "skipping unwritable-directory startup test: failed to read /proc/self/status: {error}"
        )),
    }
}

#[cfg(unix)]
fn skip_reason_for_unwritable_directory_test_from_status(
    status: Option<&str>,
) -> Option<&'static str> {
    let Some(status) = status else {
        return Some(
            "skipping unwritable-directory startup test: unable to determine privilege state",
        );
    };

    let Some(effective_uid) = status
        .lines()
        .find_map(|line| line.strip_prefix("Uid:"))
        .and_then(|line| line.split_whitespace().nth(1))
    else {
        return Some(
            "skipping unwritable-directory startup test: unable to determine privilege state",
        );
    };
    if effective_uid == "0" {
        return Some("skipping unwritable-directory startup test: effective uid is root");
    }

    let Some(cap_eff) = status
        .lines()
        .find_map(|line| line.strip_prefix("CapEff:"))
        .and_then(|line| u64::from_str_radix(line.trim(), 16).ok())
    else {
        return Some(
            "skipping unwritable-directory startup test: unable to determine privilege state",
        );
    };
    if (cap_eff & CAP_DAC_OVERRIDE_MASK) != 0 {
        return Some(
            "skipping unwritable-directory startup test: CAP_DAC_OVERRIDE bypasses mode-bit checks",
        );
    }

    None
}

#[cfg(all(test, unix))]
mod tests {
    use super::skip_reason_for_unwritable_directory_test_from_status;

    #[test]
    fn unwritable_directory_test_runs_unprivileged_without_cap_dac_override() {
        let status = "\
Name:\ttest\n\
Uid:\t1000\t1000\t1000\t1000\n\
CapEff:\t0000000000000000\n";

        assert_eq!(
            skip_reason_for_unwritable_directory_test_from_status(Some(status)),
            None
        );
    }

    #[test]
    fn unwritable_directory_test_skips_for_root() {
        let status = "\
Name:\ttest\n\
Uid:\t0\t0\t0\t0\n\
CapEff:\t0000000000000000\n";

        assert_eq!(
            skip_reason_for_unwritable_directory_test_from_status(Some(status)),
            Some("skipping unwritable-directory startup test: effective uid is root")
        );
    }

    #[test]
    fn unwritable_directory_test_skips_for_cap_dac_override() {
        let status = "\
Name:\ttest\n\
Uid:\t1000\t1000\t1000\t1000\n\
CapEff:\t0000000000000002\n";

        assert_eq!(
            skip_reason_for_unwritable_directory_test_from_status(Some(status)),
            Some(
                "skipping unwritable-directory startup test: CAP_DAC_OVERRIDE bypasses mode-bit checks"
            )
        );
    }

    #[test]
    fn unwritable_directory_test_skips_when_privilege_state_cannot_be_determined() {
        assert_eq!(
            skip_reason_for_unwritable_directory_test_from_status(None),
            Some("skipping unwritable-directory startup test: unable to determine privilege state")
        );
    }

    #[test]
    fn unwritable_directory_test_skips_when_status_text_is_malformed() {
        let status = "\
Name:\ttest\n\
Uid:\t1000\n\
CapEff:\tnot-hex\n";

        assert_eq!(
            skip_reason_for_unwritable_directory_test_from_status(Some(status)),
            Some("skipping unwritable-directory startup test: unable to determine privilege state")
        );
    }
}
