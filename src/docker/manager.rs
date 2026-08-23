use crate::config::Config;
use crate::error::{ArmorError, ArmorResult};
use crate::docker::types::*;
use async_trait::async_trait;
use bollard::container::*;
use bollard::exec::{CreateExecOptions, StartExecResults};
use bollard::models::*;
use bollard::Docker;
use futures_util::StreamExt;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{info, warn};

pub fn is_valid_task_network_name(name: &str) -> bool {
    name.starts_with("armor-")
        && name.len() <= 64
        && name["armor-".len()..]
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

pub fn task_network_name(task_id: &str) -> String {
    format!("armor-{}", task_id)
}

#[async_trait]
pub trait ContainerRuntime: Send + Sync {
    async fn ping(&self) -> ArmorResult<()>;
    async fn create_container(&self, config: &ArmorContainerConfig) -> ArmorResult<String>;
    async fn start_container(&self, id: &str) -> ArmorResult<()>;
    async fn stop_container(&self, id: &str, timeout_secs: i64) -> ArmorResult<()>;
    async fn remove_container(&self, id: &str, force: bool) -> ArmorResult<()>;
    async fn destroy_container(&self, id: &str) -> ArmorResult<()>;
    async fn exec_in_container(&self, id: &str, request: &ExecRequest) -> ArmorResult<ExecResult>;
    async fn is_running(&self, id: &str) -> ArmorResult<bool>;
    async fn create_network(&self, name: &str) -> ArmorResult<()>;
    async fn remove_network(&self, name: &str) -> ArmorResult<()>;
    fn runtime_name(&self) -> &str;
}

pub struct BollardRuntime {
    docker: Docker,
    config: Arc<Config>,
    runtime_name: String,
}

impl BollardRuntime {
    pub fn new_docker(config: Arc<Config>) -> ArmorResult<Self> {
        let docker = Self::connect(
            config.docker_socket.as_deref().unwrap_or("/var/run/docker.sock"),
        )?;
        Ok(BollardRuntime {
            docker,
            config,
            runtime_name: "docker".into(),
        })
    }

    pub fn new_podman(config: Arc<Config>) -> ArmorResult<Self> {
        let socket = std::env::var("PODMAN_SOCKET")
            .unwrap_or_else(|_| {
                let uid = unsafe { libc::getuid() };
                if uid == 0 {
                    "/run/podman/podman.sock".into()
                } else {
                    format!("/run/user/{}/podman/podman.sock", uid)
                }
            });
        let docker = Self::connect(&socket)?;
        Ok(BollardRuntime {
            docker,
            config,
            runtime_name: "podman".into(),
        })
    }

    pub fn auto_detect(config: Arc<Config>) -> ArmorResult<Self> {
        match config.container_runtime {
            crate::config::RuntimeChoice::Docker => {
                info!("Forced Docker runtime via CONTAINER_RUNTIME=docker");
                Self::new_docker(config)
            }
            crate::config::RuntimeChoice::Podman => {
                info!("Forced Podman runtime via CONTAINER_RUNTIME=podman");
                Self::new_podman(config)
            }
            crate::config::RuntimeChoice::Auto => {
                let docker_socket = config
                    .docker_socket
                    .as_deref()
                    .unwrap_or("/var/run/docker.sock");

                let docker_desktop_socket = format!(
                    "{}/.docker/run/docker.sock",
                    std::env::var("HOME").unwrap_or_default()
                );

                if std::path::Path::new(docker_socket).exists() {
                    info!("Docker socket found — using Docker runtime");
                    return Self::new_docker(config);
                }

                if std::path::Path::new(&docker_desktop_socket).exists() {
                    info!("Docker Desktop socket found at {} — using Docker runtime", docker_desktop_socket);
                    let mut new_config = (*config).clone();
                    new_config.docker_socket = Some(docker_desktop_socket);
                    return Self::new_docker(Arc::new(new_config));
                }

                let podman_socket = config.podman_socket.as_deref()
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| {
                        let uid = unsafe { libc::getuid() };
                        if uid == 0 {
                            "/run/podman/podman.sock".to_string()
                        } else {
                            format!("/run/user/{}/podman/podman.sock", uid)
                        }
                    });

                if std::path::Path::new(&podman_socket).exists() {
                    info!("Podman socket found at {} — using Podman runtime", podman_socket);
                    return Self::new_podman(config);
                }

                warn!("No Docker or Podman socket found — trying Docker defaults");
                Self::new_docker(config)
            }
        }
    }

    fn connect(socket: &str) -> ArmorResult<Docker> {
        if std::path::Path::new(socket).exists() {
            info!("Connecting to container runtime socket: {}", socket);
            Docker::connect_with_socket(
                socket,
                120,
                bollard::API_DEFAULT_VERSION,
            )
            .map_err(|e| ArmorError::DockerConnectionFailed(format!("{}: {}", socket, e)))
        } else {
            warn!("Socket not found: {} — trying default connection", socket);
            Docker::connect_with_local_defaults()
                .map_err(|e| ArmorError::DockerConnectionFailed(e.to_string()))
        }
    }
}

