use agentic_armor::docker::manager::BollardRuntime;
use agentic_armor::{ArmorContainerConfig, Config, Mount, NetworkConfig};

fn base_config() -> ArmorContainerConfig {
    ArmorContainerConfig {
        name: "armor-test".into(),
        image: "opencode-sandbox-base:latest".into(),
        command: Some(vec!["sleep".into(), "infinity".into()]),
        network: NetworkConfig::None,
        ..Default::default()
    }
}

#[test]
fn hardening_flags_cannot_be_weakened_by_caller() {
    let cfg = ArmorContainerConfig {
        readonly_rootfs: Some(false),
        cap_drop: Some(vec!["NET_ADMIN".into()]),
        user: Some("root".into()),
        ..base_config()
    };
    let out = BollardRuntime::build_bollard_config(&cfg, &Config::default()).unwrap();
    let hc = out.host_config.expect("host_config");
    assert_eq!(hc.cap_drop, Some(vec!["ALL".to_string()]));
    assert_eq!(hc.readonly_rootfs, Some(true));
    assert!(hc
        .security_opt
        .expect("security_opt")
        .iter()
        .any(|o| o == "no-new-privileges"));
    assert_eq!(out.user, Some("opencode".to_string()));
}

#[test]
fn memory_is_floored_at_512mb() {
    let tiny = ArmorContainerConfig {
        memory_limit: Some(1),
        ..base_config()
    };
    let out = BollardRuntime::build_bollard_config(&tiny, &Config::default()).unwrap();
    assert_eq!(
        out.host_config.unwrap().memory,
        Some(512 * 1024 * 1024)
    );

    let unset = base_config();
    let out = BollardRuntime::build_bollard_config(&unset, &Config::default()).unwrap();
    assert_eq!(
        out.host_config.unwrap().memory,
        Some(512 * 1024 * 1024)
    );

    let large = ArmorContainerConfig {
        memory_limit: Some(2 * 1024 * 1024 * 1024),
        ..base_config()
    };
    let out = BollardRuntime::build_bollard_config(&large, &Config::default()).unwrap();
    assert_eq!(
        out.host_config.unwrap().memory,
        Some(2 * 1024 * 1024 * 1024)
    );
}

#[test]
fn pids_limit_is_clamped_10_to_1000() {
    let low = ArmorContainerConfig {
        pids_limit: Some(1),
        ..base_config()
    };
    let out = BollardRuntime::build_bollard_config(&low, &Config::default()).unwrap();
    assert_eq!(out.host_config.unwrap().pids_limit, Some(10));

    let high = ArmorContainerConfig {
        pids_limit: Some(5000),
        ..base_config()
    };
    let out = BollardRuntime::build_bollard_config(&high, &Config::default()).unwrap();
    assert_eq!(out.host_config.unwrap().pids_limit, Some(1000));

    let unset = base_config();
    let out = BollardRuntime::build_bollard_config(&unset, &Config::default()).unwrap();
    assert_eq!(out.host_config.unwrap().pids_limit, Some(100));
}

#[test]
fn docker_socket_mounts_are_rejected() {
    let cfg = ArmorContainerConfig {
        mounts: Some(vec![Mount {
            source: "/var/run/docker.sock".into(),
            target: "/workspace/docker.sock".into(),
            mount_type: "bind".into(),
            read_only: Some(false),
            tmpfs_options: None,
        }]),
        ..base_config()
    };
    let err = BollardRuntime::build_bollard_config(&cfg, &Config::default()).unwrap_err();
    assert!(matches!(err, agentic_armor::ArmorError::ForbiddenMount(_)));
}

#[test]
fn images_outside_the_allowlist_are_rejected() {
    let cfg = ArmorContainerConfig {
        image: "ubuntu:latest".into(),
        ..base_config()
    };
    assert!(BollardRuntime::build_bollard_config(&cfg, &Config::default()).is_err());
}

#[test]
fn network_none_is_the_default_network_mode() {
    let cfg = base_config();
    let out = BollardRuntime::build_bollard_config(&cfg, &Config::default()).unwrap();
    assert_eq!(out.host_config.unwrap().network_mode, Some("none".into()));
}
