"""Exercise the uploader's real HTTP framing against a loopback server only."""
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import importlib.util
import json
from pathlib import Path
import threading
import unittest
import urllib.parse
import urllib.request


SCRIPT = Path(__file__).resolve().parents[1] / "upload_evidence_stream.py"


class AssetHandler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *_):
        pass

    def reply(self, status, value, truncate=False):
        body = json.dumps(value).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Connection", "close")
        self.end_headers()
        self.wfile.write(body[:-5] if truncate else body)
        self.wfile.flush()
        self.close_connection = True

    def do_POST(self):
        self.connection.settimeout(5)
        body = self.rfile.read(int(self.headers.get("Content-Length", "0")))
        self.server.requests.append((self.path, dict(self.headers.items()), body))
        query = urllib.parse.parse_qs(urllib.parse.urlsplit(self.path).query)
        asset = {"id": 42, "name": query["name"][0], "state": "uploaded",
                 "size": len(body), "digest": "sha256:" + hashlib.sha256(body).hexdigest()}
        self.server.assets.append(asset)
        self.reply(201, asset, truncate=self.server.truncate_upload_response)

    def do_GET(self):
        if urllib.parse.urlsplit(self.path).path != "/repos/owner/evidence/releases/7/assets":
            self.reply(404, {"message": "Unexpected test endpoint"})
            return
        self.reply(200, self.server.assets)


class RealHTTPUploadTests(unittest.TestCase):
    def setUp(self):
        spec = importlib.util.spec_from_file_location("evidence_http_uploader", SCRIPT)
        self.uploader = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(self.uploader)
        self.server = ThreadingHTTPServer(("127.0.0.1", 0), AssetHandler)
        self.server.daemon_threads = True
        self.server.requests = []
        self.server.assets = []
        self.server.truncate_upload_response = False
        self.thread = threading.Thread(
            target=self.server.serve_forever, kwargs={"poll_interval": 0.01}, daemon=True)
        self.thread.start()
        self.addCleanup(self.stop_server)
        # Disable proxy lookup so the fixture cannot leave loopback, even in CI.
        transport = urllib.request.build_opener(
            urllib.request.ProxyHandler({}), self.uploader._NoRedirect())

        def local_open(request, timeout):
            url = urllib.parse.urlsplit(request.full_url)
            self.assertEqual(url.scheme, "https")
            self.assertIn(url.netloc, ("api.github.com", "uploads.github.com"))
            local_url = f"http://127.0.0.1:{self.server.server_port}{url.path}?{url.query}"
            local_request = urllib.request.Request(
                local_url, data=request.data, headers=dict(request.header_items()),
                method=request.get_method())
            return transport.open(local_request, timeout=min(timeout, 5))

        self.client = self.uploader.GitHubClient("fixture-token", opener=local_open,
                                               sleep=lambda _: None)

    def stop_server(self):
        self.server.shutdown()
        self.server.server_close()
        self.thread.join(timeout=5)

    def test_memoryview_is_sent_as_exact_binary_with_fixed_content_length(self):
        # All byte values include NUL, CR/LF and 0xFF; the body exceeds HTTP buffers.
        payload = bytearray(bytes(range(256)) * 4097)
        name = "part-00001-" + "a" * 64 + ".tar.part"
        asset = self.client.upload_asset("owner/evidence", 7, name, memoryview(payload))
        self.assertEqual(len(self.server.requests), 1)
        path, headers, received = self.server.requests[0]
        headers = {key.lower(): value for key, value in headers.items()}
        self.assertEqual(received, bytes(range(256)) * 4097)
        self.assertEqual(headers["content-length"], "1048832")
        self.assertEqual(headers["content-type"], "application/octet-stream")
        self.assertEqual(headers["authorization"], "Bearer fixture-token")
        self.assertNotIn("transfer-encoding", headers)
        self.assertEqual(urllib.parse.urlsplit(path).path,
                         "/repos/owner/evidence/releases/7/assets")
        self.assertEqual(urllib.parse.parse_qs(urllib.parse.urlsplit(path).query),
                         {"name": [name]})
        self.assertEqual(asset["size"], 1048832)
        self.assertEqual(asset["digest"], "sha256:" + hashlib.sha256(received).hexdigest())

    def test_truncated_accepted_response_reconciles_without_second_post(self):
        self.server.truncate_upload_response = True
        payload = memoryview(b"\x00complete body\r\n\xff")
        digest = hashlib.sha256(payload).hexdigest()
        name = f"part-00001-{digest}.tar.part"
        assets = {}
        asset, uploaded = self.uploader._put_asset(
            self.client, "owner/evidence", {"id": 7, "draft": True},
            assets, name, payload, digest)
        self.assertFalse(uploaded)
        self.assertEqual(len(self.server.requests), 1)
        self.assertEqual(self.server.requests[0][2], b"\x00complete body\r\n\xff")
        self.assertEqual(asset["id"], 42)
        self.assertEqual(assets[name]["digest"], "sha256:" + digest)


if __name__ == "__main__":
    unittest.main()