#[async_trait]
impl ContainerRuntime for BollardRuntime {
    fn runtime_name(&self) -> &str {
        &self.runtime_name
    }

    async fn ping(&self) -> ArmorResult<()> {
        self.docker
            .version()
            .await
            .map_err(|e| ArmorError::DockerConnectionFailed(e.to_string()))?;
        Ok(())
    }

    async fn create_container(&self, config: &ArmorContainerConfig) -> ArmorResult<String> {
        let bollard_config = self.build_bollard_config(config)?;

        let create_options = CreateContainerOptions {
            name: config.name.clone(),
            platform: None,
        };

        match self
            .docker
            .create_container::<String, String>(Some(create_options), bollard_config)
            .await
        {
            Ok(response) => {
                let id = response.id;
                info!("Container created: {} ({})", config.name, id);
                Ok(id)
            }
            Err(e) => {
                warn!("Container creation failed: {} — {}", config.name, e);
                Err(ArmorError::ContainerCreateFailed(e.to_string()))
            }
        }
    }

    async fn start_container(&self, id: &str) -> ArmorResult<()> {
        self.docker
            .start_container::<&str>(id, None)
            .await?;
        Ok(())
    }

    async fn stop_container(&self, id: &str, timeout_secs: i64) -> ArmorResult<()> {
        self.docker
            .stop_container(id, Some(StopContainerOptions { t: timeout_secs }))
            .await?;
        Ok(())
    }

    async fn remove_container(&self, id: &str, force: bool) -> ArmorResult<()> {
        self.docker
            .remove_container(
                id,
                Some(RemoveContainerOptions {
                    force,
                    v: true,
                    link: false,
                }),
            )
            .await?;
        Ok(())
    }

    async fn destroy_container(&self, id: &str) -> ArmorResult<()> {
        let _ = self.stop_container(id, 5).await;
        self.remove_container(id, true).await?;
        Ok(())
    }

    async fn exec_in_container(
        &self,
        id: &str,
        request: &ExecRequest,
    ) -> ArmorResult<ExecResult> {
        let start = Instant::now();

        let exec_config = CreateExecOptions {
            cmd: Some(request.command.clone()),
            user: request.user.clone(),
            working_dir: request.working_dir.clone(),
            env: None,
            attach_stdout: Some(true),
            attach_stderr: Some(true),
            ..Default::default()
        };

        let exec_response = self.docker.create_exec(id, exec_config).await?;
        let exec_id = exec_response.id;

        let mut stdout_buf: Vec<u8> = Vec::new();
        let mut stderr_buf: Vec<u8> = Vec::new();

        let start_exec_result = self.docker.start_exec(&exec_id, None).await;

        let timeout_ms = request.timeout_ms.unwrap_or(60_000);

        match start_exec_result {
            Ok(StartExecResults::Attached { mut output, .. }) => {
                tokio::select! {
                    _ = async {
                        while let Some(msg) = output.next().await {
                            match msg {
                                Ok(LogOutput::StdOut { message }) => stdout_buf.extend_from_slice(&message),
                                Ok(LogOutput::StdErr { message }) => stderr_buf.extend_from_slice(&message),
                                Ok(_) => {}
                                Err(e) => {
                                    warn!("Exec stream error: {}", e);
                                    break;
                                }
                            }
                        }
                    } => {}
                    _ = tokio::time::sleep(Duration::from_millis(timeout_ms)) => {
                        warn!("Exec timed out after {}ms", timeout_ms);
                    }
                }
            }
            Ok(StartExecResults::Detached) => {
                info!("Exec detached (no output capture)");
            }
            Err(e) => {
                return Err(ArmorError::Docker(e.to_string()));
            }
        }

        let exec_inspect = self.docker.inspect_exec(&exec_id).await?;
        let exit_code = exec_inspect.exit_code.unwrap_or(-1);
        let duration_ms = start.elapsed().as_millis() as u64;

        Ok(ExecResult {
            exit_code,
            stdout: String::from_utf8_lossy(&stdout_buf).to_string(),
            stderr: String::from_utf8_lossy(&stderr_buf).to_string(),
            duration_ms,
        })
    }

    async fn is_running(&self, id: &str) -> ArmorResult<bool> {
        let info = self.docker.inspect_container(id, None).await?;
        Ok(info.state.and_then(|s| s.running).unwrap_or(false))
    }

    async fn create_network(&self, name: &str) -> ArmorResult<()> {
        if self.docker.inspect_network::<String>(name, None).await.is_ok() {
            return Ok(());
        }
        self.docker
            .create_network(bollard::network::CreateNetworkOptions {
                name: name.to_string(),
                check_duplicate: true,
                ..Default::default()
            })
            .await?;
        Ok(())
    }

