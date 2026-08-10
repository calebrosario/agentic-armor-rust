# Agentic Armor (Rust)

> Rust port of agentic-armor — hardened container sandbox execution for AI agents via MCP. Supports both Docker and Podman.

## Container Runtime Support

agentic-armor-rust supports **both Docker and Podman** through a unified `ContainerRuntime` trait. Both runtimes share the same bollard backend (Podman 4+ provides Docker-compatible API).

### Auto-Detection (Default)

```bash
cargo run
# Auto-detects Docker socket (/var/run/docker.sock) first
# Falls back to Podman socket (/run/user/$UID/podman/podman.sock)
```

### Force Docker

```bash
CONTAINER_RUNTIME=docker cargo run
```

### Force Podman

```bash
CONTAINER_RUNTIME=podman cargo run

# Or specify custom Podman socket:
PODMAN_SOCKET=/custom/path/podman.sock CONTAINER_RUNTIME=podman cargo run
```

### Starting Podman Socket

```bash
# Rootless (non-root user) — recommended for security
systemctl --user start podman.socket
systemctl --user enable podman.socket  # auto-start on boot

# Rootful
sudo systemctl start podman.socket

# Verify
curl --unix-socket /run/user/$UID/podman/podman.sock http://localhost/v5.0.0/libpod/info
```

## Docker vs Podman: Security Comparison

Podman is **recommended for production** due to its daemonless, rootless architecture. Both runtimes work identically through the unified `ContainerRuntime` trait.

| Security Property | Docker | Podman Rootless |
|--------------------|--------|-----------------|
| Architecture | Root daemon (`dockerd`) — persistent attack target | No daemon — each container is a separate process |
| Socket compromise | **Full root on host** — can mount host FS, create privileged containers | **User-scoped only** — cannot access root files, cannot escalate |
| Container root maps to | Host root (UID 0) | Unprivileged host user (UID 100000+) via user namespaces |
| Can read `~/.ssh/id_rsa`? | Yes (if socket compromised) | No (user-scoped) |
| Can install malware? | Yes (root access) | No (no sudo) |
| SELinux integration | Available but often unconfigured | Enabled by default on RHEL/Fedora |
| Daemon process | Always running as root | No daemon |

### The Docker Socket Attack Vector

agentic-armor blocks mounting `docker.sock` via pattern matching + canonicalization. But defense-in-depth matters:

```
Docker (if socket compromised):
  Agent → docker.sock → create privileged container → mount / → read everything

Podman rootless (if socket compromised):
  Agent → podman.sock → can only manage that user's containers → stuck at user level
```

### Recommendation

- **Linux servers / production:** Use Podman (`CONTAINER_RUNTIME=podman`). Rootless socket eliminates the highest-severity escape vector.
- **macOS / development:** Docker Desktop is fine — both run inside VMs so the security difference is smaller.
- **CI / untrusted workloads:** Always Podman rootless.

## Configuration

| Env Var | Default | Description |
|---------|---------|-------------|
| `CONTAINER_RUNTIME` | `auto` | `docker`, `podman`, or `auto` |
| `DOCKER_SOCKET` | `/var/run/docker.sock` | Docker socket path |
| `PODMAN_SOCKET` | Auto-detected | Podman socket path |
| `DATABASE_URL` | `postgresql://localhost:5433/agentic_armor` | PostgreSQL connection |
| `CONTAINER_MEMORY_MB` | `512` | Default memory limit per container |
| `CONTAINER_CPU_SHARES` | `1024` | Default CPU shares |
| `CONTAINER_PIDS_LIMIT` | `100` | Default PID limit |
| `ALLOW_HOST_NETWORK` | `false` | Allow `networkMode: host` |

## Security Defaults

Every container gets:
- `cap-drop: ALL` — no Linux capabilities
- `readonlyRootfs: true` — read-only root filesystem
- `noNewPrivileges: true` — no privilege escalation
- `NetworkMode: none` — no network access by default
- `Memory: 512MB`, `PidsLimit: 100`, `CpuShares: 1024`
- Docker socket mount prevention (source + target)
- Image allowlist enforcement

## Build & Test

```bash
cargo build
cargo test          # 16 tests
cargo run           # Run test container lifecycle
```

---

## Installation

### Prerequisites

- **Docker** or **Podman** (for container execution)
- **PostgreSQL** 14+ (for task persistence)
- **Rust** 1.85+ (to build from source)

### Install for Humans

**Step 1: Build the binary**

```bash
git clone https://github.com/calebrosario/rust-mcp-sdk.git
git clone https://github.com/calebrosario/agentic-armor-rust.git
cd agentic-armor-rust
cargo build --release
```

The binary is at `target/release/agentic-armor` (5.9MB, no runtime dependencies).

**Step 2: Start PostgreSQL**

```bash
docker run -d --name armor-pg \
  -e POSTGRES_USER=opencode \
  -e POSTGRES_PASSWORD=opencode \
  -e POSTGRES_DB=agentic_armor \
  -p 5433:5432 \
  postgres:16-alpine
```

**Step 3: Set environment variables**

```bash
export DATABASE_URL="postgresql://opencode:opencode@localhost:5433/agentic_armor"
export CONTAINER_RUNTIME=auto    # docker, podman, or auto
```

Add these to your `~/.zshrc` or `~/.bashrc` for persistence.

**Step 4: Connect your AI agent** (see platform setup below)

### Install for Agents

Paste this to any AI agent (Claude Code, OpenCode, Codex, etc.) and it will set everything up:

