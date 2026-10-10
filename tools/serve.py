#!/usr/bin/env python3
"""Serve the local demo without stale development modules or connection bursts."""
import argparse, functools
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
ROOT=Path(__file__).resolve().parent.parent
class Handler(SimpleHTTPRequestHandler):
    def end_headers(self):
        self.send_header('Cache-Control','no-store')
        super().end_headers()
class Server(ThreadingHTTPServer):
    request_queue_size=128
def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--port',type=int,default=6411)
    a=p.parse_args()
    with Server(('127.0.0.1',a.port),functools.partial(Handler,directory=str(ROOT/'site'))) as server:
        print(f'http://localhost:{a.port}/',flush=True);server.serve_forever()
if __name__=='__main__':main()
