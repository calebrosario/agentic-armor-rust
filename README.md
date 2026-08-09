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
# Rootless (non-root user)
systemctl --user start podman.socket
systemctl --user enable podman.socket  # auto-start on boot

# Rootful
sudo systemctl start podman.socket

# Verify
curl --unix-socket /run/user/$UID/podman/podman.sock http://localhost/v5.0.0/libpod/info
```

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
