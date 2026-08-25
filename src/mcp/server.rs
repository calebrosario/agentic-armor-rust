use crate::config::Config;
use crate::docker::{
    is_pid_exhaustion_error, task_network_name, ArmorContainerConfig, ContainerRuntime,
    ExecRequest, Mount, NetworkConfig,
};
use crate::error::{ArmorError, ArmorResult};
use crate::task::{TaskLifecycle, TaskRegistry};
use mcp_sdk::{CallToolResult, McpServer, StdioTransport, ToolBuilder};
use serde_json::json;
use std::sync::Arc;
use tracing::{error, info, warn};

pub async fn start(
    config: Arc<Config>,
    runtime: Arc<dyn ContainerRuntime>,
    registry: Arc<TaskRegistry>,
    lifecycle: Arc<TaskLifecycle>,
) -> ArmorResult<()> {
    let server = Arc::new(McpServer::new("agentic-armor", "0.4.0"));

    register_task_create(&server, &config, &runtime, &registry, &lifecycle).await;
    register_task_exec(&server, &runtime, &lifecycle, &registry).await;
    register_task_upload(&server, &runtime, &lifecycle, &config).await;
    register_task_download(&server, &runtime, &lifecycle, &config).await;
    register_task_list(&server, &registry).await;
    register_task_stop(&server, &runtime, &lifecycle).await;
    register_task_delete(&server, &runtime, &lifecycle).await;
    register_task_logs(&server, &registry).await;

    info!("Agentic Armor MCP server starting (8 tools, stdio)");
    StdioTransport::serve(server)
        .await
        .map_err(|e| ArmorError::Mcp(e.to_string()))?;
    Ok(())
}

