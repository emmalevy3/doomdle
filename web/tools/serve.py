"""Development server for the site: like `python3 -m http.server`, but tells
the browser never to cache, so edited scripts and styles always load fresh.

Usage, from the repository root:  python3 web/tools/serve.py [port]
"""

import functools
import http.server
import pathlib
import sys

WEB = pathlib.Path(__file__).resolve().parent.parent


class NoCacheHandler(http.server.SimpleHTTPRequestHandler):
    def end_headers(self):
        self.send_header("Cache-Control", "no-store")
        super().end_headers()


def main():
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 8000
    handler = functools.partial(NoCacheHandler, directory=str(WEB))
    with http.server.ThreadingHTTPServer(("", port), handler) as server:
        print(f"Serving {WEB} at http://localhost:{port}/ (no caching)")
        server.serve_forever()


if __name__ == "__main__":
    main()
