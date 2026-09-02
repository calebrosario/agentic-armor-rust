use crate::config::Config;
use crate::docker::types::*;
use crate::error::{ArmorError, ArmorResult};
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
        && name.len() > "armor-".len()
        && name.len() <= 64
        && name["armor-".len()..]
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

pub fn task_network_name(task_id: &str) -> String {
    format!("armor-{}", task_id)
}

pub fn docker_network_mode(config: &NetworkConfig, allow_host: bool) -> ArmorResult<String> {
    match config {
        NetworkConfig::None => Ok("none".into()),
        NetworkConfig::Bridge { network } => {
            if !is_valid_task_network_name(network) {
                return Err(ArmorError::InvalidNetworkMode(format!(
                    "network_name must be 'armor-<taskId>' (alphanumeric/_/-), got '{}'",
                    network
                )));
            }
            Ok(network.clone())
        }
        NetworkConfig::Host => {
            if allow_host {
                warn!("Host network mode allowed via ALLOW_HOST_NETWORK=true");
                Ok("host".into())
            } else {
                Err(ArmorError::InvalidNetworkMode("host".into()))
            }
        }
    }
}

pub fn is_pid_exhaustion_error(err: &ArmorError) -> bool {
    let msg = err.to_string().to_lowercase();
    msg.contains("procready not received")
        || msg.contains("cannot fork")
        || msg.contains("can't fork")
        || msg.contains("resource temporarily unavailable")
        || msg.contains("no space left on device")
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
            config
                .docker_socket
                .as_deref()
                .unwrap_or("/var/run/docker.sock"),
        )?;
        Ok(BollardRuntime {
            docker,
            config,
            runtime_name: "docker".into(),
        })
    }

    pub fn new_podman(config: Arc<Config>) -> ArmorResult<Self> {
        let socket = std::env::var("PODMAN_SOCKET").unwrap_or_else(|_| {
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
                    info!(
                        "Docker Desktop socket found at {} — using Docker runtime",
                        docker_desktop_socket
                    );
                    let mut new_config = (*config).clone();
                    new_config.docker_socket = Some(docker_desktop_socket);
                    return Self::new_docker(Arc::new(new_config));
                }

                let podman_socket = config
                    .podman_socket
                    .as_deref()
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
                    info!(
                        "Podman socket found at {} — using Podman runtime",
                        podman_socket
                    );
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
            Docker::connect_with_socket(socket, 120, bollard::API_DEFAULT_VERSION)
                .map_err(|e| ArmorError::DockerConnectionFailed(format!("{}: {}", socket, e)))
        } else {
            warn!("Socket not found: {} — trying default connection", socket);
            Docker::connect_with_local_defaults()
                .map_err(|e| ArmorError::DockerConnectionFailed(e.to_string()))
        }
    }
}

/// Wraps an exec command so the wrapper pid (== process-group id) is recorded to
/// `pid_file`. The payload runs as a child of the wrapper in the same process group:
/// on success the pidfile is removed and the exit status propagated. On timeout a
/// group kill (`kill -9 -PID`) is *attempted* against that group — it is
/// best-effort, not a guarantee: the caller verifies the outcome and reports it as
/// confirmed/unverified/undeliverable, and a payload that calls setsid can survive
/// its process group.
pub fn exec_wrap_command(pid_file: &str, command: &[String]) -> Vec<String> {
    let mut wrapped = vec![
        "sh".to_string(),
        "-c".to_string(),
        format!(
            "echo $$ > {f}; \"$@\" <&0 >&1 2>&2; s=$?; rm -f {f}; exit $s",
            f = pid_file
        ),
        "armor".to_string(),
    ];
    wrapped.extend(command.iter().cloned());
    wrapped
}

