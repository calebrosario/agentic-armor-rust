use agentic_armor::config::Config;
use agentic_armor::docker::{exec_kill_command, exec_wrap_command};
use agentic_armor::mcp::server::{
    arg_opt_str, arg_str, arg_str_array, arg_u64, base64_encode, default_task_mounts,
    default_task_mounts_for, is_valid_network_mode, npm_scripts_blocked_env, upload_chunk_commands,
    validate_path,
};

fn mounts_by_target(target: &str) -> agentic_armor::Mount {
    default_task_mounts()
        .into_iter()
        .find(|m| m.target == target)
        .unwrap_or_else(|| panic!("no mount for {}", target))
}

#[test]
fn default_mounts_cover_all_writable_paths() {
    let mounts = default_task_mounts();
    assert_eq!(mounts.len(), 3);
    for m in &mounts {
        assert_eq!(m.mount_type, "tmpfs");
        assert!(m.read_only.is_none());
    }
}

#[test]
fn tmpfs_mounts_are_writable_by_sandbox_user() {
    for target in ["/tmp", "/home/opencode", "/workspace"] {
        let opts = mounts_by_target(target)
            .tmpfs_options
            .expect("tmpfs options required");
        assert!(
            opts.contains("uid=1001") && opts.contains("gid=1001") && opts.contains("mode=0775"),
            "{} must be owned by sandbox user 1001, got: {}",
            target,
            opts
        );
    }
}

#[test]
fn podman_tmpfs_mounts_use_sticky_mode_instead_of_uid_options() {
    for target in ["/tmp", "/home/opencode", "/workspace"] {
        let m = default_task_mounts_for("podman")
            .into_iter()
            .find(|m| m.target == target)
            .unwrap_or_else(|| panic!("no mount for {}", target));
        let opts = m.tmpfs_options.expect("tmpfs options required");
        assert!(
            opts.contains("mode=1777"),
            "{} must be sticky world-writable on podman, got: {}",
            target,
            opts
        );
        assert!(
            !opts.contains("uid=") && !opts.contains("gid="),
            "podman rejects uid=/gid= tmpfs options, got: {}",
            opts
        );
    }
}

#[test]
fn tmpfs_sizes_match_documented_limits() {
    assert!(mounts_by_target("/tmp")
        .tmpfs_options
        .unwrap()
        .contains("size=64m"));
    assert!(mounts_by_target("/home/opencode")
        .tmpfs_options
        .unwrap()
        .contains("size=64m"));
    assert!(mounts_by_target("/workspace")
        .tmpfs_options
        .unwrap()
        .contains("size=256m"));
}

#[test]
fn network_mode_accepts_only_none_and_bridge() {
    assert!(is_valid_network_mode("none"));
    assert!(is_valid_network_mode("bridge"));
    for invalid in [
        "host",
        "",
        "Bridge",
        "none ",
        "bridge\n",
        "container:foo",
        "custom0",
    ] {
        assert!(
            !is_valid_network_mode(invalid),
            "'{}' must be rejected",
            invalid
        );
    }
}

#[test]
fn validate_path_accepts_documented_paths() {
    let cfg = Config::default();
    for path in [
        "/tmp/x.txt",
        "/tmp/nested/dir/file.json",
        "/home/opencode/script.py",
        "/workspace/src/main.rs",
        "/workspace/data.csv",
        "/tmp/a.b.c.d",
        "/tmp/kebab-case_file@2.0-1.txt",
    ] {
        assert!(
            validate_path(path, &cfg).is_ok(),
            "'{}' should be allowed",
            path
        );
    }
}

#[test]
fn validate_path_rejects_relative_paths() {
    let cfg = Config::default();
    for path in ["tmp/x", "workspace/file", "", "./x", "~/secret"] {
        assert!(
            validate_path(path, &cfg).is_err(),
            "'{}' must be rejected",
            path
        );
    }
}

