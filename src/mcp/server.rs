use crate::config::Config;
use crate::docker::{task_network_name, ArmorContainerConfig, ContainerRuntime, ExecRequest, Mount};
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
    StdioTransport::serve(server).await.map_err(|e| ArmorError::Mcp(e.to_string()))?;
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
                    let task_id = args.get("taskId").and_then(|v| v.as_str()).unwrap_or("unknown");

                    if task_id.is_empty() || task_id.len() > 128 ||
                       !task_id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
                        return Ok(CallToolResult::error("Invalid taskId: must match ^[a-zA-Z0-9_-]{1,128}$"));
                    }

                    let name = args.get("name").and_then(|v| v.as_str()).unwrap_or("task");
                    let owner = args.get("owner").and_then(|v| v.as_str());
                    let image = args.get("image").and_then(|v| v.as_str()).unwrap_or("opencode-sandbox-developer:latest");
                    let network_mode = args.get("network").and_then(|v| v.as_str()).unwrap_or("none");

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
                            let _ = lc.delete_task(task_id).await;
                            return Ok(CallToolResult::error(format!("Network creation failed: {}", e)));
                        }
                    }

                    let container_config = ArmorContainerConfig {
                        name: format!("armor-{}", task.id),
                        image: image.to_string(),
                        command: Some(vec!["sleep".into(), "infinity".into()]),
                        network_mode: Some(network_mode.into()),
                        network_name: per_task_network.clone(),
                        memory_limit: Some(cfg.container_memory_mb * 1024 * 1024),
                        cpu_shares: Some(cfg.container_cpu_shares),
                        pids_limit: Some(cfg.container_pids_limit),
                        readonly_rootfs: Some(true),
                        no_new_privileges: Some(true),
                        cap_drop: Some(vec!["ALL".into()]),
                        user: Some("opencode".into()),
                        mounts: Some(default_task_mounts()),
                        ..Default::default()
                    };

                    let container_id = match rt.create_container(&container_config).await {
                        Ok(id) => {
                            if network_mode == "bridge" {
                                warn!("Task {} created with network access (isolated bridge {})", task_id, task_network_name(task_id));
                                reg.add_event(task_id, "network_enabled", "Container created with isolated per-task bridge networking").await.ok();
                            }
                            id
                        }
                        Err(e) => {
                            error!("Container creation failed for task {}: {}", task_id, e);
                            if let Some(net) = &per_task_network {
                                let _ = rt.remove_network(net).await;
                            }
                            let _ = lc.delete_task(task_id).await;
                            return Ok(CallToolResult::error(format!("Container creation failed: {}", e)));
                        }
                    };

                    if let Err(e) = rt.start_container(&container_id).await {
                        error!("Container start failed for {}: {}", container_id, e);
                        let _ = rt.destroy_container(&container_id).await;
                        if let Some(net) = &per_task_network {
                            let _ = rt.remove_network(net).await;
                        }
                        let _ = lc.delete_task(task_id).await;
                        return Ok(CallToolResult::error(format!("Container start failed: {}", e)));
                    }

                    if let Err(e) = reg.set_container_id(task_id, &container_id).await {
                        error!("Failed to persist containerId: {}", e);
                        let _ = rt.destroy_container(&container_id).await;
                        if let Some(net) = &per_task_network {
                            let _ = rt.remove_network(net).await;
                        }
                        let _ = lc.delete_task(task_id).await;
                        return Ok(CallToolResult::error(format!("Failed to associate container: {}", e)));
                    }

                    reg.add_event(task_id, "container_created", &format!("Container {} started", container_id)).await.ok();

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
                    let task_id = args.get("taskId").and_then(|v| v.as_str()).unwrap_or("");
                    let command: Vec<String> = args.get("command")
                        .and_then(|v| v.as_array())
                        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                        .unwrap_or_default();
                    let timeout_ms = args.get("timeout").and_then(|v| v.as_u64());

                    let audit_cmd: String = {
                        let joined = command.join(" ");
                        joined.chars().take(512).collect()
                    };

                    let container_id = match lc.get_container_id(task_id).await {
                        Ok(id) => id,
                        Err(e) => {
                            reg.add_event(task_id, "exec_logged", &format!("exec rejected (task not found): {}", audit_cmd)).await.ok();
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
                            reg.add_event(task_id, "exec_logged", &format!("exec error ({}): {}", e, audit_cmd)).await.ok();
                            return Ok(CallToolResult::error(format!("Exec failed: {}", e)));
                        }
                    };

                    reg.add_event(task_id, "exec_logged", &format!("exec exit={} durMs={}: {}", result.exit_code, result.duration_ms, audit_cmd)).await.ok();

                    Ok(CallToolResult::text(json!({
                        "success": result.exit_code == 0,
                        "exitCode": result.exit_code,
                        "stdout": result.stdout,
                        "stderr": result.stderr,
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
                    let task_id = args.get("taskId").and_then(|v| v.as_str()).unwrap_or("");
                    let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("");
                    let content = args.get("content").and_then(|v| v.as_str()).unwrap_or("");

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
                    let task_id = args.get("taskId").and_then(|v| v.as_str()).unwrap_or("");
                    let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("");

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
                        command: vec!["sh".into(), "-c".into(), format!("head -c {} '{}'", max_bytes, resolved)],
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

    server.register_tool(
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
                    let limit = args.get("limit").and_then(|v| v.as_i64()).unwrap_or(100).min(1000).max(1);
                    let tasks = match reg.list(limit).await {
                        Ok(t) => t,
                        Err(e) => return Ok(CallToolResult::error(format!("Database error: {}", e))),
                    };

                    Ok(CallToolResult::text(json!({
                        "tasks": tasks.iter().map(|t| json!({
                            "id": t.id,
                            "name": t.name,
                            "status": format!("{:?}", t.status).to_lowercase(),
                            "owner": t.owner,
                            "createdAt": t.created_at
                        })).collect::<Vec<_>>(),
                        "count": tasks.len()
                    }).to_string()))
                }
            }),
    ).await;
}

