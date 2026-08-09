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
    assert!(config.allowed_images.contains(&"opencode-sandbox-base:latest".to_string()));
    assert!(config.allowed_images.contains(&"opencode-sandbox-developer:latest".to_string()));
}

#[test]
fn test_forbidden_mount_patterns() {
    let config = Config::default();
    assert!(config.forbidden_mount_patterns.contains(&"docker.sock".to_string()));
    assert!(config.forbidden_mount_patterns.contains(&"/var/run/docker".to_string()));
}

#[test]
fn test_allowed_path_prefixes() {
    let config = Config::default();
    assert!(config.allowed_path_prefixes.contains(&"/tmp/".to_string()));
    assert!(config.allowed_path_prefixes.contains(&"/home/opencode/".to_string()));
    assert!(config.allowed_path_prefixes.contains(&"/workspace/".to_string()));
}