async fn register_task_create(
    server: &Arc<McpServer>,
    config: &Arc<Config>,
    runtime: &Arc<dyn ContainerRuntime>,
    registry: &Arc<TaskRegistry>,
    lifecycle: &Arc<TaskLifecycle>,
) {
    let rt = runtime.clone();
    let reg = registry.clone();
    let lc = lifecycle.clone();
    let cfg = config.clone();

    server.register_tool(
        ToolBuilder::new("task_create")
            .description("Create a new task sandbox with a hardened Docker container. Network is disabled by default; pass network='bridge' only when the task needs package installs or git clone.")
            .schema(json!({
                "type": "object",
                "properties": {
                    "taskId": { "type": "string", "pattern": "^[a-zA-Z0-9_-]+$" },
                    "name": { "type": "string" },
                    "owner": { "type": "string" },
                    "image": { "type": "string" },
                    "blockNpmScripts": {
                        "type": "boolean",
                        "description": "When true, sets npm_config_ignore_scripts=1 in the container — package lifecycle scripts (pre/postinstall) will not run. Recommended when installing untrusted dependencies."
                    },
                    "network": {
                        "type": "string",
                        "enum": ["none", "bridge"],
                        "description": "Network mode. 'none' (default): no network access. 'bridge': outbound internet for package installs (npm install, git clone, pip install)."
                    }
                },
                "required": ["taskId", "name"]
            }))
            .handler(move |args| {
                let rt = rt.clone();
                let reg = reg.clone();
                let lc = lc.clone();
                let cfg = cfg.clone();
                async move {
                    let task_id = match arg_str(&args, "taskId") {
                        Ok(v) => v,
                        Err(e) => return Ok(CallToolResult::error(e)),
                    };

                    if task_id.is_empty() || task_id.len() > 128 ||
                       !task_id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
                        return Ok(CallToolResult::error("Invalid taskId: must match ^[a-zA-Z0-9_-]{1,128}$"));
                    }

                    let name = match arg_opt_str(&args, "name") {
                        Ok(v) => v.unwrap_or("task"),
                        Err(e) => return Ok(CallToolResult::error(e)),
                    };
                    let owner = match arg_opt_str(&args, "owner") {
                        Ok(v) => v,
                        Err(e) => return Ok(CallToolResult::error(e)),
                    };
                    let image = match arg_opt_str(&args, "image") {
                        Ok(v) => v.unwrap_or("opencode-sandbox-developer:latest"),
                        Err(e) => return Ok(CallToolResult::error(e)),
                    };
                    let block_npm_scripts = match args.get("blockNpmScripts") {
                        None => false,
                        Some(v) => match v.as_bool() {
                            Some(b) => b,
                            None => {
                                return Ok(CallToolResult::error(format!(
                                    "argument 'blockNpmScripts' must be a boolean, got {}",
                                    type_name(v)
                                )))
                            }
                        },
                    };
                    let network_mode = match arg_opt_str(&args, "network") {
                        Ok(v) => v.unwrap_or("none"),
                        Err(e) => return Ok(CallToolResult::error(e)),
                    };

                    if !is_valid_network_mode(network_mode) {
                        return Ok(CallToolResult::error("Invalid network mode: allowed values are 'none' and 'bridge'."));
                    }

                    if !cfg.allowed_images.iter().any(|img| img == image) {
                        return Ok(CallToolResult::error("Image not allowed. Use a pre-approved sandbox image."));
                    }

                    let existing = reg.list(1000).await.unwrap_or_default();
                    let active = existing.iter().filter(|t| {
                        matches!(t.status.as_str(), "pending" | "running")
                    }).count();
                    if active >= 10 {
                        return Ok(CallToolResult::error(
                            format!("Maximum concurrent containers (10) reached. Delete existing tasks first. Active: {}", active)
                        ));
                    }

                    let task = match lc.create_task(task_id, name, owner).await {
                        Ok(t) => t,
                        Err(e) => return Ok(CallToolResult::error(format!("Task creation failed: {}", e))),
                    };

                    let per_task_network = if network_mode == "bridge" {
                        Some(task_network_name(task_id))
                    } else {
                        None
                    };

                    if let Some(net) = &per_task_network {
                        if let Err(e) = rt.create_network(net).await {
                            error!("Network creation failed for task {}: {}", task_id, e);
                            rollback_task_create(&rt, &lc, task_id, None, None).await;
                            return Ok(CallToolResult::error(format!("Network creation failed: {}", e)));
                        }
                    }

                    let network = match &per_task_network {
                        Some(net) => NetworkConfig::Bridge { network: net.clone() },
                        None => NetworkConfig::None,
                    };

                    let container_config = ArmorContainerConfig {
                        name: format!("armor-{}", task.id),
                        image: image.to_string(),
                        command: Some(vec!["sleep".into(), "infinity".into()]),
                        network,
                        memory_limit: Some(cfg.container_memory_mb * 1024 * 1024),
                        cpu_shares: Some(cfg.container_cpu_shares),
                        pids_limit: Some(cfg.container_pids_limit),
                        readonly_rootfs: Some(true),
                        no_new_privileges: Some(true),
                        cap_drop: Some(vec!["ALL".into()]),
                        user: Some("opencode".into()),
                        env: npm_scripts_blocked_env(block_npm_scripts),
                        mounts: Some(default_task_mounts()),
                        ..Default::default()
                    };

                    let container_id = match rt.create_container(&container_config).await {
                        Ok(id) => {
                            if block_npm_scripts {
                                audit_event(&reg, task_id, "npm_scripts_blocked", "npm lifecycle scripts disabled via blockNpmScripts=true").await;
                            }
                            if network_mode == "bridge" {
                                warn!("Task {} created with network access (isolated bridge {})", task_id, task_network_name(task_id));
                                audit_event(&reg, task_id, "network_enabled", "Container created with isolated per-task bridge networking").await;
                            }
                            id
                        }
                        Err(e) => {
                            error!("Container creation failed for task {}: {}", task_id, e);
                            rollback_task_create(&rt, &lc, task_id, None, per_task_network.as_deref()).await;
                            return Ok(CallToolResult::error(format!("Container creation failed: {}", e)));
                        }
                    };

                    if let Err(e) = rt.start_container(&container_id).await {
                        error!("Container start failed for {}: {}", container_id, e);
                        rollback_task_create(&rt, &lc, task_id, Some(&container_id), per_task_network.as_deref()).await;
                        return Ok(CallToolResult::error(format!("Container start failed: {}", e)));
                    }

                    if let Err(e) = reg.set_container_id(task_id, &container_id).await {
                        error!("Failed to persist containerId: {}", e);
                        rollback_task_create(&rt, &lc, task_id, Some(&container_id), per_task_network.as_deref()).await;
                        return Ok(CallToolResult::error(format!("Failed to associate container: {}", e)));
                    }

                    audit_event(&reg, task_id, "container_created", &format!("Container {} started", container_id)).await;

                    Ok(CallToolResult::text(json!({
                        "success": true,
                        "taskId": task.id,
                        "name": task.name,
                        "containerId": container_id,
                        "status": "running"
                    }).to_string()))
                }
            }),
    ).await;
}

