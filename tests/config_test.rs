use agentic_armor::{Config, RuntimeChoice};

#[test]
fn test_default_config() {
    let config = Config::default();
    assert_eq!(config.container_memory_mb, 512);
    assert_eq!(config.container_cpu_shares, 1024);
    assert_eq!(config.container_pids_limit, 100);
    assert!(!config.allow_host_network);
    assert_eq!(config.container_runtime, RuntimeChoice::Auto);
}

#[test]
fn test_allowed_images() {
    let config = Config::default();
    assert!(config
        .allowed_images
        .contains(&"opencode-sandbox-base:latest".to_string()));
    assert!(config
        .allowed_images
        .contains(&"opencode-sandbox-developer:latest".to_string()));
}

#[test]
fn test_forbidden_mount_patterns() {
    let config = Config::default();
    for pattern in [
        "docker.sock",
        "/var/run/docker",
        "/run/docker",
        "podman.sock",
        "/run/podman",
    ] {
        assert!(
            config
                .forbidden_mount_patterns
                .contains(&pattern.to_string()),
            "missing forbidden mount pattern: {}",
            pattern
        );
    }
}

#[test]
fn test_allowed_path_prefixes() {
    let config = Config::default();
    assert!(config.allowed_path_prefixes.contains(&"/tmp/".to_string()));
    assert!(config
        .allowed_path_prefixes
        .contains(&"/home/opencode/".to_string()));
    assert!(config
        .allowed_path_prefixes
        .contains(&"/workspace/".to_string()));
}

#[test]
fn database_url_must_be_a_sqlite_url() {
    use agentic_armor::config::validate_database_url;
    assert!(validate_database_url("sqlite:./data/agentic_armor.db").is_ok());
    assert!(validate_database_url("sqlite::memory:").is_ok());
    for bad in [
        "postgresql://host/db",
        "postgres://host/db",
        "mysql://host/db",
        "",
        "./data/x.db",
    ] {
        assert!(
            validate_database_url(bad).is_err(),
            "'{bad}' must be rejected"
        );
    }
}

#[test]
fn tombstone_path_lives_next_to_the_database() {
    let nested = agentic_armor::config::tombstone_path_for("sqlite://var/lib/armor/x.db");
    assert_eq!(
        nested,
        std::path::PathBuf::from("var/lib/armor/tombstones.jsonl")
    );
    let bare = agentic_armor::config::tombstone_path_for("sqlite:x.db");
    assert_eq!(bare, std::path::PathBuf::from("./tombstones.jsonl"));
}

#[test]
fn task_network_egress_parses() {
    use agentic_armor::config::TaskNetworkEgress;
    assert_eq!(
        TaskNetworkEgress::parse("internal"),
        TaskNetworkEgress::Internal
    );
    assert_eq!(
        TaskNetworkEgress::parse("Masquerade"),
        TaskNetworkEgress::Masquerade
    );
    assert_eq!(
        TaskNetworkEgress::parse("bogus"),
        TaskNetworkEgress::Masquerade,
        "unknown values must fail open to the audited default, not to lockdown"
    );
}

#[test]
fn csv_lists_trim_and_drop_empties() {
    use agentic_armor::config::parse_csv_list;
    assert!(parse_csv_list("").is_empty());
    assert!(parse_csv_list(" , ,").is_empty());
    assert_eq!(
        parse_csv_list(" a.img:1 , b.img:2 ,, c.img:3"),
        vec!["a.img:1", "b.img:2", "c.img:3"]
    );
}