#[test]
fn validate_path_rejects_traversal() {
    let cfg = Config::default();
    for path in [
        "/tmp/../etc/passwd",
        "/workspace/../../etc/shadow",
        "/tmp/a..b",
        "/home/opencode/..",
        "/tmp/./../proc/self/environ",
    ] {
        assert!(
            validate_path(path, &cfg).is_err(),
            "'{}' must be rejected",
            path
        );
    }
}

#[test]
fn validate_path_rejects_prefix_bypass_attempts() {
    let cfg = Config::default();
    for path in [
        "/tmpfoo/file",
        "/tmpx/file",
        "/workspaceevil",
        "/home/opencodeX/file",
        "/etc/passwd",
        "/proc/self/environ",
        "/root/.ssh/id_rsa",
        "/var/run/docker.sock",
        "/tmp",
        "/workspace",
        "/home/opencode",
    ] {
        assert!(
            validate_path(path, &cfg).is_err(),
            "'{}' must be rejected",
            path
        );
    }
}

#[test]
fn validate_path_rejects_dangerous_characters() {
    let cfg = Config::default();
    for path in [
        "/tmp/my file.txt",
        "/tmp/$(whoami).txt",
        "/tmp/`id`.txt",
        "/tmp/semi;colon",
        "/tmp/pipe|cat",
        "/tmp/quote'quote",
        "/tmp/newline\n.txt",
        "/tmp/uni\u{00e9}.txt",
    ] {
        assert!(
            validate_path(path, &cfg).is_err(),
            "'{}' must be rejected",
            path
        );
    }
}

#[test]
fn base64_encode_roundtrips() {
    let cases = ["", "a", "ab", "abc", "hello world\n", "\u{00e9}\u{4e2d}"];
    for input in cases {
        let encoded = base64_encode(input);
        assert!(
            !encoded.contains('\''),
            "encoded output must be shell-single-quote-safe"
        );
        assert_eq!(encoded.len() % 4, 0);
        assert!(encoded
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '='));
    }
}

#[test]
fn upload_chunks_stay_under_argmax() {
    let big_b64 = base64_encode(&"x".repeat(300 * 1024));
    let cmds = upload_chunk_commands("/workspace/big.txt", &big_b64);
    assert!(
        cmds.len() >= 7,
        "300KB payload must split into multiple chunks, got {}",
        cmds.len()
    );
    for cmd in &cmds {
        assert!(
            cmd.len() < 64 * 1024,
            "chunk command must stay under MAX_ARG_STRLEN (128KB), got {}",
            cmd.len()
        );
    }
    assert!(cmds[0].contains("> '/workspace/big.txt'"));
    assert!(
        cmds[1].contains(">> '/workspace/big.txt'"),
        "subsequent chunks must append"
    );
}

#[test]
fn upload_chunk_boundaries_are_base64_aligned() {
    let input = "A".repeat(200 * 1024);
    let b64 = base64_encode(&input);
    for chunk in b64.as_bytes().chunks(48 * 1024) {
        assert_eq!(
            chunk.len() % 4,
            0,
            "every chunk must be independently decodable base64"
        );
    }
}

#[test]
fn upload_empty_content_creates_truncated_file() {
    let cmds = upload_chunk_commands("/tmp/empty.txt", "");
    assert_eq!(cmds.len(), 1);
    assert!(cmds[0].contains(": > '/tmp/empty.txt'"));
    assert!(
        cmds[0].contains("[ ! -L '/tmp/empty.txt' ]"),
        "must refuse writing through a final-component symlink"
    );
}

#[test]
fn upload_first_chunk_guards_final_symlink() {
    let cmds = upload_chunk_commands("/workspace/f.txt", &base64_encode(&"x".repeat(100 * 1024)));
    assert!(cmds.len() >= 2, "payload must span multiple chunks");
    assert!(cmds[0].contains("[ ! -L '/workspace/f.txt' ]"));
    assert!(
        !cmds[1].contains("[ ! -L"),
        "guard only needed on first chunk"
    );
}

