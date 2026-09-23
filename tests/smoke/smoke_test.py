#!/usr/bin/env python3
"""Runtime smoke test for agentic-armor: drives the real binary over MCP stdio
and exercises the paths unit tests cannot reach (live container round-trips).

Covers: task lifecycle, taskId length contract, binary upload/download
round-trip (encoding=base64), text round-trip (encoding=utf8), task_exec env
injection, JSON-argv audit trail, cleanup.

Usage:
    python3 tests/smoke/smoke_test.py          # uses target/debug binary
    ARMOR_BIN=/path/to/binary python3 tests/smoke/smoke_test.py
    ARMOR_IMAGE=<allowed-image> ALLOWED_IMAGES=<allowed-image> \\
        python3 tests/smoke/smoke_test.py      # non-default image

Exits 0 on success, 1 on any failure (best-effort cleanup either way).
Requires a reachable Docker/Podman daemon and an allowed sandbox image.
"""
import base64
import json
import os
import subprocess
import sys

BIN = os.environ.get(
    "ARMOR_BIN",
    os.path.join(os.path.dirname(__file__), "..", "..", "target", "debug", "agentic-armor"),
)
TASK_ID = "smoke-1"
BINARY_BYTES = b"\x00\x01\x02\xff\xfe\x00"


class McpClient:
    def __init__(self, binary_path):
        self.proc = subprocess.Popen(
            [binary_path],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            cwd=os.path.join(os.path.dirname(__file__), "..", ".."),
        )
        self._id = 0

    def _send(self, obj):
        self.proc.stdin.write((json.dumps(obj) + "\n").encode())
        self.proc.stdin.flush()

    def request(self, method, params=None):
        self._id += 1
        req = {
            "jsonrpc": "2.0",
            "id": self._id,
            "method": method,
            "params": params or {},
        }
        self._send(req)
        while True:
            line = self.proc.stdout.readline()
            if not line:
                raise RuntimeError(f"binary closed stdout while awaiting {method}")
            line = line.strip()
            if not line:
                continue
            msg = json.loads(line)
            if msg.get("id") == self._id:
                if "error" in msg:
                    raise RuntimeError(f"{method} rpc error: {msg['error']}")
                return msg["result"]

    def notify(self, method, params=None):
        self._send({"jsonrpc": "2.0", "method": method, "params": params or {}})

    def close(self):
        try:
            self.proc.stdin.close()
            self.proc.wait(timeout=10)
        except Exception:
            self.proc.kill()


def call_tool(client, name, arguments):
    result = client.request(
        "tools/call", {"name": name, "arguments": arguments}
    )
    text = result["content"][0]["text"]
    try:
        return json.loads(text)
    except json.JSONDecodeError:
        return {"_raw": text, "_isError": result.get("isError", False)}


def check(label, condition, detail=""):
    status = "ok" if condition else "FAIL"
    print(f"[{status}] {label}" + (f" — {detail}" if detail and not condition else ""))
    if not condition:
        raise AssertionError(label)


def main():
    client = McpClient(BIN)
    created = False
    try:
        init = client.request(
            "initialize",
            {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {"name": "smoke-test", "version": "0.0.1"},
            },
        )
        check("initialize", init.get("serverInfo", {}).get("name") == "agentic-armor")
        client.notify("notifications/initialized")

        create_args = {"taskId": TASK_ID, "name": "smoke"}
        image = os.environ.get("ARMOR_IMAGE")
        if image:
            create_args["image"] = image
        created_resp = call_tool(client, "task_create", create_args)
        check("task_create", created_resp.get("success") is True
              and created_resp.get("status") == "running",
              str(created_resp))
        created = True

        long_resp = call_tool(
            client, "task_create", {"taskId": "a" * 59, "name": "too-long"}
        )
        check("taskId > 58 rejected", long_resp.get("_isError") is True
              or long_resp.get("success") is not True, str(long_resp))

        # Binary file written inside the container, then downloaded via base64.
        bin_sh = "printf '" + "".join(f"\\x{b:02x}" for b in BINARY_BYTES) + "' > /tmp/bin.dat"
        exec_resp = call_tool(
            client, "task_exec", {"taskId": TASK_ID, "command": ["sh", "-c", bin_sh]}
        )
        check("write binary in container", exec_resp.get("exitCode") == 0, str(exec_resp))

        dl = call_tool(client, "task_download", {"taskId": TASK_ID, "path": "/tmp/bin.dat"})
        check("download reports base64", dl.get("encoding") == "base64", str(dl)[:200])
        decoded = base64.b64decode(dl.get("content", ""))
        check("binary round-trip byte-identical", decoded == BINARY_BYTES,
              f"got {decoded!r}, want {BINARY_BYTES!r}")
        check("download bytes exact", dl.get("bytes") == len(BINARY_BYTES), str(dl.get("bytes")))

        env_resp = call_tool(
            client,
            "task_exec",
            {
                "taskId": TASK_ID,
                "command": ["sh", "-c", "echo $SMOKE_VAR"],
                "env": ["SMOKE_VAR=hello-env"],
            },
        )
        check("task_exec env reaches payload",
              env_resp.get("stdout", "").strip() == "hello-env", str(env_resp)[:200])

        text_payload = "plain text\nwith unicode ✓\n"
        up = call_tool(
            client,
            "task_upload",
            {"taskId": TASK_ID, "path": "/tmp/text.txt", "content": text_payload},
        )
        check("task_upload", up.get("success") is True, str(up))
        dl2 = call_tool(client, "task_download", {"taskId": TASK_ID, "path": "/tmp/text.txt"})
        check("text round-trip exact",
              dl2.get("encoding") == "utf8" and dl2.get("content") == text_payload,
              str(dl2)[:200])

        logs = call_tool(client, "task_logs", {"taskId": TASK_ID, "limit": 50})
        messages = [e.get("message", "") for e in logs.get("logs", [])]
        argv_logged = any(
            m.startswith("exec ") and '["sh","-c"' in m for m in messages
        )
        check("audit trail carries JSON argv", argv_logged,
              "; ".join(messages[:3])[:200])

        print("\nSMOKE TEST PASSED")
        return 0
    except AssertionError as e:
        print(f"\nSMOKE TEST FAILED: {e}")
        return 1
    finally:
        if created:
            try:
                call_tool(client, "task_delete", {"taskId": TASK_ID})
            except Exception as e:
                print(f"[warn] cleanup task_delete failed: {e}", file=sys.stderr)
        client.close()


if __name__ == "__main__":
    sys.exit(main())
