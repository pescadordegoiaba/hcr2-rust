#!/usr/bin/env python3
"""Serve the game and open it in the browser.

A wasm module cannot be fetched over file://, so this starts a tiny static
server rooted at this folder and opens the page.
"""

import functools
import http.server
import socket
import webbrowser
from pathlib import Path

HOST = "127.0.0.1"
ROOT = Path(__file__).resolve().parent
PAGE = "/index.html"


def free_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
        sock.bind((HOST, 0))
        return sock.getsockname()[1]


def main() -> None:
    port = free_port()
    handler = functools.partial(http.server.SimpleHTTPRequestHandler, directory=str(ROOT))
    server = http.server.ThreadingHTTPServer((HOST, port), handler)
    url = f"http://{HOST}:{port}{PAGE}"
    print(f"Servindo {ROOT}")
    print(url)
    print("Ctrl+C encerra.")
    webbrowser.open(url)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        print()
    finally:
        server.server_close()


if __name__ == "__main__":
    main()