#[test]
fn upload_chunks_cleanup_on_failure() {
    let cmds = upload_chunk_commands("/workspace/f.txt", &base64_encode("data"));
    assert!(
        cmds.iter().all(|c| c.contains("rm -f '/workspace/f.txt'")),
        "failed writes must not leave partial files"
    );
}

#[test]
fn task_network_names_are_namespaced_and_valid() {
    use agentic_armor::docker::{is_valid_task_network_name, task_network_name};
    assert_eq!(task_network_name("s12-a"), "armor-s12-a");
    assert!(is_valid_task_network_name("armor-s12-a"));
    assert!(is_valid_task_network_name("armor-My_Task-01"));
    assert!(
        !is_valid_task_network_name("bridge"),
        "shared bridge must be rejected"
    );
    assert!(!is_valid_task_network_name("host"));
    assert!(!is_valid_task_network_name("armor-"), "empty task id part");
    assert!(!is_valid_task_network_name("armor-a/b"), "path chars");
    assert!(!is_valid_task_network_name("armor-a b"), "spaces");
    let long_name = format!("armor-{}", "x".repeat(80));
    assert!(!is_valid_task_network_name(&long_name), "over 64 chars");
}

#[test]
fn pid_exhaustion_signatures_are_recognized() {
    use agentic_armor::docker::is_pid_exhaustion_error;
    use agentic_armor::error::ArmorError;
    assert!(is_pid_exhaustion_error(&ArmorError::Docker(
        "Error in the hyper legacy client: OCI runtime exec failed: exec failed: unable to start container process: procReady not received".into())));
    assert!(is_pid_exhaustion_error(&ArmorError::Docker(
        "sh: can't fork: Resource temporarily unavailable".into()
    )));
    assert!(is_pid_exhaustion_error(&ArmorError::Docker(
        "write /proc/self/oom: No space left on device".into()
    )));
    assert!(!is_pid_exhaustion_error(&ArmorError::Docker(
        "image not found".into()
    )));
    assert!(!is_pid_exhaustion_error(&ArmorError::TaskNotFound(
        "x".into()
    )));
}

#[test]
fn shell_quote_escapes_single_quotes() {
    use agentic_armor::mcp::server::shell_quote;
    assert_eq!(shell_quote("/tmp/plain.txt"), "'/tmp/plain.txt'");
    assert_eq!(shell_quote("/tmp/it's.txt"), "'/tmp/it'\\''s.txt'");
    assert_eq!(shell_quote(""), "''");
}

#[test]
fn shell_quoted_upload_commands_stay_safe_with_hostile_names() {
    let cmds = upload_chunk_commands("/tmp/x'; rm -rf /; '", &base64_encode("data"));
    assert!(
        cmds.iter()
            .all(|c| !c.contains("; rm -rf / ;") || c.contains("'\\''")),
        "raw quote must never appear unescaped: {:?}",
        cmds
    );
}

#[test]
fn audit_command_truncates_at_512_chars_on_char_boundary() {
    use agentic_armor::mcp::server::audit_command;
    let long: Vec<String> = vec!["x".repeat(600)];
    let truncated = audit_command(&long);
    assert_eq!(truncated.chars().count(), 512);

    let multibyte: Vec<String> = vec![format!("{}{}", "a".repeat(510), "é中🎉")];
    let t = audit_command(&multibyte);
    assert!(t.chars().count() <= 512);
    assert!(t.is_char_boundary(t.len()), "must end on a UTF-8 boundary");

    assert_eq!(audit_command(&[]), "");
    assert_eq!(audit_command(&["echo".into(), "hi".into()]), "echo hi");
}