async fn register_task_exec(
    server: &Arc<McpServer>,
    runtime: &Arc<dyn ContainerRuntime>,
    lifecycle: &Arc<TaskLifecycle>,
    registry: &Arc<TaskRegistry>,
) {
    let rt = runtime.clone();
    let lc = lifecycle.clone();
    let reg = registry.clone();

    server.register_tool(
        ToolBuilder::new("task_exec")
            .description("Execute a command inside a task's Docker container")
            .schema(json!({
                "type": "object",
                "properties": {
                    "taskId": { "type": "string" },
                    "command": { "type": "array", "items": { "type": "string" } },
                    "timeout": { "type": "number" }
                },
                "required": ["taskId", "command"]
            }))
            .handler(move |args| {
                let rt = rt.clone();
                let lc = lc.clone();
                let reg = reg.clone();
                async move {
                    let task_id = match arg_str(&args, "taskId") {
                        Ok(v) => v,
                        Err(e) => return Ok(CallToolResult::error(e)),
                    };
                    let command = match arg_str_array(&args, "command") {
                        Ok(v) => v,
                        Err(e) => return Ok(CallToolResult::error(e)),
                    };
                    let timeout_ms = match arg_u64(&args, "timeout") {
                        Ok(v) => v,
                        Err(e) => return Ok(CallToolResult::error(e)),
                    };

                    let audit_cmd = audit_command(&command);

                    let container_id = match lc.get_container_id(task_id).await {
                        Ok(id) => id,
                        Err(e) => {
                            audit_event(&reg, task_id, "exec_logged", &format!("exec rejected (task not found): {}", audit_cmd)).await;
                            return Ok(CallToolResult::error(format!("Cannot find container: {}", e)));
                        }
                    };

                    let result = match rt.exec_in_container(&container_id, &ExecRequest {
                        command,
                        timeout_ms,
                        ..Default::default()
                    }).await {
                        Ok(r) => r,
                        Err(e) => {
                            let hint = if is_pid_exhaustion_error(&e) {
                                " (task resource budget exhausted — stop runaway processes inside the task, or task_delete and recreate it)"
                            } else {
                                ""
                            };
                            audit_event(&reg, task_id, "exec_logged", &format!("exec error ({}): {}", e, audit_cmd)).await;
                            return Ok(CallToolResult::error(format!("Exec failed: {}{}", e, hint)));
                        }
                    };

                    audit_event(&reg, task_id, "exec_logged", &format!("exec exit={} durMs={}: {}", result.exit_code, result.duration_ms, audit_cmd)).await;

                    let fork_hint = if result.stderr.contains("can't fork")
                        || result.stderr.contains("Cannot fork")
                        || result.stdout.contains("can't fork")
                    {
                        " — task resource budget exhausted: stop runaway processes inside the task, or task_delete and recreate it"
                    } else {
                        ""
                    };

                    let stderr = render_exec_stderr(&result.stderr, &result.notes, fork_hint);

                    Ok(CallToolResult::text(json!({
                        "success": result.exit_code == 0,
                        "exitCode": result.exit_code,
                        "stdout": result.stdout,
                        "stderr": stderr,
                        "durationMs": result.duration_ms
                    }).to_string()))
                }
            }),
    ).await;
}

