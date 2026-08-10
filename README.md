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