#[test]
fn docker_network_mode_matrix() {
    use agentic_armor::docker::docker_network_mode;
    use agentic_armor::docker::NetworkConfig;
    assert_eq!(
        docker_network_mode(&NetworkConfig::None, false).unwrap(),
        "none"
    );
    assert_eq!(
        docker_network_mode(
            &NetworkConfig::Bridge {
                network: "armor-t1".into()
            },
            false
        )
        .unwrap(),
        "armor-t1"
    );
    assert!(
        docker_network_mode(
            &NetworkConfig::Bridge {
                network: "bridge".into()
            },
            false
        )
        .is_err(),
        "shared bridge rejected at type level"
    );
    assert!(
        docker_network_mode(
            &NetworkConfig::Bridge {
                network: "armor-".into()
            },
            false
        )
        .is_err(),
        "empty suffix rejected"
    );
    assert!(
        docker_network_mode(
            &NetworkConfig::Bridge {
                network: "armor-a b".into()
            },
            false
        )
        .is_err(),
        "space rejected"
    );
    assert!(
        docker_network_mode(&NetworkConfig::Host, false).is_err(),
        "host rejected without escape hatch"
    );
    assert_eq!(
        docker_network_mode(&NetworkConfig::Host, true).unwrap(),
        "host",
        "host allowed only via ALLOW_HOST_NETWORK"
    );
}

#[test]
fn upload_chunks_exact_boundary_single_chunk() {
    let b64_exact = "A".repeat(48 * 1024);
    let cmds = upload_chunk_commands("/tmp/b.txt", &b64_exact);
    assert_eq!(cmds.len(), 1, "exactly 48K of base64 must be one chunk");
    assert!(cmds[0].contains("> '/tmp/b.txt'"));

    let b64_over = "A".repeat(48 * 1024 + 4);
    let cmds = upload_chunk_commands("/tmp/b.txt", &b64_over);
    assert_eq!(cmds.len(), 2, "48K+4 must split into two chunks");
    assert!(cmds[0].contains("> '/tmp/b.txt'"));
    assert!(cmds[1].contains(">> '/tmp/b.txt'"));
}

#[test]
fn upload_chunks_each_decode_independently_and_concat() {
    let input = "é中🎉 payload ".repeat(9000);
    let b64 = base64_encode(&input);
    let cmds = upload_chunk_commands("/tmp/u.txt", &b64);
    let mut decoded = String::new();
    for cmd in &cmds {
        let payload = cmd
            .split("printf %s '")
            .nth(1)
            .unwrap()
            .split("' | base64")
            .next()
            .unwrap();
        let bytes = base64_decode_manual(payload);
        decoded.push_str(&bytes);
    }
    assert_eq!(
        decoded, input,
        "concatenated chunk decodes must equal original"
    );
}

fn base64_decode_manual(s: &str) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut buf: Vec<u8> = Vec::new();
    let mut acc: u32 = 0;
    let mut bits = 0;
    for c in s.chars() {
        if c == '=' || c == '\'' {
            break;
        }
        let v = CHARS
            .iter()
            .position(|&x| x as char == c)
            .expect("valid b64 char") as u32;
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            buf.push((acc >> bits) as u8);
        }
    }
    String::from_utf8(buf).expect("utf8")
}

#[test]
fn render_exec_stderr_joins_parts_in_order() {
    use agentic_armor::mcp::server::render_exec_stderr;
    assert_eq!(render_exec_stderr("", &[], ""), "");
    assert_eq!(render_exec_stderr("boom", &[], ""), "boom");
    assert_eq!(
        render_exec_stderr("boom", &["[agentic-armor] exec timed out".into()], ""),
        "boom\n[agentic-armor] exec timed out"
    );
    assert_eq!(
        render_exec_stderr("", &["note".into()], "  — hint  "),
        "note\n— hint",
        "fork hint trimmed, leading stderr gap skipped"
    );
}

// ---------------------------------------------------------------------------
// JSON argument validation (type-confusion hardening)
// ---------------------------------------------------------------------------

#[test]
fn arg_str_requires_string_and_reports_actual_type() {
    let args = serde_json::json!({ "taskId": 123, "owner": "agent-1" });
    let err = arg_str(&args, "taskId").unwrap_err();
    assert!(
        err.contains("'taskId' must be a string, got number"),
        "got: {err}"
    );
    assert_eq!(arg_str(&args, "owner").unwrap(), "agent-1");
    let missing = arg_str(&args, "nope").unwrap_err();
    assert!(
        missing.contains("missing required argument: nope"),
        "got: {missing}"
    );
}