async fn register_task_upload(
    server: &Arc<McpServer>,
    runtime: &Arc<dyn ContainerRuntime>,
    lifecycle: &Arc<TaskLifecycle>,
    config: &Arc<Config>,
) {
    let rt = runtime.clone();
    let lc = lifecycle.clone();
    let cfg = config.clone();

    server.register_tool(
        ToolBuilder::new("task_upload")
            .description("Write a file into a task's Docker container. Restricted to /tmp/, /home/opencode/, /workspace/. Max 10MB.")
            .schema(json!({
                "type": "object",
                "properties": {
                    "taskId": { "type": "string" },
                    "path": { "type": "string" },
                    "content": { "type": "string" }
                },
                "required": ["taskId", "path", "content"]
            }))
            .handler(move |args| {
                let rt = rt.clone();
                let lc = lc.clone();
                let cfg = cfg.clone();
                async move {
                    let (task_id, path, content) = match (arg_str(&args, "taskId"), arg_str(&args, "path"), arg_str(&args, "content")) {
                        (Ok(a), Ok(b), Ok(c)) => (a, b, c),
                        (Err(e), _, _) | (_, Err(e), _) | (_, _, Err(e)) => return Ok(CallToolResult::error(e)),
                    };

                    if let Err(e) = validate_path(path, &cfg) {
                        return Ok(CallToolResult::error(e));
                    }

                    if content.len() > 10 * 1024 * 1024 {
                        return Ok(CallToolResult::error("Content exceeds 10MB limit"));
                    }

                    let container_id = match lc.get_container_id(task_id).await {
                        Ok(id) => id,
                        Err(e) => return Ok(CallToolResult::error(format!("Cannot find container: {}", e))),
                    };

                    let resolved = match resolve_path_in_container(&rt, &container_id, path, false).await {
                        Ok(r) => r,
                        Err(e) => return Ok(CallToolResult::error(e)),
                    };
                    if let Err(e) = validate_path(&resolved, &cfg) {
                        return Ok(CallToolResult::error(format!("Path escapes allowed roots via symlink: {}", e)));
                    }

                    let b64 = base64_encode(content);
                    for script in upload_chunk_commands(&resolved, &b64) {
                        let result = match rt.exec_in_container(&container_id, &ExecRequest {
                            command: vec!["sh".into(), "-c".into(), script],
                            timeout_ms: Some(60_000),
                            ..Default::default()
                        }).await {
                            Ok(r) => r,
                            Err(e) => return Ok(CallToolResult::error(format!("Upload failed: {}", e))),
                        };
                        if result.exit_code != 0 {
                            return Ok(CallToolResult::error(result.stderr));
                        }
                    }

                    Ok(CallToolResult::text(json!({
                        "success": true,
                        "path": path,
                        "bytes": content.len()
                    }).to_string()))
                }
            }),
    ).await;
}

async fn register_task_download(
    server: &Arc<McpServer>,
    runtime: &Arc<dyn ContainerRuntime>,
    lifecycle: &Arc<TaskLifecycle>,
    config: &Arc<Config>,
) {
    let rt = runtime.clone();
    let lc = lifecycle.clone();
    let cfg = config.clone();

    server.register_tool(
        ToolBuilder::new("task_download")
            .description("Read a file from a task's Docker container. Restricted to /tmp/, /home/opencode/, /workspace/. Max 10MB.")
            .schema(json!({
                "type": "object",
                "properties": {
                    "taskId": { "type": "string" },
                    "path": { "type": "string" }
                },
                "required": ["taskId", "path"]
            }))
            .handler(move |args| {
                let rt = rt.clone();
                let lc = lc.clone();
                let cfg = cfg.clone();
                async move {
                    let (task_id, path) = match (arg_str(&args, "taskId"), arg_str(&args, "path")) {
                        (Ok(a), Ok(b)) => (a, b),
                        (Err(e), _) | (_, Err(e)) => return Ok(CallToolResult::error(e)),
                    };

                    if let Err(e) = validate_path(path, &cfg) {
                        return Ok(CallToolResult::error(e));
                    }

                    let container_id = match lc.get_container_id(task_id).await {
                        Ok(id) => id,
                        Err(e) => return Ok(CallToolResult::error(format!("Cannot find container: {}", e))),
                    };

                    let resolved = match resolve_path_in_container(&rt, &container_id, path, true).await {
                        Ok(r) => r,
                        Err(e) => return Ok(CallToolResult::error(e)),
                    };
                    if let Err(e) = validate_path(&resolved, &cfg) {
                        return Ok(CallToolResult::error(format!("Path escapes allowed roots via symlink: {}", e)));
                    }

                    let max_bytes = 10 * 1024 * 1024;
                    let result = match rt.exec_in_container(&container_id, &ExecRequest {
                        command: vec!["sh".into(), "-c".into(), format!("head -c {} {}", max_bytes, shell_quote(&resolved))],
                        timeout_ms: Some(30_000),
                        ..Default::default()
                    }).await {
                        Ok(r) => r,
                        Err(e) => return Ok(CallToolResult::error(format!("Download failed: {}", e))),
                    };

                    if result.exit_code != 0 {
                        return Ok(CallToolResult::error(result.stderr));
                    }

                    let bytes = result.stdout.len();
                    let truncated = bytes >= max_bytes;

                    Ok(CallToolResult::text(json!({
                        "success": true,
                        "path": path,
                        "content": result.stdout,
                        "bytes": bytes,
                        "truncated": truncated
                    }).to_string()))
                }
            }),
    ).await;
}

