# PROJECT KNOWLEDGE BASE

**Repo:** agentic-armor-rust
**Branch:** main

## OVERVIEW

Hardened container-sandbox execution for AI agents via MCP (stdio). One Rust binary: embedded SQLite registry + append-only audit trail, Docker and Podman through a unified `ContainerRuntime` trait, 8 MCP tools. Rust port of agentic-armor (the TypeScript original required PostgreSQL; this one has zero runtime services).

## STRUCTURE

```
agentic-armor-rust/
├── Cargo.toml              # mcp-rust-sdk pinned git dep; Cargo.lock IS committed
├── src/
│   ├── lib.rs              # Public API re-exports
│   ├── main.rs             # Binary: config → runtime detect+ping → SQLite pool → MCP serve
│   ├── config.rs           # Config (env-driven), RuntimeChoice, TaskNetworkEgress, parse_csv_list
│   ├── error.rs            # ArmorError + machine codes (IMAGE_NOT_ALLOWED, FORBIDDEN_MOUNT, …)
│   ├── docker/
│   │   ├── types.rs        # ArmorContainerConfig, Mount, ExecRequest/Result, KillOutcome, NetworkConfig
│   │   └── manager.rs      # BollardRuntime + ContainerRuntime trait; build_bollard_config (pure, testable)
│   ├── task/
│   │   ├── registry.rs     # TaskRegistry (sqlx SQLite): tasks + task_events (append-only, monotonic seq)
│   │   └── lifecycle.rs    # TaskLifecycle: create / mark_running / cancel / delete + audit events
│   └── mcp/
│       └── server.rs       # 8 MCP tools, path validation, upload chunking, base64 downloads, tombstones
├── tests/
│   ├── hardening_test.rs   # build_bollard_config contract: hardening flags, clamps, mount policy
│   ├── mcp_server_test.rs  # pure helpers: paths, base64, chunking, audit formats, tombstone replay
│   ├── registry_test.rs    # migrations (v1→v2 seq), status lifecycle, audit survival
│   ├── config_test.rs / error_test.rs / runtime_test.rs / types_test.rs
│   └── adversarial/        # 13-scenario escape/exfil harness (real agent; manual, ~1h) + reports
└── docs/ADVERSARIAL-TESTING.md
```

## WHERE TO LOOK

| Task | Location |
|------|----------|
| Container operations | `src/docker/manager.rs` — `ContainerRuntime` trait |
| Security enforcement (caps / mounts / limits / userns) | `src/docker/manager.rs` — `build_bollard_config()` (pure fn; tested via `tests/hardening_test.rs`) |
| MCP tool behavior, path rules, uploads/downloads | `src/mcp/server.rs` |
| Config knobs | `src/config.rs` — `Config::default()` reads env |
| Audit trail / tombstone recovery | `src/task/registry.rs` + `server.rs::replay_tombstones` |
| Error codes | `src/error.rs` — `ArmorError::code()` |
| Change runtime | `CONTAINER_RUNTIME=docker\|podman\|auto` |

## CONTAINER RUNTIME ARCHITECTURE

- `ContainerRuntime` trait abstracts the lifecycle; `BollardRuntime` implements it for Docker and Podman (Podman 4+ Docker-compatible API)
- Auto-detection: Docker socket → Docker Desktop socket → Podman socket (`auto_detect`); `main.rs` pings the daemon at startup with an actionable hint
- Per-task isolated bridge networks `armor-<taskId>` (taskId ≤ 58 chars — network names cap at 64); `TASK_NETWORK_EGRESS=internal` makes bridge egress fail closed

## SECURITY MODEL (enforced in build_bollard_config — no caller knobs)

- Hardcoded: `cap-drop: ALL`, read-only rootfs, `no-new-privileges`, user `opencode`, 512MB memory floor, PID clamp 10–1000
- Image allowlist via `ALLOWED_IMAGES` (defaults: `opencode-sandbox-{base,developer}:latest`) → `ArmorError::ImageNotAllowed`
- Mounts: blocklist (`forbidden_mount_patterns`) AND canonicalized source allowlist (`ALLOWED_MOUNT_PREFIXES`, empty = deny all bind/volume; tmpfs exempt)
- Upload/download: prefix + charset allowlist, in-container canonicalization, final-symlink guard; downloads base64 round-trip (`encoding: utf8|base64`)
- Exec: process-group wrap, verified timeout kills (`KillOutcome` taxonomy), handler ceiling `AA_HANDLER_TIMEOUT_SECS`

## CONVENTIONS

- Rust 2021, `async_trait`, `bollard`, `sqlx` (SQLite, runtime queries), `mcp-rust-sdk` (pinned git dep), `tracing`, `thiserror`
- `ArmorResult<T>`; `ArmorContainerConfig` deliberately has NO hardening fields (weakening is a compile error)
- CI gates: `cargo fmt --check`, `clippy -D warnings`, `cargo test --locked`, gating `cargo audit`
- Commits: conventional `type(scope): summary`

## COMMANDS

```bash
cargo test --locked         # 92 tests
cargo clippy --all-targets --locked
cargo fmt
cargo build --release
```

<!-- BEGIN BEADS INTEGRATION v:1 profile:minimal hash:ca08a54f -->
## Beads Issue Tracker

This project uses **bd (beads)** for issue tracking. Run `bd prime` to see full workflow context and commands.

### Quick Reference

```bash
bd ready              # Find available work
bd show <id>          # View issue details
bd update <id> --claim  # Claim work
bd close <id>         # Complete work
```

### Rules

- Use `bd` for ALL task tracking — do NOT use TodoWrite, TaskCreate, or markdown TODO lists
- Run `bd prime` for detailed command reference and session close protocol
- Use `bd remember` for persistent knowledge — do NOT use MEMORY.md files

## Session Completion

**When ending a work session**, you MUST complete ALL steps below. Work is NOT complete until `git push` succeeds.

**MANDATORY WORKFLOW:**

1. **File issues for remaining work** - Create issues for anything that needs follow-up
2. **Run quality gates** (if code changed) - Tests, linters, builds
3. **Update issue status** - Close finished work, update in-progress items
4. **PUSH TO REMOTE** - This is MANDATORY:
   ```bash
   git pull --rebase
   bd dolt push
   git push
   git status  # MUST show "up to date with origin"
   ```
5. **Clean up** - Clear stashes, prune remote branches
6. **Verify** - All changes committed AND pushed
7. **Hand off** - Provide context for next session

**CRITICAL RULES:**
- Work is NOT complete until `git push` succeeds
- NEVER stop before pushing - that leaves work stranded locally
- NEVER say "ready to push when you are" - YOU must push
- If push fails, resolve and retry until it succeeds
<!-- END BEADS INTEGRATION -->
