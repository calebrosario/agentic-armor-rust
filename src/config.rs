use std::env;

#[derive(Clone, Debug, Default, PartialEq)]
pub enum RuntimeChoice {
    #[default]
    Auto,
    Docker,
    Podman,
}

impl RuntimeChoice {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "docker" => RuntimeChoice::Docker,
            "podman" => RuntimeChoice::Podman,
            _ => RuntimeChoice::Auto,
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
    pub container_runtime: RuntimeChoice,
    pub podman_socket: Option<String>,
    pub allowed_images: Vec<String>,
    pub allowed_path_prefixes: Vec<String>,
    pub forbidden_mount_patterns: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            database_url: env::var("DATABASE_URL")
                .unwrap_or_else(|_| "postgresql://localhost:5433/agentic_armor".into()),
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
            container_runtime: RuntimeChoice::from_str(
                &env::var("CONTAINER_RUNTIME").unwrap_or_default(),
            ),
            podman_socket: env::var("PODMAN_SOCKET").ok(),
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
            ],
        }
    }
}