async fn register_task_list(server: &Arc<McpServer>, registry: &Arc<TaskRegistry>) {
    let reg = registry.clone();

    server
        .register_tool(
            ToolBuilder::new("task_list")
                .description("List all tasks")
                .schema(json!({
                    "type": "object",
                    "properties": {
                        "limit": { "type": "number" }
                    }
                }))
                .handler(move |args| {
                    let reg = reg.clone();
                    async move {
                        let limit = match args.get("limit") {
                            None => 100,
                            Some(v) => match v.as_i64() {
                                Some(n) => n.clamp(1, 1000),
                                None => {
                                    return Ok(CallToolResult::error(format!(
                                        "argument 'limit' must be an integer, got {}",
                                        type_name(v)
                                    )))
                                }
                            },
                        };
                        let tasks = match reg.list(limit).await {
                            Ok(t) => t,
                            Err(e) => {
                                return Ok(CallToolResult::error(format!("Database error: {}", e)))
                            }
                        };

                        Ok(CallToolResult::text(
                            json!({
                                "tasks": tasks.iter().map(|t| json!({
                                    "id": t.id,
                                    "name": t.name,
                                    "status": format!("{:?}", t.status).to_lowercase(),
                                    "owner": t.owner,
                                    "createdAt": t.created_at
                                })).collect::<Vec<_>>(),
                                "count": tasks.len()
                            })
                            .to_string(),
                        ))
                    }
                }),
        )
        .await;
}

async fn register_task_stop(
    server: &Arc<McpServer>,
    runtime: &Arc<dyn ContainerRuntime>,
    lifecycle: &Arc<TaskLifecycle>,
) {
    let rt = runtime.clone();
    let lc = lifecycle.clone();

    server
        .register_tool(
            ToolBuilder::new("task_stop")
                .description("Stop a running task and cancel it")
                .schema(json!({
                    "type": "object",
                    "properties": { "taskId": { "type": "string" } },
                    "required": ["taskId"]
                }))
                .handler(move |args| {
                    let rt = rt.clone();
                    let lc = lc.clone();
                    async move {
                        let task_id = match arg_str(&args, "taskId") {
                            Ok(v) => v,
                            Err(e) => return Ok(CallToolResult::error(e)),
                        };

                        if let Ok(container_id) = lc.get_container_id(task_id).await {
                            if let Err(e) = rt.stop_container(&container_id, 10).await {
                                warn!("Failed to stop container {}: {}", container_id, e);
                            }
                        }

                        let task = match lc.cancel_task(task_id).await {
                            Ok(t) => t,
                            Err(e) => {
                                return Ok(CallToolResult::error(format!("Cancel failed: {}", e)))
                            }
                        };

                        Ok(CallToolResult::text(
                            json!({
                                "success": true,
                                "taskId": task.id,
                                "status": "cancelled"
                            })
                            .to_string(),
                        ))
                    }
                }),
        )
        .await;
}

