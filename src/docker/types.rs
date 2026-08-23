use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArmorContainerConfig {
    pub name: String,
    pub image: String,
    pub command: Option<Vec<String>>,
    pub working_dir: Option<String>,
    pub env: Option<Vec<String>>,
    pub mounts: Option<Vec<Mount>>,
    pub network_mode: Option<String>,
    pub network_name: Option<String>,
    pub memory_limit: Option<i64>,
    pub cpu_shares: Option<i64>,
    pub pids_limit: Option<i64>,
    pub cap_drop: Option<Vec<String>>,
    pub readonly_rootfs: Option<bool>,
    pub no_new_privileges: Option<bool>,
    pub user: Option<String>,
    pub auto_remove: Option<bool>,
}

impl Default for ArmorContainerConfig {
    fn default() -> Self {
        ArmorContainerConfig {
            name: String::new(),
            image: "opencode-sandbox-base:latest".into(),
            command: None,
            working_dir: None,
            env: None,
            mounts: None,
            network_mode: None,
            network_name: None,
            memory_limit: None,
            cpu_shares: None,
            pids_limit: None,
            cap_drop: None,
            readonly_rootfs: None,
            no_new_privileges: None,
            user: None,
            auto_remove: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Mount {
    pub source: String,
    pub target: String,
    #[serde(rename = "type")]
    pub mount_type: String,
    #[serde(rename = "readOnly", skip_serializing_if = "Option::is_none")]
    pub read_only: Option<bool>,
    pub tmpfs_options: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecResult {
    pub exit_code: i64,
    pub stdout: String,
    pub stderr: String,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ContainerStats {
    pub memory_usage: u64,
    pub memory_limit: u64,
    pub cpu_percent: f64,
    pub pids: u64,
    pub network_rx_bytes: u64,
    pub network_tx_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ExecRequest {
    pub command: Vec<String>,
    pub timeout_ms: Option<u64>,
    pub user: Option<String>,
    pub working_dir: Option<String>,
    pub env: Option<Vec<String>>,
}
