use std::env;

#[derive(Clone, Debug, Default, PartialEq)]
pub enum RuntimeChoice {
    #[default]
    Auto,
    Docker,
    Podman,
}

impl RuntimeChoice {
    pub fn parse(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "docker" => RuntimeChoice::Docker,
            "podman" => RuntimeChoice::Podman,
            _ => RuntimeChoice::Auto,
        }
    }
}

/// Per-task network egress policy. `Masquerade` is stock Docker NAT (the
/// S08 bridge-exfil boundary is by-design). `Internal` creates per-task
/// networks with internal=true: no outbound routing at all, so egress fails
/// closed instead of relying on agent cooperation.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum TaskNetworkEgress {
    #[default]
    Masquerade,
    Internal,
}

impl TaskNetworkEgress {
    pub fn parse(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "internal" => TaskNetworkEgress::Internal,
            _ => TaskNetworkEgress::Masquerade,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Config {
    pub database_url: String,
    pub docker_socket: Option<String>,
    pub container_memory_mb: i64,
    pub container_cpu_shares: i64,
    pub container_pids_limit: i64,
    pub allow_host_network: bool,
    pub task_network_egress: TaskNetworkEgress,
    pub container_runtime: RuntimeChoice,
    pub podman_socket: Option<String>,
    pub container_userns_mode: Option<String>,
    pub allowed_images: Vec<String>,
    pub allowed_path_prefixes: Vec<String>,
    pub forbidden_mount_patterns: Vec<String>,
    pub tombstone_path: std::path::PathBuf,
}

impl Default for Config {
    fn default() -> Self {
        let database_url =
            env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite:./data/agentic_armor.db".into());
        Config {
            database_url: database_url.clone(),
            docker_socket: env::var("DOCKER_SOCKET").ok(),
            container_memory_mb: env::var("CONTAINER_MEMORY_MB")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(512),
            container_cpu_shares: env::var("CONTAINER_CPU_SHARES")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(1024),
            container_pids_limit: env::var("CONTAINER_PIDS_LIMIT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(100),
            allow_host_network: env::var("ALLOW_HOST_NETWORK").as_deref() == Ok("true"),
            task_network_egress: TaskNetworkEgress::parse(
                &env::var("TASK_NETWORK_EGRESS").unwrap_or_default(),
            ),
            container_runtime: RuntimeChoice::parse(
                &env::var("CONTAINER_RUNTIME").unwrap_or_default(),
            ),
            podman_socket: env::var("PODMAN_SOCKET").ok(),
            container_userns_mode: env::var("CONTAINER_USERNS_MODE")
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty()),
            allowed_images: vec![
                "opencode-sandbox-base:latest".into(),
                "opencode-sandbox-developer:latest".into(),
            ],
            allowed_path_prefixes: vec![
                "/tmp/".into(),
                "/home/opencode/".into(),
                "/workspace/".into(),
            ],
            forbidden_mount_patterns: vec![
                "docker.sock".into(),
                "/var/run/docker".into(),
                "/run/docker".into(),
                "podman.sock".into(),
                "/run/podman".into(),
            ],
            tombstone_path: tombstone_path_for(&database_url),
        }
    }
}

/// Where audit events that could not be written to the database are parked
/// (`tombstones.jsonl` beside the database file) until the next boot replays
/// them.
pub fn tombstone_path_for(database_url: &str) -> std::path::PathBuf {
    let file = database_url
        .strip_prefix("sqlite://")
        .or_else(|| database_url.strip_prefix("sqlite:"))
        .unwrap_or(database_url);
    let parent = std::path::Path::new(file)
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(std::path::Path::new("."));
    parent.join("tombstones.jsonl")
}

pub fn validate_database_url(url: &str) -> Result<(), String> {
    if url.starts_with("sqlite:") {
        Ok(())
    } else {
        Err(format!("DATABASE_URL must be a sqlite: URL, got {:?}", url))
    }
}