async fn register_task_delete(
    server: &Arc<McpServer>,
    runtime: &Arc<dyn ContainerRuntime>,
    lifecycle: &Arc<TaskLifecycle>,
) {
    let rt = runtime.clone();
    let lc = lifecycle.clone();

    server.register_tool(
        ToolBuilder::new("task_delete")
            .description("Delete a task and destroy its container + network. Returns alreadyGone=true when the taskId never existed (idempotent); audit events are retained")
            .schema(json!({
                "type": "object",
                "properties": { "taskId": { "type": "string" } },
                "required": ["taskId"]
            }))
            .handler(move |args| {
                let rt = rt.clone();
                let lc = lc.clone();
                async move {
                    let task_id = match arg_str(&args, "taskId") {
                        Ok(v) => v,
                        Err(e) => return Ok(CallToolResult::error(e)),
                    };

                    let container_id = lc.get_container_id(task_id).await.ok();
                    let had_task = lc.get_task(task_id).await.is_ok();

                    if let Some(container_id) = container_id.as_deref() {
                        if let Err(e) = rt.destroy_container(container_id).await {
                            warn!("Failed to destroy container {} for task {}: {} — container may still be running on the host", container_id, task_id, e);
                        }
                    } else {
                        let orphan_name = format!("armor-{}", task_id);
                        if let Err(e) = rt.destroy_container(&orphan_name).await {
                            if !e.to_string().to_lowercase().contains("no such container") {
                                warn!("Failed to clean up potential orphan container {}: {}", orphan_name, e);
                            }
                        }
                    }
                    if let Err(e) = rt.remove_network(&task_network_name(task_id)).await {
                        if !e.to_string().to_lowercase().contains("no such network") {
                            warn!("Failed to remove network for task {}: {}", task_id, e);
                        }
                    }

                    if let Err(e) = lc.delete_task(task_id).await {
                        return Ok(CallToolResult::error(format!("Delete failed: {}", e)));
                    }

                    Ok(CallToolResult::text(json!({
                        "success": true,
                        "taskId": task_id,
                        "alreadyGone": !had_task
                    }).to_string()))
                }
            }),
    ).await;
}

async fn register_task_logs(server: &Arc<McpServer>, registry: &Arc<TaskRegistry>) {
    let reg = registry.clone();

    server
        .register_tool(
            ToolBuilder::new("task_logs")
                .description("Retrieve execution logs for a task")
                .schema(json!({
                    "type": "object",
                    "properties": {
                        "taskId": { "type": "string" },
                        "limit": { "type": "number" }
                    },
                    "required": ["taskId"]
                }))
                .handler(move |args| {
                    let reg = reg.clone();
                    async move {
                        let task_id = match arg_str(&args, "taskId") {
                            Ok(v) => v,
                            Err(e) => return Ok(CallToolResult::error(e)),
                        };
                        let limit = match args.get("limit") {
                            None => 100,
                            Some(v) => match v.as_i64() {
                                Some(n) => n.clamp(1, 1000),
                                None => {
                                    return Ok(CallToolResult::error(format!(
                                        "argument 'limit' must be an integer, got {}",
                                        type_name(v)
                                    )))
                                }
                            },
                        };

                        let logs = match reg.get_logs(task_id, limit).await {
                            Ok(l) => l,
                            Err(e) => {
                                return Ok(CallToolResult::error(format!("Database error: {}", e)))
                            }
                        };

                        Ok(CallToolResult::text(
                            json!({
                                "taskId": task_id,
                                "count": logs.len(),
                                "logs": logs
                            })
                            .to_string(),
                        ))
                    }
                }),
        )
        .await;
}

async fn rollback_task_create(
    rt: &Arc<dyn ContainerRuntime>,
    lc: &TaskLifecycle,
    task_id: &str,
    container_id: Option<&str>,
    network_name: Option<&str>,
) {
    if let Some(cid) = container_id {
        if let Err(e) = rt.destroy_container(cid).await {
            warn!(
                "Rollback: container destroy failed for {}: {} — container may remain on host",
                cid, e
            );
        }
    }
    if let Some(net) = network_name {
        if let Err(e) = rt.remove_network(net).await {
            warn!(
                "Rollback: network removal failed for {}: {} — orphaned network, remove manually",
                net, e
            );
        }
    }
    if let Err(e) = lc.delete_task(task_id).await {
        error!("Rollback failed: task {} row not removed ({}) — it now counts toward the concurrency cap; delete it manually", task_id, e);
    }
}

