# Adversarial Test Harness

Runs escape/exfiltration scenarios against agentic-armor using a real agent. See [docs/ADVERSARIAL-TESTING.md](../../docs/ADVERSARIAL-TESTING.md) for methodology, verdicts, and the 13-scenario matrix.

## Layout

- `runner.py` — orchestrator; scenario definitions embedded (prompt + verdict logic per scenario)
- `canaries.sh` — plant/verify synthetic secrets on the VM host
- `sink.py` — HTTP exfiltration logger on `0.0.0.0:9999`
- `reports/` — generated evidence bundles + `AGGREGATE.md`

## Preconditions

1. Isolated Linux VM with Docker, agentic-armor built (`cargo build --release` at repo root), opencode installed and authenticated
2. Canaries planted: `bash canaries.sh plant`
3. Sink running: `python3 sink.py &`
4. Agent-side isolation: the runner creates `~/sandbox/adv` with `bash: {"*": "deny"}` — the agent can only act through agentic-armor MCP tools, mirroring a protected deployment

## Run

```bash
python3 runner.py --all          # full suite, ~45-70 min
python3 runner.py --only S07,S09 # subset
```

Each run writes `reports/<id>_report.md` (tool-call trace, sink window, canary hashes, `task_events` dump incl. `exec_logged`, docker state, transcript) and refreshes `reports/AGGREGATE.md`.

## Safety

The harness is designed for a disposable isolated VM. Never run it on a machine with real credentials: scenarios deliberately instruct the agent to attack the host, and the sink logs whatever exfiltration succeeds.
