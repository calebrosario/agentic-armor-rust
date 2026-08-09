# PROJECT KNOWLEDGE BASE

**Repo:** agentic-armor-rust
**Branch:** main

## OVERVIEW

Rust port of agentic-armor. Hardened container sandbox execution for AI agents via MCP. Supports both Docker and Podman via unified ContainerRuntime trait.

## STRUCTURE

```
agentic-armor-rust/
├── Cargo.toml
├── src/
│   ├── lib.rs              # Public API exports
│   ├── main.rs             # Demo binary (create → exec → destroy)
│   ├── config.rs           # Config struct + RuntimeChoice enum
│   ├── error.rs            # ArmorError with security error codes
│   └── docker/
│       ├── mod.rs
│       ├── types.rs         # ArmorContainerConfig, Mount, ExecRequest, ExecResult
│       └── manager.rs       # BollardRuntime (Docker + Podman) + ContainerRuntime trait
├── tests/
│   ├── config_test.rs       # 4 tests: defaults, images, patterns, paths
│   ├── error_test.rs        # 2 tests: error codes, display messages
│   ├── runtime_test.rs      # 5 tests: RuntimeChoice parsing
│   └── types_test.rs        # 5 tests: container config, mount, exec
```

## WHERE TO LOOK

| Task | Location |
|------|----------|
| Add container operations | `src/docker/manager.rs` — ContainerRuntime trait |
| Add security checks | `src/docker/manager.rs` — build_bollard_config() |
| Add config options | `src/config.rs` — Config struct |
| Add error types | `src/error.rs` — ArmorError enum |
| Change runtime | `CONTAINER_RUNTIME=docker|podman|auto` env var |

## CONTAINER RUNTIME ARCHITECTURE

`ContainerRuntime` trait abstracts container lifecycle:
- `BollardRuntime` implements it for both Docker and Podman
- Podman 4+ provides Docker-compatible API (same bollard calls)
- Auto-detection: tries Docker socket, then Podman socket
- `RuntimeChoice` enum: Auto (default), Docker, Podman

## CONVENTIONS

- Rust 2021 Edition
- `async_trait` for trait objects
- `bollard` for Docker/Podman API
- `sqlx` for PostgreSQL (compile-time checked queries)
- `mcp-sdk` from `../rust-mcp-sdk` (local path dependency)
- `tracing` for logging
- `ArmorResult<T> = Result<T, ArmorError>`
- `ArmorContainerConfig` (not `ContainerConfig` to avoid bollard collision)

## COMMANDS

```bash
cargo test
cargo run
cargo build --release
cargo clippy
```