```
Set up agentic-armor-rust on this machine:
1. Clone repos: git clone https://github.com/calebrosario/rust-mcp-sdk.git ~/Documents/sandbox/rust-mcp-sdk && git clone https://github.com/calebrosario/agentic-armor-rust.git ~/Documents/sandbox/agentic-armor-rust
2. Build: cd ~/Documents/sandbox/agentic-armor-rust && cargo build --release
3. Start PostgreSQL: docker run -d --name armor-pg -e POSTGRES_USER=opencode -e POSTGRES_PASSWORD=opencode -e POSTGRES_DB=agentic_armor -p 5433:5432 postgres:16-alpine
4. Add to ~/.zshrc: export DATABASE_URL="postgresql://opencode:opencode@localhost:5433/agentic_armor"
5. Verify: ~/Documents/sandbox/agentic-armor-rust/target/release/agentic-armor (should start MCP server on stdio)
```

---

## Platform Setup

agentic-armor-rust works with any MCP-compatible AI agent via stdio. The binary path below is `/path/to/agentic-armor-rust/target/release/agentic-armor` — adjust to your install location.

### Claude Code

Add to `~/.claude/claude_desktop_config.json`:

```json
{
  "mcpServers": {
    "agentic-armor": {
      "command": "/path/to/agentic-armor-rust/target/release/agentic-armor",
      "env": {
        "DATABASE_URL": "postgresql://opencode:opencode@localhost:5433/agentic_armor"
      }
    }
  }
}
```

Or project-level (`.mcp.json` in your project root):

```json
{
  "mcpServers": {
    "agentic-armor": {
      "command": "/path/to/agentic-armor-rust/target/release/agentic-armor"
    }
  }
}
```

Restart Claude Code. Say "list your tools" to verify 8 tools are available.

### Cursor

Add to `.cursor/mcp.json` in your project root:

```json
{
  "mcpServers": {
    "agentic-armor": {
      "command": "/path/to/agentic-armor-rust/target/release/agentic-armor",
      "env": {
        "DATABASE_URL": "postgresql://opencode:opencode@localhost:5433/agentic_armor"
      }
    }
  }
}
```

Restart Cursor. Tools appear in the MCP panel.

### Windsurf

Add to Windsurf Settings → MCP Servers, or edit the Windsurf config file:

```json
{
  "mcpServers": {
    "agentic-armor": {
      "command": "/path/to/agentic-armor-rust/target/release/agentic-armor",
      "env": {
        "DATABASE_URL": "postgresql://opencode:opencode@localhost:5433/agentic_armor"
      }
    }
  }
}
```

Restart Windsurf.

### OpenCode

Add to `~/.config/opencode/opencode.json` under `mcp`:

```json
{
  "mcp": {
    "agentic-armor": {
      "type": "local",
      "command": ["/path/to/agentic-armor-rust/target/release/agentic-armor"],
      "enabled": true
    }
  }
}
```

Restart OpenCode.

### Codex (OpenAI)

Add to `~/.codex/config.json` (or `.codex/mcp.json` in project root):

```json
{
  "mcpServers": {
    "agentic-armor": {
      "command": "/path/to/agentic-armor-rust/target/release/agentic-armor",
      "env": {
        "DATABASE_URL": "postgresql://opencode:opencode@localhost:5433/agentic_armor"
      }
    }
  }
}
```

Restart Codex CLI.

### Continue.dev

Add to `~/.continue/config.json`:

```json
{
  "mcpServers": {
    "agentic-armor": {
      "command": "/path/to/agentic-armor-rust/target/release/agentic-armor"
    }
  }
}
```

Restart Continue.

### GitHub Copilot Chat (VS Code)

Add to VS Code `settings.json`:

```json
{
  "github.copilot.chat.mcp.servers": {
    "agentic-armor": {
      "command": "/path/to/agentic-armor-rust/target/release/agentic-armor",
      "env": {
        "DATABASE_URL": "postgresql://opencode:opencode@localhost:5433/agentic_armor"
      }
    }
  }
}
```

Reload VS Code window.

### Verification

After connecting any platform, verify by asking the agent:

```
List your available MCP tools.
```

You should see 8 tools: `task_create`, `task_exec`, `task_upload`, `task_download`, `task_list`, `task_stop`, `task_delete`, `task_logs`.

Then test:

```
Create a task called "test" with taskId "test-1", then list all tasks.
```

---

## The 8 MCP Tools

| Tool | Purpose | Key Parameters |
|------|---------|----------------|
| `task_create` | Create sandboxed task + Docker container | `taskId`, `name`, `image?`, `owner?` |
| `task_exec` | Run command inside sandbox | `taskId`, `command` (string[]), `timeout?` |
| `task_upload` | Write file into container (restricted paths) | `taskId`, `path`, `content` (max 10MB) |
| `task_download` | Read file from container (restricted paths) | `taskId`, `path` |
| `task_list` | List all tasks | `limit?` (max 1000) |
| `task_stop` | Stop a running task | `taskId` |
| `task_delete` | Delete task + destroy container | `taskId` |
| `task_logs` | Retrieve execution logs | `taskId`, `limit?` |

---

## Usage Examples

### Run a Python script in isolation

```
task_create  taskId="job-1"  name="Analytics"
task_upload  taskId="job-1"  path="/home/opencode/script.py"  content=<code>
task_exec    taskId="job-1"  command=["python3", "/home/opencode/script.py"]
task_delete  taskId="job-1"
```

### Build and test in a sandbox

```
task_create  taskId="build-1"
task_upload  path="/workspace/main.rs"  content=<code>
task_exec    command=["sh", "-c", "cd /workspace && cargo test"]  timeout=300000
task_delete  taskId="build-1"
```

### Container lifecycle

```
task_create  taskId="session-1"
task_exec    command=["git", "clone", "https://github.com/repo.git", "/workspace/repo"]
task_exec    command=["sh", "-c", "cd /workspace/repo && npm ci && npm test"]
task_stop    taskId="session-1"
task_delete  taskId="session-1"
```
