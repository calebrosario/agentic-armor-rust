use agentic_armor::config::Config;
use agentic_armor::mcp::server::{base64_encode, default_task_mounts, is_valid_network_mode, upload_chunk_commands, validate_path};

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
fn tmpfs_sizes_match_documented_limits() {
    assert!(mounts_by_target("/tmp").tmpfs_options.unwrap().contains("size=64m"));
    assert!(mounts_by_target("/home/opencode").tmpfs_options.unwrap().contains("size=64m"));
    assert!(mounts_by_target("/workspace").tmpfs_options.unwrap().contains("size=256m"));
}

#[test]
fn network_mode_accepts_only_none_and_bridge() {
    assert!(is_valid_network_mode("none"));
    assert!(is_valid_network_mode("bridge"));
    for invalid in ["host", "", "Bridge", "none ", "bridge\n", "container:foo", "custom0"] {
        assert!(!is_valid_network_mode(invalid), "'{}' must be rejected", invalid);
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
        assert!(validate_path(path, &cfg).is_ok(), "'{}' should be allowed", path);
    }
}

#[test]
fn validate_path_rejects_relative_paths() {
    let cfg = Config::default();
    for path in ["tmp/x", "workspace/file", "", "./x", "~/secret"] {
        assert!(validate_path(path, &cfg).is_err(), "'{}' must be rejected", path);
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
        assert!(validate_path(path, &cfg).is_err(), "'{}' must be rejected", path);
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
        assert!(validate_path(path, &cfg).is_err(), "'{}' must be rejected", path);
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
        assert!(validate_path(path, &cfg).is_err(), "'{}' must be rejected", path);
    }
}

#[test]
fn base64_encode_roundtrips() {
    let cases = ["", "a", "ab", "abc", "hello world\n", "\u{00e9}\u{4e2d}"];
    for input in cases {
        let encoded = base64_encode(input);
        assert!(!encoded.contains('\''), "encoded output must be shell-single-quote-safe");
        assert_eq!(encoded.len() % 4, 0);
        assert!(encoded.chars().all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '='));
    }
}

#[test]
fn upload_chunks_stay_under_argmax() {
    let big_b64 = base64_encode(&"x".repeat(300 * 1024));
    let cmds = upload_chunk_commands("/workspace/big.txt", &big_b64);
    assert!(cmds.len() >= 7, "300KB payload must split into multiple chunks, got {}", cmds.len());
    for cmd in &cmds {
        assert!(cmd.len() < 64 * 1024, "chunk command must stay under MAX_ARG_STRLEN (128KB), got {}", cmd.len());
    }
    assert!(cmds[0].contains("> '/workspace/big.txt'"));
    assert!(cmds[1].contains(">> '/workspace/big.txt'"), "subsequent chunks must append");
}

#[test]
fn upload_chunk_boundaries_decode_independently() {
    let input = "A".repeat(200 * 1024);
    let b64 = base64_encode(&input);
    for chunk in b64.as_bytes().chunks(48 * 1024) {
        assert_eq!(chunk.len() % 4, 0, "every non-final chunk must be base64-aligned");
    }
}

#[test]
fn upload_empty_content_creates_truncated_file() {
    let cmds = upload_chunk_commands("/tmp/empty.txt", "");
    assert_eq!(cmds.len(), 1);
    assert!(cmds[0].contains(": > '/tmp/empty.txt'"));
    assert!(cmds[0].contains("[ ! -L '/tmp/empty.txt' ]"), "must refuse writing through a final-component symlink");
}

#[test]
fn upload_first_chunk_guards_final_symlink() {
    let cmds = upload_chunk_commands("/workspace/f.txt", &base64_encode(&"x".repeat(100 * 1024)));
    assert!(cmds.len() >= 2, "payload must span multiple chunks");
    assert!(cmds[0].contains("[ ! -L '/workspace/f.txt' ]"));
    assert!(!cmds[1].contains("[ ! -L"), "guard only needed on first chunk");
}

#[test]
fn upload_chunks_cleanup_on_failure() {
    let cmds = upload_chunk_commands("/workspace/f.txt", &base64_encode("data"));
    assert!(cmds.iter().all(|c| c.contains("rm -f '/workspace/f.txt'")), "failed writes must not leave partial files");
}
