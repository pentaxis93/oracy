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
fn quadlet_template_privately_relabels_selinux_visible_mounts() {
    let template = std::fs::read_to_string("../deploy/quadlet/oracy.container.in")
        .expect("read container Quadlet template");

    assert!(template.contains("Volume=@ORACY_CONFIG_PATH@:/etc/oracy/oracy.toml:ro,Z"));
    assert!(template.contains("Volume=oracy-data.volume:/var/lib/oracy:rw,Z"));
    assert!(!template.contains("SecurityLabelDisable=true"));
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

#[test]
fn deployment_readme_documents_reverse_proxy_networking_patterns() {
    let readme = std::fs::read_to_string("../deploy/README.md").expect("read deployment README");

    assert!(readme.contains("127.0.0.1:8080:8080"));
    assert!(readme.contains("Network=oracy-proxy.network"));
    assert!(readme.contains("http://oracy:8080"));
    assert!(readme.contains("remove the public `PublishPort=@ORACY_PUBLIC_PUBLISH@` line"));
    assert!(readme.contains("host.containers.internal"));
    assert!(readme.contains("host.docker.internal"));
    assert!(readme.contains("--add-host=host.docker.internal:host-gateway"));
    assert!(readme.contains("extra_hosts"));
    assert!(readme.contains("host.docker.internal:host-gateway"));
    assert!(readme.contains("host-gateway"));
    assert!(readme.contains("0.0.0.0:8080:8080"));
    assert!(readme.contains("is reachability, not protection"));
    assert!(readme.contains("verify that the port is blocked from untrusted networks"));
}

#[test]
fn deployment_contract_supports_common_reverse_proxy_topologies() {
    let contract =
        std::fs::read_to_string("../spec/deployment.md").expect("read deployment contract");
    let public_api = markdown_section(&contract, "Public API Reverse Proxy");

    assert!(public_api.contains("host-system reverse proxy"));
    assert!(public_api.contains("shared container network"));
    assert!(public_api.contains("isolated container reverse proxy"));
    assert!(public_api.contains("operator-managed firewall"));
    assert!(public_api.contains("non-loopback binding is reachability, not protection"));
}

#[test]
fn deployment_contract_documents_selinux_labeling_for_all_state_storage() {
    let contract =
        std::fs::read_to_string("../spec/deployment.md").expect("read deployment contract");
    let accepted_audio = markdown_section(&contract, "Accepted Audio Directory");
    let sqlite_database = markdown_section(&contract, "SQLite Database File");

    assert!(accepted_audio.contains("On SELinux-enforcing hosts"));
    assert!(sqlite_database.contains("On SELinux-enforcing hosts"));
    assert!(sqlite_database.contains("parent directory"));
    assert!(sqlite_database.contains("SQLite sidecar files"));
}

fn quadlet_seconds(template: &str, key: &str) -> u64 {
    template
        .lines()
        .find_map(|line| line.strip_prefix(&format!("{key}=")))
        .unwrap_or_else(|| panic!("{key} should be declared"))
        .parse()
        .unwrap_or_else(|_| panic!("{key} should be declared as plain seconds"))
}

fn markdown_section<'a>(document: &'a str, heading: &str) -> &'a str {
    let start_marker = format!("## {heading}");
    let start = document
        .find(&start_marker)
        .unwrap_or_else(|| panic!("{heading} section should exist"));
    let after_heading = start + start_marker.len();
    let end = document[after_heading..]
        .find("\n## ")
        .map_or(document.len(), |offset| after_heading + offset);

    &document[start..end]
}