async fn register_task_stop(server: &Arc<McpServer>, runtime: &Arc<dyn ContainerRuntime>, lifecycle: &Arc<TaskLifecycle>) {
    let rt = runtime.clone();
    let lc = lifecycle.clone();

    server.register_tool(
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
                    let task_id = args.get("taskId").and_then(|v| v.as_str()).unwrap_or("");

                    if let Ok(container_id) = lc.get_container_id(task_id).await {
                        if let Err(e) = rt.stop_container(&container_id, 10).await {
                            warn!("Failed to stop container {}: {}", container_id, e);
                        }
                    }

                    let task = match lc.cancel_task(task_id).await {
                        Ok(t) => t,
                        Err(e) => return Ok(CallToolResult::error(format!("Cancel failed: {}", e))),
                    };

                    Ok(CallToolResult::text(json!({
                        "success": true,
                        "taskId": task.id,
                        "status": "cancelled"
                    }).to_string()))
                }
            }),
    ).await;
}

async fn register_task_delete(server: &Arc<McpServer>, runtime: &Arc<dyn ContainerRuntime>, lifecycle: &Arc<TaskLifecycle>) {
    let rt = runtime.clone();
    let lc = lifecycle.clone();

    server.register_tool(
        ToolBuilder::new("task_delete")
            .description("Delete a task and destroy its container")
            .schema(json!({
                "type": "object",
                "properties": { "taskId": { "type": "string" } },
                "required": ["taskId"]
            }))
            .handler(move |args| {
                let rt = rt.clone();
                let lc = lc.clone();
                async move {
                    let task_id = args.get("taskId").and_then(|v| v.as_str()).unwrap_or("");

                    if let Ok(container_id) = lc.get_container_id(task_id).await {
                        if let Err(e) = rt.destroy_container(&container_id).await {
                            warn!("Failed to destroy container {}: {}", container_id, e);
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
                        "taskId": task_id
                    }).to_string()))
                }
            }),
    ).await;
}

