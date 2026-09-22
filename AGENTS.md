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