#[test]
fn arg_opt_str_accepts_absent_but_rejects_wrong_type() {
    let args = serde_json::json!({ "image": false });
    assert!(arg_opt_str(&args, "absent").unwrap().is_none());
    let err = arg_opt_str(&args, "image").unwrap_err();
    assert!(
        err.contains("'image' must be a string, got boolean"),
        "got: {err}"
    );
}

#[test]
fn arg_u64_rejects_negative_float_and_string_but_accepts_u64() {
    let args = serde_json::json!({ "a": -1, "b": 1.5, "c": "3000", "d": 3000 });
    assert!(arg_u64(&args, "a")
        .unwrap_err()
        .contains("must be a non-negative integer"));
    assert!(arg_u64(&args, "b")
        .unwrap_err()
        .contains("must be a non-negative integer"));
    assert!(arg_u64(&args, "c")
        .unwrap_err()
        .contains("must be a non-negative integer"));
    assert!(arg_u64(&args, "absent").unwrap().is_none());
    assert_eq!(arg_u64(&args, "d").unwrap(), Some(3000));
}

#[test]
fn arg_str_array_names_the_offending_index() {
    let args = serde_json::json!({ "command": ["ls", "-la", 42] });
    let err = arg_str_array(&args, "command").unwrap_err();
    assert!(
        err.contains("'command'[2] must be a string, got number"),
        "got: {err}"
    );
    assert_eq!(
        arg_str_array(&serde_json::json!({ "command": ["echo", "hi"] }), "command").unwrap(),
        vec!["echo".to_string(), "hi".to_string()]
    );
    assert!(arg_str_array(&serde_json::json!({}), "command")
        .unwrap_err()
        .contains("missing required argument"));
}

// ---------------------------------------------------------------------------
// blockNpmScripts env mapping
// ---------------------------------------------------------------------------

#[test]
fn npm_scripts_blocked_env_maps_flag() {
    assert_eq!(
        npm_scripts_blocked_env(true),
        Some(vec!["NPM_CONFIG_IGNORE_SCRIPTS=1".to_string()])
    );
    assert_eq!(npm_scripts_blocked_env(false), None);
}

// ---------------------------------------------------------------------------
// Exec timeout kill plumbing
// ---------------------------------------------------------------------------

#[test]
fn exec_wrap_command_records_pgid_runs_payload_and_self_cleans() {
    let pid_file = "/tmp/armor-exec-deadbeef.pid";
    let payload = vec!["sleep".to_string(), "60".to_string()];
    let wrapped = exec_wrap_command(pid_file, &payload);
    assert_eq!(wrapped[0], "sh");
    assert_eq!(wrapped[1], "-c");
    let script = &wrapped[2];
    assert!(
        script.starts_with(&format!("echo $$ > {pid_file};")),
        "script must record the wrapper pid (== pgid) first: {script}"
    );
    assert!(
        script.contains("\"$@\" <&0 >&1 2>&2"),
        "payload must run with inherited stdio: {script}"
    );
    assert!(
        script.contains("; s=$?; rm -f ") && script.ends_with("; exit $s"),
        "script must rm the pidfile and propagate the payload exit status: {script}"
    );
    assert_eq!(
        &wrapped[4..],
        &payload[..],
        "payload preserved verbatim after $0 marker"
    );
}

#[test]
fn exec_kill_command_kills_process_group_and_cleans_up() {
    let pid_file = "/tmp/armor-exec-cafe.pid";
    let kill = exec_kill_command(pid_file);
    assert_eq!(kill[0], "sh");
    assert_eq!(kill[1], "-c");
    assert!(
        kill[2].starts_with("kill -9 -$(cat "),
        "must SIGKILL the recorded process GROUP (negative pid): {}",
        kill[2]
    );
    assert!(kill[2].contains(pid_file));
    assert!(
        kill[2].contains("rm -f"),
        "must remove the pidfile after killing: {}",
        kill[2]
    );
}

