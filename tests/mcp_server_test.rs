use agentic_armor::config::Config;
use agentic_armor::mcp::server::{base64_encode, default_task_mounts, is_valid_network_mode, validate_path};

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
