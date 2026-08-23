# Agentic Armor (Rust)

> One binary. Zero dependencies. Hardened AI agent sandboxes via MCP. Docker + Podman.

[![CI](https://github.com/calebrosario/agentic-armor-rust/actions/workflows/ci.yml/badge.svg)](https://github.com/calebrosario/agentic-armor-rust/actions/workflows/ci.yml)
[![Security Audit](https://img.shields.io/badge/security-cargo--audit-green)](https://github.com/calebrosario/agentic-armor-rust/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.85%2B-orange.svg)](https://www.rust-lang.org/)
[![Binary Size](https://img.shields.io/badge/binary-7.3MB-success.svg)](https://github.com/calebrosario/agentic-armor-rust/releases)
[![Runtime](https://img.shields.io/badge/runtime-Docker%20%7C%20Podman-blue.svg)](https://github.com/calebrosario/agentic-armor-rust#container-runtime-support)
[![MCP Clients](https://img.shields.io/badge/MCP%20clients-7-purple.svg)](https://github.com/calebrosario/agentic-armor-rust#platform-setup)
[![Database](https://img.shields.io/badge/database-SQLite%20embedded-success.svg)](https://github.com/calebrosario/agentic-armor-rust#why-no-postgresql)
[![GitHub stars](https://img.shields.io/github/stars/calebrosario/agentic-armor-rust?style=social)](https://github.com/calebrosario/agentic-armor-rust/stargazers)

**Agentic Armor** gives AI agents (Claude Code, OpenCode, Cursor, Codex, Windsurf) the ability to run code inside hardened, isolated containers — without touching your host filesystem. One 7.3MB Rust binary. No Node.js, no PostgreSQL, no external services.

## Why Agentic Armor?

| Without Agentic Armor | With Agentic Armor |
|----------------------|-------------------|
| Agent runs `rm -rf ~/Documents` on your Mac | Agent runs inside a container — can't see your home directory |
| Agent reads `~/.ssh/id_rsa` and exfiltrates it | SSH keys don't exist inside the container |
| Agent installs malware via `curl \| sh` | `cap-drop: ALL`, `no-new-privileges`, no network by default |
| You babysit permission prompts all day | Agent runs autonomously in isolation — you review when done |
| Agent needs Node.js + npm + PostgreSQL installed | One binary, SQLite embedded, zero external dependencies |

## One Binary, Zero Dependencies

```
7.3MB binary
├── Embedded SQLite database (auto-created on first run)
├── MCP server (stdio transport — talks to any AI agent)
├── Docker + Podman manager (auto-detects which is running)
├── 8 tools (create, exec, upload, download, list, stop, delete, logs)
└── Security hardening (cap-drop, readonly rootfs, network isolation)
```

No Node.js runtime. No npm install. No PostgreSQL container. No Docker Compose for the database. Just:

```bash
./agentic-armor
```

The SQLite database is auto-created at `./data/agentic_armor.db` on first run. Schema is auto-migrated. If you delete the file, it recreates itself.

## Security Model

Every container created by Agentic Armor is hardened — **non-overridable by design**:

| Control | Value | Bypass Possible? |
|---------|-------|-----------------|
| Linux capabilities | `cap-drop: ALL` | ❌ Hardcoded |
| Root filesystem | Read-only | ❌ Hardcoded |
| Privilege escalation | `no-new-privileges` | ❌ Hardcoded |
| Container user | `opencode` (non-root) | ❌ Hardcoded |
| Network mode | `none` (no internet) | ❌ Default; `network:"bridge"` grants egress via an **isolated per-task network** — cross-task traffic unreachable |
| Docker socket mounts | Blocked (source + target, canonicalized) | ❌ Pattern + symlink check |
| Image selection | Allowlist only | ❌ Enforced at runtime layer |
| Memory | 512MB minimum | ❌ Clamped (never 0) |
| Process limit | 100 PIDs | ❌ Clamped (10-1000) |
| Max concurrent containers | 10 | ❌ DoS protection |
| Path access (upload/download) | `/tmp/`, `/home/opencode/`, `/workspace/` only | ❌ Prefix + char allowlist |
| Mount validation | Canonicalized paths + forbidden patterns | ❌ Symlink-aware |

**What the agent CAN do:** edit files in `/workspace/`, run builds, execute tests, install packages (if network enabled), read/write to allowed paths.

**What the agent CANNOT do:** read `~/.ssh/id_rsa`, access host filesystem, escalate to root, mount Docker socket, create unlimited containers, bypass the image allowlist, or reach the internet (by default).

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
cargo test          # 31 tests
cargo run           # Run test container lifecycle
```

### Adversarial Testing

12 automated scenarios where a real agent (OpenCode + LLM) is instructed to escape the sandbox, exfiltrate secrets, or abuse the system — with every attempt verified contained and logged: [docs/ADVERSARIAL-TESTING.md](docs/ADVERSARIAL-TESTING.md)

```bash
python3 tests/adversarial/runner.py --all   # full suite (~1h), results in tests/adversarial/reports/
```

Every `task_exec` attempt is persisted to the audit trail (`exec_logged` events, surfaced by `task_logs`).

---

## Installation

### Prerequisites

- **Docker** or **Podman** (for sandbox containers — that's it)

### Install for Humans

**Step 1: Build the binary**

```bash
git clone https://github.com/calebrosario/rust-mcp-sdk.git
git clone https://github.com/calebrosario/agentic-armor-rust.git
cd agentic-armor-rust
cargo build --release
```

The binary is at `target/release/agentic-armor` (7.3MB, no runtime dependencies).

**Step 2: Verify it works**

```bash
./target/release/agentic-armor
# → Auto-detects Docker/Podman
# → Creates ./data/agentic_armor.db (SQLite, auto-migrated)
# → Starts MCP server on stdio (8 tools)
```

That's it. No PostgreSQL. No Docker Compose for the database. No environment variables needed.

**Step 3: Connect your AI agent** (see platform setup below)

### Install for Agents

Paste this to any AI agent and it will set everything up:

```
Set up agentic-armor-rust:
1. Clone: git clone https://github.com/calebrosario/rust-mcp-sdk.git ~/Documents/sandbox/rust-mcp-sdk
2. Clone: git clone https://github.com/calebrosario/agentic-armor-rust.git ~/Documents/sandbox/agentic-armor-rust
3. Build: cd ~/Documents/sandbox/agentic-armor-rust && cargo build --release
4. Test: ~/Documents/sandbox/agentic-armor-rust/target/release/agentic-armor (should start, create SQLite DB, connect to Docker)
5. Add to ~/.config/opencode/opencode.json mcp section: {"agentic-armor": {"type":"local","command":["$HOME/Documents/sandbox/agentic-armor-rust/target/release/agentic-armor"],"enabled":true}}
```

### Why No PostgreSQL?

Agentic Armor uses **embedded SQLite** instead of PostgreSQL. The database is a single file (`./data/agentic_armor.db`) that:

- Auto-creates on first run (zero setup)
- Auto-migrates the schema
- Handles the workload perfectly (single process, small dataset, ephemeral tasks)
- Can be deleted and recreated at any time (tasks are ephemeral)
- Requires zero configuration, zero ports, zero credentials

The previous TypeScript version required PostgreSQL running in Docker — creating a circular dependency (Docker → PostgreSQL → agentic-armor → Docker). The Rust version eliminates this entirely.

---

## Platform Setup

agentic-armor-rust works with any MCP-compatible AI agent via stdio. The binary path below is `/path/to/agentic-armor-rust/target/release/agentic-armor` — adjust to your install location.

### Claude Code

Add to `~/.claude/claude_desktop_config.json`:

```json
{
  "mcpServers": {
    "agentic-armor": {
      "command": "/path/to/agentic-armor-rust/target/release/agentic-armor"
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
      "command": "/path/to/agentic-armor-rust/target/release/agentic-armor"
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
      "command": "/path/to/agentic-armor-rust/target/release/agentic-armor"
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
      "command": "/path/to/agentic-armor-rust/target/release/agentic-armor"
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
      "command": "/path/to/agentic-armor-rust/target/release/agentic-armor"
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