#[test]
fn test_task_summary_json_status_is_not_debug_quoted() {
    use agentic_armor::mcp::server::task_summary_json;
    use agentic_armor::Task;

    let t = Task {
        id: "t-1".into(),
        name: "demo".into(),
        status: "running".into(),
        owner: Some("agent".into()),
        metadata: serde_json::json!({}),
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };

    let summary = task_summary_json(&t);
    assert_eq!(summary["status"].as_str(), Some("running"));
    assert_eq!(summary["id"].as_str(), Some("t-1"));
    assert_eq!(summary["owner"].as_str(), Some("agent"));
    assert_eq!(summary["name"].as_str(), Some("demo"));
}

#[test]
fn exec_audit_message_distinguishes_all_kill_outcomes() {
    use agentic_armor::mcp::server::exec_audit_message;
    use agentic_armor::KillOutcome;

    assert!(
        exec_audit_message(137, 5100, KillOutcome::Verified, "sleep 60").contains("kill=verified")
    );
    assert!(
        exec_audit_message(137, 5100, KillOutcome::SentUnverified, "sleep 60")
            .contains("kill=unverified")
    );
    assert!(
        exec_audit_message(137, 5100, KillOutcome::NotDelivered, "sleep 60")
            .contains("kill=undeliverable")
    );
    assert_eq!(
        exec_audit_message(0, 42, KillOutcome::NotTimedOut, "echo hi"),
        "exec exit=0 durMs=42: echo hi"
    );
}

#[test]
fn stream_errors_containing_timed_out_never_attest_a_kill() {
    use agentic_armor::mcp::server::exec_audit_message;
    use agentic_armor::KillOutcome;

    let msg = exec_audit_message(-1, 120_000, KillOutcome::NotTimedOut, "sleep 300");
    assert!(
        !msg.contains("kill="),
        "no timeout fired and no kill was attempted — transport-error text must not attest one: {msg}"
    );
    assert!(msg.starts_with("exec exit=-1 durMs=120000:"));
}
#[test]
fn stop_delete_error_classification_separates_benign_from_real_failures() {
    use agentic_armor::mcp::server::{is_already_stopped_error, is_no_such_container_error};

    for benign in [
        "Container already stopped",
        "container abc is not running",
        "Docker responded with status code 304: ",
    ] {
        assert!(
            is_already_stopped_error(benign),
            "'{benign}' is a skip, not a failure"
        );
    }
    for real in [
        "permission denied",
        "connection refused",
        "no such container: 304ab9c",
    ] {
        assert!(
            !is_already_stopped_error(real),
            "'{real}' must surface stop_failed, not stop_skipped — note the hex id contains '304'"
        );
    }
    for gone in ["No such container: abc", "container not found: abc"] {
        assert!(
            is_no_such_container_error(gone),
            "'{gone}' is destroy_skipped"
        );
    }
    assert!(
        !is_no_such_container_error("device or resource busy"),
        "real destroy failures must retain the row"
    );
}

#[tokio::test]
async fn tombstoned_audit_events_replay_into_the_database() {
    let path = std::env::temp_dir().join(format!("aa-tombstones-{}.jsonl", uuid::Uuid::new_v4()));
    let line = serde_json::json!({
        "ts": "2026-09-03T00:00:00+00:00",
        "task_id": "t-tomb",
        "event_type": "stop_failed",
        "message": "boom"
    })
    .to_string();
    std::fs::write(&path, format!("{}\nnot json\n", line)).expect("write tombstone file");

    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("in-memory pool");
    let registry = agentic_armor::task::TaskRegistry::new(pool);
    registry.migrate().await.expect("migrate");

    let replayed = agentic_armor::mcp::replay_tombstones(&registry, &path).await;
    assert_eq!(replayed, 1, "one valid line replays");
    assert!(
        !path.exists(),
        "fully-replayed tombstone file (with only droppable corrupt lines) is removed"
    );

    let logs = registry.get_logs("t-tomb", 10).await.expect("logs");
    assert_eq!(logs.len(), 1);
    let message = logs[0].message.as_deref().unwrap_or("");
    assert!(
        message.contains(
            "[replayed from tombstone, originally recorded 2026-09-03T00:00:00+00:00] boom"
        ),
        "replayed event keeps original content plus provenance, got: {}",
        message
    );
    assert_eq!(
        agentic_armor::mcp::replay_tombstones(&registry, &path).await,
        0,
        "second replay after removal is a no-op"
    );
}

