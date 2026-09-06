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
    let cfg = Config {
        container_memory_mb: 512,
        ..Config::default()
    };
    for (input, expected) in [
        (Some(1), 512 * 1024 * 1024),
        (None, 512 * 1024 * 1024),
        (Some(2 * 1024 * 1024 * 1024), 2 * 1024 * 1024 * 1024),
    ] {
        let c = ArmorContainerConfig {
            memory_limit: input,
            ..base_config()
        };
        let out = BollardRuntime::build_bollard_config(&c, &cfg).unwrap();
        assert_eq!(out.host_config.unwrap().memory, Some(expected));
    }
}

#[test]
fn cpu_shares_are_clamped_2_to_4096() {
    let cfg = Config {
        container_cpu_shares: 1024,
        ..Config::default()
    };
    for (input, expected) in [(Some(1), 2), (Some(999_999), 4096), (None, 1024)] {
        let c = ArmorContainerConfig {
            cpu_shares: input,
            ..base_config()
        };
        let out = BollardRuntime::build_bollard_config(&c, &cfg).unwrap();
        assert_eq!(out.host_config.unwrap().cpu_shares, Some(expected));
    }
}

#[test]
fn pids_limit_is_clamped_10_to_1000() {
    let cfg = Config {
        container_pids_limit: 100,
        ..Config::default()
    };
    for (input, expected) in [(Some(1), 10), (Some(5000), 1000), (None, 100)] {
        let c = ArmorContainerConfig {
            pids_limit: input,
            ..base_config()
        };
        let out = BollardRuntime::build_bollard_config(&c, &cfg).unwrap();
        assert_eq!(out.host_config.unwrap().pids_limit, Some(expected));
    }
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
    let err = BollardRuntime::build_bollard_config(&cfg, &Config::default()).unwrap_err();
    assert!(matches!(err, agentic_armor::ArmorError::ForbiddenMount(_)));
}

#[test]
fn network_none_is_the_default_network_mode() {
    let cfg = base_config();
    let out = BollardRuntime::build_bollard_config(&cfg, &Config::default()).unwrap();
    assert_eq!(out.host_config.unwrap().network_mode, Some("none".into()));
}

#[test]
fn userns_mode_is_opt_in_and_validated() {
    let out = BollardRuntime::build_bollard_config(&base_config(), &Config::default()).unwrap();
    assert_eq!(out.host_config.unwrap().userns_mode, None);

    let cfg = Config {
        container_userns_mode: Some("auto".into()),
        ..Config::default()
    };
    let out = BollardRuntime::build_bollard_config(&base_config(), &cfg).unwrap();
    assert_eq!(
        out.host_config.unwrap().userns_mode,
        Some("auto".to_string())
    );

    let bad = Config {
        container_userns_mode: Some("AUTO MODE!".into()),
        ..Config::default()
    };
    let err = BollardRuntime::build_bollard_config(&base_config(), &bad).unwrap_err();
    assert!(matches!(
        err,
        agentic_armor::ArmorError::InvalidUsernsMode(_)
    ));

    for weakens in ["host", "private"] {
        let isolate_off = Config {
            container_userns_mode: Some(weakens.into()),
            ..Config::default()
        };
        let err = BollardRuntime::build_bollard_config(&base_config(), &isolate_off).unwrap_err();
        assert!(
            matches!(err, agentic_armor::ArmorError::InvalidUsernsMode(_)),
            "'{}' must be rejected: it disables or breaks userns isolation",
            weakens
        );
    }
}

#[test]
fn forbidden_mount_patterns_are_read_from_config_not_hardcoded() {
    let hostile = ArmorContainerConfig {
        mounts: Some(vec![Mount {
            source: "/srv/topsecret/keys".into(),
            target: "/workspace/keys".into(),
            mount_type: "bind".into(),
            read_only: Some(true),
            tmpfs_options: None,
        }]),
        ..base_config()
    };
    let cfg = Config {
        forbidden_mount_patterns: vec!["topsecret".into()],
        ..Config::default()
    };
    let err = BollardRuntime::build_bollard_config(&hostile, &cfg).unwrap_err();
    assert!(
        matches!(err, agentic_armor::ArmorError::ForbiddenMount(_)),
        "a custom pattern must be enforced: {err}"
    );

    let permissive = Config {
        forbidden_mount_patterns: vec![],
        ..Config::default()
    };
    assert!(
        BollardRuntime::build_bollard_config(&hostile, &permissive).is_ok(),
        "an empty config list must allow the mount — proving the field drives the check"
    );

    let via_target = ArmorContainerConfig {
        mounts: Some(vec![Mount {
            source: "/data".into(),
            target: "/workspace/DOCKer.SOCK".into(),
            mount_type: "bind".into(),
            read_only: Some(false),
            tmpfs_options: None,
        }]),
        ..base_config()
    };
    assert!(BollardRuntime::build_bollard_config(&via_target, &Config::default()).is_err());
}

#[test]
fn internal_egress_flag_reaches_network_creation() {
    use agentic_armor::config::TaskNetworkEgress;
    let internal = agentic_armor::docker::BollardRuntime::network_create_options(
        "armor-t1",
        TaskNetworkEgress::Internal,
    );
    assert!(
        internal.internal,
        "internal egress must set the docker flag"
    );
    let masquerade = agentic_armor::docker::BollardRuntime::network_create_options(
        "armor-t1",
        TaskNetworkEgress::Masquerade,
    );
    assert!(!masquerade.internal);
}

#[test]
fn cgroup_membership_check_accepts_both_layouts_and_rejects_other_containers() {
    let id = "a".repeat(64);
    let cgroupfs = format!("0::/docker/{}/payload", id);
    let systemd = format!("11:devices:/system.slice/docker-{}.scope", id);
    let other = "b".repeat(64);
    assert!(agentic_armor::docker::cgroup_contains_container(
        &cgroupfs, &id
    ));
    assert!(agentic_armor::docker::cgroup_contains_container(
        &systemd, &id
    ));
    assert!(!agentic_armor::docker::cgroup_contains_container(
        &cgroupfs, &other
    ));
}

#[test]
fn host_escalated_kills_get_their_own_audit_label() {
    let message = agentic_armor::mcp::server::exec_audit_message(
        137,
        5000,
        agentic_armor::docker::KillOutcome::HostEscalated,
        "sleep 999",
    );
    assert!(
        message.contains("timedOut=true kill=host-escalated"),
        "{}",
        message
    );
    assert!(message.contains("exec exit=137"), "{}", message);
}

#[test]
fn handler_ceiling_defaults_high_enough_for_real_builds() {
    assert_eq!(
        agentic_armor::config::Config::default().handler_timeout_secs,
        900,
        "default ceiling must accommodate multi-minute builds and test suites"
    );
}
