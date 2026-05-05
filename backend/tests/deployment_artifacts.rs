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