async fn register_task_logs(server: &Arc<McpServer>, registry: &Arc<TaskRegistry>) {
    let reg = registry.clone();

    server.register_tool(
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
                    let task_id = args.get("taskId").and_then(|v| v.as_str()).unwrap_or("");
                    let limit = args.get("limit").and_then(|v| v.as_i64()).unwrap_or(100).min(1000).max(1);

                    let logs = match reg.get_logs(task_id, limit).await {
                        Ok(l) => l,
                        Err(e) => return Ok(CallToolResult::error(format!("Database error: {}", e))),
                    };

                    Ok(CallToolResult::text(json!({
                        "taskId": task_id,
                        "count": logs.len(),
                        "logs": logs
                    }).to_string()))
                }
            }),
    ).await;
}

pub fn default_task_mounts() -> Vec<Mount> {
    vec![
        Mount { source: "".into(), target: "/tmp".into(), mount_type: "tmpfs".into(), read_only: None, tmpfs_options: Some("size=64m,uid=1001,gid=1001,mode=0775".into()) },
        Mount { source: "".into(), target: "/home/opencode".into(), mount_type: "tmpfs".into(), read_only: None, tmpfs_options: Some("size=64m,uid=1001,gid=1001,mode=0775".into()) },
        Mount { source: "".into(), target: "/workspace".into(), mount_type: "tmpfs".into(), read_only: None, tmpfs_options: Some("size=256m,uid=1001,gid=1001,mode=0775".into()) },
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
    if !path.chars().all(|c| c.is_ascii_alphanumeric() || c == '/' || c == '.' || c == '_' || c == '@' || c == '-') {
        return Err("Path contains invalid characters".into());
    }
    if !config.allowed_path_prefixes.iter().any(|prefix| path.starts_with(prefix.as_str())) {
        return Err(format!("Path must be under one of: {:?}", config.allowed_path_prefixes));
    }
    Ok(())
}

pub fn base64_encode(input: &str) -> String {
    use std::fmt::Write;
    let mut result = String::with_capacity((input.len() + 2) / 3 * 4);
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

async fn resolve_path_in_container(
    rt: &Arc<dyn ContainerRuntime>,
    container_id: &str,
    path: &str,
    follow_final: bool,
) -> Result<String, String> {
    let script = if follow_final {
        format!("readlink -f '{}'", path)
    } else {
        format!(
            "p=\"$(dirname '{}')\"; tail=''; while [ \"$p\" != / ] && [ ! -e \"$p\" ]; do tail=\"/$(basename \"$p\")$tail\"; p=$(dirname \"$p\"); done; echo \"$(readlink -f \"$p\")$tail/$(basename '{}')\"",
            path, path
        )
    };
    let result = rt
        .exec_in_container(container_id, &ExecRequest {
            command: vec!["sh".into(), "-c".into(), script],
            timeout_ms: Some(10_000),
            ..Default::default()
        })
        .await
        .map_err(|e| format!("Path resolution failed: {}", e))?;
    if result.exit_code != 0 {
        return Err(format!("Path resolution failed: {}", result.stderr.trim()));
    }
    Ok(result.stdout.trim().to_string())
}

const UPLOAD_CHUNK_BYTES: usize = 48 * 1024;

pub fn upload_chunk_commands(resolved_path: &str, b64: &str) -> Vec<String> {
    let symlink_guard = format!(
        "[ ! -L '{}' ] || {{ echo 'refusing to write through symlink' >&2; exit 1; }}",
        resolved_path
    );
    if b64.is_empty() {
        return vec![format!("{} && : > '{}'", symlink_guard, resolved_path)];
    }
    b64.as_bytes()
        .chunks(UPLOAD_CHUNK_BYTES)
        .enumerate()
        .map(|(i, chunk)| {
            let redirect = if i == 0 { ">" } else { ">>" };
            format!(
                "{} && mkdir -p \"$(dirname '{}')\" && printf %s '{}' | base64 -d {} '{}' || {{ rm -f '{}'; exit 1; }}",
                if i == 0 { &symlink_guard } else { "true" },
                resolved_path,
                std::str::from_utf8(chunk).unwrap_or(""),
                redirect,
                resolved_path,
                resolved_path
            )
        })
        .collect()
}
