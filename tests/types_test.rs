use agentic_armor::{ArmorContainerConfig, NetworkConfig};

#[test]
fn test_container_config_defaults() {
    let config = ArmorContainerConfig::default();
    assert_eq!(config.image, "opencode-sandbox-base:latest");
    assert!(config.command.is_none());
    assert_eq!(config.network, NetworkConfig::None);
    assert!(config.mounts.is_none());
    assert!(config.readonly_rootfs.is_none());
    assert!(config.cap_drop.is_none());
}

#[test]
fn test_container_config_builder_pattern() {
    let config = ArmorContainerConfig {
        name: "test".into(),
        image: "alpine:latest".into(),
        command: Some(vec!["sleep".into(), "10".into()]),
        network: NetworkConfig::Bridge { network: "armor-t1".into() },
        memory_limit: Some(512 * 1024 * 1024),
        cpu_shares: Some(1024),
        pids_limit: Some(100),
        readonly_rootfs: Some(true),
        cap_drop: Some(vec!["ALL".into()]),
        ..Default::default()
    };

    assert_eq!(config.name, "test");
    assert_eq!(config.image, "alpine:latest");
    assert_eq!(config.command.as_ref().unwrap().len(), 2);
    assert_eq!(
        config.network,
        NetworkConfig::Bridge { network: "armor-t1".into() }
    );
}

#[test]
fn test_mount_serialization() {
    let mount = agentic_armor::Mount {
        source: "/host/data".into(),
        target: "/container/data".into(),
        mount_type: "bind".into(),
        read_only: Some(true),
        tmpfs_options: None,
    };

    let json = serde_json::to_string(&mount).unwrap();
    assert!(json.contains(r#""source":"/host/data""#));
    assert!(json.contains(r#""type":"bind""#));
    assert!(json.contains(r#""readOnly":true"#));
}

#[test]
fn test_exec_request_default() {
    let req = agentic_armor::ExecRequest::default();
    assert!(req.command.is_empty());
    assert!(req.timeout_ms.is_none());
    assert!(req.user.is_none());
}

#[test]
fn test_exec_result_serialization() {
    let result = agentic_armor::ExecResult {
        exit_code: 0,
        stdout: "hello\n".into(),
        stderr: "".into(),
        notes: vec![],
        duration_ms: 42,
    };

    let json = serde_json::to_string(&result).unwrap();
    let back: agentic_armor::ExecResult = serde_json::from_str(&json).unwrap();
    assert_eq!(back.exit_code, 0);
    assert_eq!(back.stdout, "hello\n");
    assert!(back.notes.is_empty());
    assert_eq!(back.duration_ms, 42);
}