/// Attempts to kill the process group recorded in `pid_file` (SIGKILL) and removes
/// the file. Best-effort: errors are suppressed inside the command; the caller
/// verifies whether the exec actually stopped.
pub fn exec_kill_command(pid_file: &str) -> Vec<String> {
    vec![
        "sh".to_string(),
        "-c".to_string(),
        format!(
            "kill -9 -$(cat {}) 2>/dev/null; rm -f {}",
            pid_file, pid_file
        ),
    ]
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
        let bollard_config = Self::build_bollard_config(config, &self.config)?;

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
        self.docker.start_container::<&str>(id, None).await?;
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

    async fn exec_in_container(&self, id: &str, request: &ExecRequest) -> ArmorResult<ExecResult> {
        let start = Instant::now();

        let exec_nonce = uuid::Uuid::new_v4().simple().to_string();
        let pid_file = format!("/tmp/armor-exec-{}.pid", exec_nonce);
        let wrapped = exec_wrap_command(&pid_file, &request.command);

        let exec_config = CreateExecOptions {
            cmd: Some(wrapped),
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
        let mut exec_notes: Vec<String> = Vec::new();

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
                                    exec_notes.push(format!("[agentic-armor] exec stream error: {} — output may be truncated", e));
                                    break;
                                }
                            }
                        }
                    } => {}
                    _ = tokio::time::sleep(Duration::from_millis(timeout_ms)) => {
                        warn!("Exec timed out after {}ms — killing pid from {}", timeout_ms, pid_file);
                        let kill_cmd = exec_kill_command(&pid_file);
                        let kill_launched = match self.docker.create_exec(id, CreateExecOptions {
                            cmd: Some(kill_cmd),
                            attach_stdout: Some(true),
                            attach_stderr: Some(false),
                            ..Default::default()
                        }).await {
                            Ok(kill_exec) => self.docker.start_exec(&kill_exec.id, None).await.is_ok(),
                            Err(e) => {
                                warn!("kill exec creation failed after timeout: {}", e);
                                false
                            }
                        };
                        if !kill_launched {
                            warn!("kill exec could not be launched after timeout — the payload may still be running");
                        }
                        let mut kill_verified = false;
                        if kill_launched {
                            let poll_deadline = Instant::now() + Duration::from_secs(3);
                            while Instant::now() < poll_deadline {
                                match self.docker.inspect_exec(&exec_id).await {
                                    Ok(inspect) if inspect.running == Some(false) => {
                                        kill_verified = true;
                                        break;
                                    }
                                    Ok(_) => {}
                                    Err(e) => {
                                        warn!("kill verification inspect failed: {}", e);
                                        break;
                                    }
                                }
                                tokio::time::sleep(Duration::from_millis(100)).await;
                            }
                        }
                        exec_notes.push(if kill_verified {
                            format!("[agentic-armor] exec timed out after {}ms — timeout kill confirmed: the exec is no longer running", timeout_ms)
                        } else if kill_launched {
                            format!("[agentic-armor] exec timed out after {}ms — SIGKILL was sent but termination could NOT be verified; the payload may still be running inside the container", timeout_ms)
                        } else {
                            format!("[agentic-armor] exec timed out after {}ms — the kill command could NOT be delivered; the payload may still be running inside the container", timeout_ms)
                        });
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

        let exit_code = match self.docker.inspect_exec(&exec_id).await {
            Ok(inspect) => inspect.exit_code.unwrap_or(-1),
            Err(e) if !exec_notes.is_empty() => {
                warn!("post-exec inspect failed after notes were recorded: {}", e);
                exec_notes.push(format!("[agentic-armor] final status unavailable: {}", e));
                -1
            }
            Err(e) => return Err(ArmorError::Docker(e.to_string())),
        };
        if exit_code < 0 && exec_notes.is_empty() {
            exec_notes.push(
                "[agentic-armor] exec produced no exit code (killed or still running)".into(),
            );
        }
        let duration_ms = start.elapsed().as_millis() as u64;

        Ok(ExecResult {
            exit_code,
            stdout: String::from_utf8_lossy(&stdout_buf).to_string(),
            stderr: String::from_utf8_lossy(&stderr_buf).to_string(),
            notes: exec_notes,
            duration_ms,
        })
    }

    async fn is_running(&self, id: &str) -> ArmorResult<bool> {
        let info = self.docker.inspect_container(id, None).await?;
        Ok(info.state.and_then(|s| s.running).unwrap_or(false))
    }

    async fn create_network(&self, name: &str) -> ArmorResult<()> {
        if self
            .docker
            .inspect_network::<String>(name, None)
            .await
            .is_ok()
        {
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
        config: &ArmorContainerConfig,
        runtime_config: &Config,
    ) -> ArmorResult<bollard::container::Config<String>> {
        if !runtime_config
            .allowed_images
            .iter()
            .any(|img| img == &config.image)
        {
            return Err(ArmorError::ForbiddenMount("Image not allowed".to_string()));
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
                        let source_to_check =
                            if let Ok(canonical) = std::fs::canonicalize(&mount.source) {
                                canonical.to_string_lossy().to_lowercase()
                            } else {
                                mount.source.to_lowercase()
                            };
                        let target_lower = mount.target.to_lowercase();
                        for pattern in &runtime_config.forbidden_mount_patterns {
                            if source_to_check.contains(pattern) || target_lower.contains(pattern) {
                                warn!(
                                    "Security policy rejected mount '{}:{}'",
                                    mount.source, mount.target
                                );
                                return Err(ArmorError::ForbiddenMount(
                                    "Mount blocked by security policy".into(),
                                ));
                            }
                        }
                        let ro = mount.read_only.unwrap_or(false);
                        binds.push(format!(
                            "{}:{}{}",
                            mount.source,
                            mount.target,
                            if ro { ":ro" } else { "" }
                        ));
                    }
                    "tmpfs" => {
                        let opts = mount.tmpfs_options.clone().unwrap_or_default();
                        tmpfs.insert(mount.target.clone(), opts);
                    }
                    _ => {
                        return Err(ArmorError::InvalidMountConfig(format!(
                            "Unsupported mount type: '{}'",
                            mount.mount_type
                        )));
                    }
                }
            }
        }

        let network_mode = docker_network_mode(&config.network, runtime_config.allow_host_network)?;

        let memory = config
            .memory_limit
            .unwrap_or(runtime_config.container_memory_mb * 1024 * 1024)
            .max(512 * 1024 * 1024);
        let cpu_shares = config
            .cpu_shares
            .unwrap_or(runtime_config.container_cpu_shares)
            .clamp(2, 4096);
        let pids_limit = config
            .pids_limit
            .unwrap_or(runtime_config.container_pids_limit)
            .clamp(10, 1000);

        let userns_mode = match runtime_config.container_userns_mode.as_deref() {
            None | Some("") => None,
            Some(mode) => {
                let valid = mode.len() <= 64
                    && mode.chars().all(|c| {
                        c.is_ascii_lowercase()
                            || c.is_ascii_digit()
                            || c == '-'
                            || c == '_'
                            || c == '.'
                    });
                if !valid {
                    return Err(ArmorError::InvalidUsernsMode(mode.to_string()));
                }
                Some(mode.to_string())
            }
        };

        let cap_drop = vec!["ALL".to_string()];
        let readonly_rootfs = true;
        let security_opt: Vec<String> = vec!["no-new-privileges".into()];

        let host_config = HostConfig {
            binds: if binds.is_empty() { None } else { Some(binds) },
            tmpfs: if tmpfs.is_empty() { None } else { Some(tmpfs) },
            network_mode: Some(network_mode),
            memory: Some(memory),
            cpu_shares: Some(cpu_shares),
            pids_limit: Some(pids_limit),
            cap_drop: Some(cap_drop),
            readonly_rootfs: Some(readonly_rootfs),
            auto_remove: config.auto_remove,
            security_opt: Some(security_opt),
            userns_mode,
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