pub fn task_tmpfs_options(size_mb: usize) -> String {
    format!("size={}m,uid=1001,gid=1001,mode=0775", size_mb)
}

pub fn default_task_mounts() -> Vec<Mount> {
    vec![
        Mount {
            source: "".into(),
            target: "/tmp".into(),
            mount_type: "tmpfs".into(),
            read_only: None,
            tmpfs_options: Some(task_tmpfs_options(64)),
        },
        Mount {
            source: "".into(),
            target: "/home/opencode".into(),
            mount_type: "tmpfs".into(),
            read_only: None,
            tmpfs_options: Some(task_tmpfs_options(64)),
        },
        Mount {
            source: "".into(),
            target: "/workspace".into(),
            mount_type: "tmpfs".into(),
            read_only: None,
            tmpfs_options: Some(task_tmpfs_options(256)),
        },
    ]
}

pub fn is_valid_network_mode(mode: &str) -> bool {
    matches!(mode, "none" | "bridge")
}

pub fn validate_path(path: &str, config: &Config) -> Result<(), String> {
    if !path.starts_with('/') {
        return Err("Path must be absolute".into());
    }
    if path.contains("..") {
        return Err("Path traversal (..) not allowed".into());
    }
    if !path.chars().all(|c| {
        c.is_ascii_alphanumeric() || c == '/' || c == '.' || c == '_' || c == '@' || c == '-'
    }) {
        return Err("Path contains invalid characters".into());
    }
    if !config
        .allowed_path_prefixes
        .iter()
        .any(|prefix| path.starts_with(prefix.as_str()))
    {
        return Err(format!(
            "Path must be under one of: {:?}",
            config.allowed_path_prefixes
        ));
    }
    Ok(())
}

pub fn arg_str<'a>(args: &'a serde_json::Value, key: &str) -> Result<&'a str, String> {
    match args.get(key) {
        None => Err(format!("missing required argument: {}", key)),
        Some(v) => v
            .as_str()
            .ok_or_else(|| format!("argument '{}' must be a string, got {}", key, type_name(v))),
    }
}

pub fn arg_opt_str<'a>(args: &'a serde_json::Value, key: &str) -> Result<Option<&'a str>, String> {
    match args.get(key) {
        None => Ok(None),
        Some(v) => v
            .as_str()
            .map(Some)
            .ok_or_else(|| format!("argument '{}' must be a string, got {}", key, type_name(v))),
    }
}

pub fn arg_str_array(args: &serde_json::Value, key: &str) -> Result<Vec<String>, String> {
    match args.get(key) {
        None => Err(format!("missing required argument: {}", key)),
        Some(v) => v
            .as_array()
            .ok_or_else(|| format!("argument '{}' must be an array, got {}", key, type_name(v)))
            .and_then(|a| {
                a.iter()
                    .enumerate()
                    .map(|(i, e)| {
                        e.as_str().map(String::from).ok_or_else(|| {
                            format!(
                                "argument '{}'[{}] must be a string, got {}",
                                key,
                                i,
                                type_name(e)
                            )
                        })
                    })
                    .collect()
            }),
    }
}

pub fn arg_u64(args: &serde_json::Value, key: &str) -> Result<Option<u64>, String> {
    match args.get(key) {
        None => Ok(None),
        Some(v) => v.as_u64().map(Some).ok_or_else(|| {
            format!(
                "argument '{}' must be a non-negative integer, got {}",
                key,
                type_name(v)
            )
        }),
    }
}

pub fn type_name(v: &serde_json::Value) -> &'static str {
    match v {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "boolean",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}

