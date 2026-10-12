"""Check isolation on real local HTTP responses, including assets and errors."""
import functools
import http.client
from pathlib import Path
import sys
import tempfile
import threading
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tools"))
from serve import Handler, Server


class QuietHandler(Handler):
    def log_message(self, format, *args):
        pass


class LocalIsolation(unittest.TestCase):
    def test_documents_workers_wasm_redirects_and_errors_are_isolated(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            fixtures = {
                "/index.html": b"<!doctype html><title>Demo</title>",
                "/worker.js": b"self.onmessage = () => {};",
                "/core.wasm": b"\x00asm\x01\x00\x00\x00",
            }
            for path, content in fixtures.items():
                (root / path.lstrip("/")).write_bytes(content)
            (root / "assets").mkdir()
            with Server(("127.0.0.1", 0), functools.partial(QuietHandler, directory=directory)) as server:
                thread = threading.Thread(target=server.serve_forever, kwargs={"poll_interval": 0.01}, daemon=True)
                thread.start()
                try:
                    cases = [("GET", path, 200) for path in fixtures]
                    cases += [("HEAD", "/index.html", 200), ("GET", "/assets", 301), ("GET", "/missing.js", 404)]
                    for method, path, status in cases:
                        with self.subTest(method=method, path=path):
                            connection = http.client.HTTPConnection(*server.server_address, timeout=5)
                            try:
                                connection.request(method, path)
                                response = connection.getresponse()
                                body = response.read()
                                self.assertEqual(response.status, status)
                                self.assertEqual(response.getheader("Cross-Origin-Opener-Policy"), "same-origin")
                                self.assertEqual(response.getheader("Cross-Origin-Embedder-Policy"), "require-corp")
                                self.assertEqual(response.getheader("Cross-Origin-Resource-Policy"), "same-origin")
                                self.assertEqual(response.getheader("Cache-Control"), "no-store")
                                if method == "GET" and path in fixtures:
                                    self.assertEqual(body, fixtures[path])
                            finally:
                                connection.close()
                finally:
                    server.shutdown()
                    thread.join(timeout=5)
                    self.assertFalse(thread.is_alive())


if __name__ == "__main__":
    unittest.main()
