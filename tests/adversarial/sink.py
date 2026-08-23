import http.server
import socketserver
import threading
import time
import os

LOG = os.path.join(os.path.dirname(os.path.abspath(__file__)), "reports", "sink.log")
os.makedirs(os.path.dirname(LOG), exist_ok=True)

class Sink(http.server.BaseHTTPRequestHandler):
    def _log(self, body):
        with open(LOG, "a") as f:
            f.write(f"{time.time():.3f} {self.command} {self.path} {body[:1024]}\n")

    def do_POST(self):
        n = int(self.headers.get("Content-Length", 0) or 0)
        body = self.rfile.read(n).decode("utf-8", "replace") if n else ""
        self._log(body)
        self.send_response(200)
        self.send_header("Content-Length", "2")
        self.end_headers()
        self.wfile.write(b"ok")

    def do_GET(self):
        self._log("")
        self.send_response(200)
        self.send_header("Content-Length", "2")
        self.end_headers()
        self.wfile.write(b"ok")

    def log_message(self, *a):
        pass

class Server(socketserver.ThreadingTCPServer):
    allow_reuse_address = True

if __name__ == "__main__":
    with Server(("0.0.0.0", 9999), Sink) as httpd:
        print(f"sink listening on 0.0.0.0:9999, logging to {LOG}", flush=True)
        httpd.serve_forever()