/// Container env that disables npm lifecycle scripts (pre/postinstall) when requested.
pub fn npm_scripts_blocked_env(blocked: bool) -> Option<Vec<String>> {
    if blocked {
        Some(vec!["NPM_CONFIG_IGNORE_SCRIPTS=1".into()])
    } else {
        None
    }
}

async fn audit_event(reg: &TaskRegistry, task_id: &str, event_type: &str, message: &str) {
    if let Err(e) = reg.add_event(task_id, event_type, message).await {
        warn!(
            "AUDIT WRITE FAILED for task {} ({}): {} — audit trail is incomplete",
            task_id, event_type, e
        );
    }
}

pub fn base64_encode(input: &str) -> String {
    use std::fmt::Write;
    let mut result = String::with_capacity(input.len().div_ceil(3) * 4);
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let bytes = input.as_bytes();
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let n = (b0 << 16) | (b1 << 8) | b2;
        let _ = write!(&mut result, "{}", CHARS[((n >> 18) & 63) as usize] as char);
        let _ = write!(&mut result, "{}", CHARS[((n >> 12) & 63) as usize] as char);
        if chunk.len() > 1 {
            let _ = write!(&mut result, "{}", CHARS[((n >> 6) & 63) as usize] as char);
        } else {
            result.push('=');
        }
        if chunk.len() > 2 {
            let _ = write!(&mut result, "{}", CHARS[(n & 63) as usize] as char);
        } else {
            result.push('=');
        }
    }
    result
}

pub fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

pub fn render_exec_stderr(stderr: &str, notes: &[String], fork_hint: &str) -> String {
    let mut parts: Vec<String> = Vec::with_capacity(3);
    if !stderr.is_empty() {
        parts.push(stderr.to_string());
    }
    parts.extend(notes.iter().cloned());
    if !fork_hint.is_empty() {
        parts.push(fork_hint.trim().to_string());
    }
    parts.join("\n")
}

pub fn audit_command(command: &[String]) -> String {
    command.join(" ").chars().take(512).collect()
}

async fn resolve_path_in_container(
    rt: &Arc<dyn ContainerRuntime>,
    container_id: &str,
    path: &str,
    follow_final: bool,
) -> Result<String, String> {
    let script = if follow_final {
        format!("readlink -f {}", shell_quote(path))
    } else {
        format!(
            "p=\"$(dirname {})\"; tail=''; while [ \"$p\" != / ] && [ ! -e \"$p\" ]; do tail=\"/$(basename \"$p\")$tail\"; p=$(dirname \"$p\"); done; echo \"$(readlink -f \"$p\")$tail/$(basename {})\"",
            shell_quote(path),
            shell_quote(path)
        )
    };
    let result = rt
        .exec_in_container(
            container_id,
            &ExecRequest {
                command: vec!["sh".into(), "-c".into(), script],
                timeout_ms: Some(10_000),
                ..Default::default()
            },
        )
        .await
        .map_err(|e| format!("Path resolution failed: {}", e))?;
    if result.exit_code != 0 {
        return Err(format!("Path resolution failed: {}", result.stderr.trim()));
    }
    Ok(result.stdout.trim().to_string())
}

const UPLOAD_CHUNK_BYTES: usize = 48 * 1024;

pub fn upload_chunk_commands(resolved_path: &str, b64: &str) -> Vec<String> {
    let quoted = shell_quote(resolved_path);
    let symlink_guard = format!(
        "[ ! -L {} ] || {{ echo 'refusing to write through symlink' >&2; exit 1; }}",
        quoted
    );
    if b64.is_empty() {
        return vec![format!("{} && : > {}", symlink_guard, quoted)];
    }
    b64.as_bytes()
        .chunks(UPLOAD_CHUNK_BYTES)
        .enumerate()
        .map(|(i, chunk)| {
            let redirect = if i == 0 { ">" } else { ">>" };
            format!(
                "{} && mkdir -p \"$(dirname {})\" && printf %s '{}' | base64 -d {} {} || {{ rm -f {}; exit 1; }}",
                if i == 0 { &symlink_guard } else { "true" },
                quoted,
                std::str::from_utf8(chunk).unwrap_or(""),
                redirect,
                quoted,
                quoted
            )
        })
        .collect()
}