    async fn remove_network(&self, name: &str) -> ArmorResult<()> {
        self.docker.remove_network(name).await?;
        Ok(())
    }
}

impl BollardRuntime {
    pub fn build_bollard_config(
        &self,
        config: &ArmorContainerConfig,
    ) -> ArmorResult<bollard::container::Config<String>> {
        if !self.config.allowed_images.iter().any(|img| img == &config.image) {
            return Err(ArmorError::ForbiddenMount(format!(
                "Image not allowed"
            )));
        }

        let cmd = config.command.clone();
        let env = config.env.clone();

        let mut binds: Vec<String> = Vec::new();
        let mut tmpfs: HashMap<String, String> = HashMap::new();

        if let Some(ref mounts) = config.mounts {
            for mount in mounts {
                match mount.mount_type.as_str() {
                    "bind" | "volume" => {
                        if mount.source.trim().is_empty() {
                            return Err(ArmorError::InvalidMountConfig(format!(
                                "Mount type '{}' requires non-empty source",
                                mount.mount_type
                            )));
                        }
                        let source_to_check = if let Ok(canonical) = std::fs::canonicalize(&mount.source) {
                            canonical.to_string_lossy().to_lowercase()
                        } else {
                            mount.source.to_lowercase()
                        };
                        let target_lower = mount.target.to_lowercase();
                        let all_patterns: Vec<&str> = vec![
                            "docker.sock", "/var/run/docker", "/run/docker",
                            "podman.sock", "/run/podman",
                        ];
                        for pattern in all_patterns {
                            if source_to_check.contains(pattern) || target_lower.contains(pattern) {
                                warn!("Security policy rejected mount '{}:{}'", mount.source, mount.target);
                                return Err(ArmorError::ForbiddenMount("Mount blocked by security policy".into()));
                            }
                        }
                        let ro = mount.read_only.unwrap_or(false);
                        binds.push(format!("{}:{}{}", mount.source, mount.target, if ro { ":ro" } else { "" }));
                    }
                    "tmpfs" => {
                        let opts = mount.tmpfs_options.clone().unwrap_or_default();
                        tmpfs.insert(mount.target.clone(), opts);
                    }
                    _ => {
                        return Err(ArmorError::InvalidMountConfig(
                            format!("Unsupported mount type: '{}'", mount.mount_type)
                        ));
                    }
                }
            }
        }

        let effective_network_mode = config.network_mode.clone().unwrap_or_else(|| "none".into());
        let allowed = ["bridge", "none"];
        if !allowed.contains(&effective_network_mode.as_str()) {
            if effective_network_mode == "host" && self.config.allow_host_network {
                warn!("Host network mode allowed via ALLOW_HOST_NETWORK=true");
            } else {
                return Err(ArmorError::InvalidNetworkMode(effective_network_mode));
            }
        }

        let docker_network_mode = match (effective_network_mode.as_str(), &config.network_name) {
            ("bridge", Some(name)) => {
                if !is_valid_task_network_name(name) {
                    return Err(ArmorError::InvalidNetworkMode(format!(
                        "network_name must be 'armor-<taskId>' (alphanumeric/_/-), got '{}'",
                        name
                    )));
                }
                name.clone()
            }
            ("none", Some(_)) => {
                return Err(ArmorError::InvalidNetworkMode(
                    "network_name cannot be combined with network mode 'none'".into(),
                ));
            }
            (mode, _) => mode.to_string(),
        };

        let memory = config.memory_limit
            .unwrap_or(self.config.container_memory_mb * 1024 * 1024)
            .max(64 * 1024 * 1024);
        let cpu_shares = config.cpu_shares
            .unwrap_or(self.config.container_cpu_shares)
            .clamp(2, 4096);
        let pids_limit = config.pids_limit
            .unwrap_or(self.config.container_pids_limit)
            .clamp(10, 1000);

        let cap_drop = vec!["ALL".to_string()];
        let readonly_rootfs = true;
        let security_opt: Vec<String> = vec!["no-new-privileges".into()];

        let host_config = HostConfig {
            binds: if binds.is_empty() { None } else { Some(binds) },
            tmpfs: if tmpfs.is_empty() { None } else { Some(tmpfs) },
            network_mode: Some(docker_network_mode),
            memory: Some(memory),
            cpu_shares: Some(cpu_shares),
            pids_limit: Some(pids_limit),
            cap_drop: Some(cap_drop),
            readonly_rootfs: Some(readonly_rootfs),
            auto_remove: config.auto_remove,
            security_opt: Some(security_opt),
            ..Default::default()
        };

        let bollard_config = bollard::container::Config {
            image: Some(config.image.clone()),
            cmd,
            env,
            user: Some("opencode".into()),
            working_dir: config.working_dir.clone(),
            host_config: Some(host_config),
            ..Default::default()
        };

        Ok(bollard_config)
    }
}

/// Convenience alias — BollardRuntime IS the DockerManager/ContainerRuntime
pub type DockerManager = BollardRuntime;
