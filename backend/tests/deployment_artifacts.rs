#[test]
fn quadlet_template_publishes_operator_metrics_on_loopback_by_default() {
    let template = std::fs::read_to_string("../deploy/quadlet/oracy.container.in")
        .expect("read container Quadlet template");

    assert!(template.contains("PublishPort=@ORACY_PUBLIC_PUBLISH@"));
    assert!(template.contains("PublishPort=127.0.0.1:@ORACY_OPERATOR_HOST_PORT@:9090"));
    assert!(!template.contains("PublishPort=0.0.0.0:@ORACY_OPERATOR_HOST_PORT@:9090"));
}

#[test]
fn quadlet_template_mounts_host_bound_state_through_volume_template() {
    let container = std::fs::read_to_string("../deploy/quadlet/oracy.container.in")
        .expect("read container Quadlet template");
    let volume = std::fs::read_to_string("../deploy/quadlet/oracy-data.volume.in")
        .expect("read volume Quadlet template");

    assert!(container.contains("Volume=oracy-data.volume:/var/lib/oracy:rw"));
    assert!(volume.contains("Device=@ORACY_STATE_DIR@"));
    assert!(volume.contains("Type=none"));
    assert!(volume.contains("Options=bind"));
}

#[test]
fn quadlet_template_grants_podman_a_full_thirty_second_stop_window() {
    let template = std::fs::read_to_string("../deploy/quadlet/oracy.container.in")
        .expect("read container Quadlet template");

    let stop_timeout = quadlet_seconds(&template, "StopTimeout");
    let timeout_stop_sec = quadlet_seconds(&template, "TimeoutStopSec");

    assert_eq!(stop_timeout, 30);
    assert_eq!(timeout_stop_sec, 40);
    assert!(timeout_stop_sec > stop_timeout);
}

#[test]
fn deployment_readme_manages_the_quadlet_generated_service_unit() {
    let readme = std::fs::read_to_string("../deploy/README.md").expect("read deployment README");

    assert!(readme.contains("systemctl --user start oracy.service"));
    assert!(readme.contains("systemctl --user status oracy.service"));
    assert!(!readme.contains("systemctl --user start oracy.container"));
    assert!(!readme.contains("systemctl --user enable oracy.service"));
}

#[test]
fn deployment_readme_names_quadlet_rendered_filenames() {
    let readme = std::fs::read_to_string("../deploy/README.md").expect("read deployment README");

    assert!(readme.contains("oracy.container"));
    assert!(readme.contains("oracy-data.volume"));
}

fn quadlet_seconds(template: &str, key: &str) -> u64 {
    template
        .lines()
        .find_map(|line| line.strip_prefix(&format!("{key}=")))
        .unwrap_or_else(|| panic!("{key} should be declared"))
        .parse()
        .unwrap_or_else(|_| panic!("{key} should be declared as plain seconds"))
}