#[test]
fn task_id_length_caps_at_58_for_network_name_room() {
    use agentic_armor::mcp::server::{validate_task_id, MAX_TASK_ID_LEN};

    assert!(validate_task_id("ok-ID_01").is_ok());
    assert!(validate_task_id("").is_err());
    assert!(validate_task_id("bad/id").is_err());
    assert!(validate_task_id("has space").is_err());

    assert_eq!(MAX_TASK_ID_LEN, 58);
    let max_id = "a".repeat(58);
    assert!(validate_task_id(&max_id).is_ok());
    let too_long = "a".repeat(59);
    assert!(validate_task_id(&too_long).is_err());
    // 59 chars would produce a 65-char network name, over the runtime's 64 cap
    assert!(format!("armor-{}", too_long).len() > 64);
}

#[test]
fn download_payload_roundtrips_text_and_binary() {
    use agentic_armor::mcp::server::{base64_decode, base64_encode_bytes, decode_download_payload};

    let text = "hello\nworld\n";
    let (content, encoding, bytes, truncated) =
        decode_download_payload(&base64_encode_bytes(text.as_bytes()), 1024).unwrap();
    assert_eq!(content, text);
    assert_eq!(encoding, "utf8");
    assert_eq!(bytes, text.len());
    assert!(!truncated);

    let binary: Vec<u8> = vec![
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0xFF, 0x00, 0x81,
    ];
    let b64 = base64_encode_bytes(&binary);
    let (content, encoding, bytes, truncated) = decode_download_payload(&b64, 1024).unwrap();
    assert_eq!(
        encoding, "base64",
        "invalid-UTF-8 payloads must not be mangled through lossy conversion"
    );
    assert_eq!(bytes, binary.len());
    assert!(!truncated);
    assert_eq!(
        base64_decode(&content).unwrap(),
        binary,
        "client can decode losslessly"
    );
}

#[test]
fn download_truncation_flag_counts_decoded_bytes_exactly() {
    use agentic_armor::mcp::server::{base64_encode_bytes, decode_download_payload};

    let exact = "x".repeat(16);
    let (_, _, bytes, truncated) =
        decode_download_payload(&base64_encode_bytes(exact.as_bytes()), 16).unwrap();
    assert_eq!(bytes, 16);
    assert!(
        truncated,
        "exactly max bytes counts as truncated (head -c may have cut the file)"
    );

    let under = "y".repeat(15);
    let (_, _, bytes, truncated) =
        decode_download_payload(&base64_encode_bytes(under.as_bytes()), 16).unwrap();
    assert_eq!(bytes, 15);
    assert!(!truncated);
}

#[test]
fn download_decode_rejects_garbage() {
    use agentic_armor::mcp::server::decode_download_payload;

    assert!(decode_download_payload("not valid base64!!!", 10).is_err());
}

#[test]
fn optional_string_arrays_parse_strictly() {
    use agentic_armor::mcp::server::arg_opt_str_array;
    use serde_json::json;

    let absent = json!({});
    assert!(arg_opt_str_array(&absent, "env").unwrap().is_none());

    let present = json!({"env": ["FOO=1", "BAR=two words"]});
    assert_eq!(
        arg_opt_str_array(&present, "env").unwrap(),
        Some(vec!["FOO=1".to_string(), "BAR=two words".to_string()])
    );

    let wrong_type = json!({"env": "FOO=1"});
    assert!(arg_opt_str_array(&wrong_type, "env").is_err());

    let wrong_element = json!({"env": ["FOO=1", 42]});
    assert!(arg_opt_str_array(&wrong_element, "env").is_err());
}
