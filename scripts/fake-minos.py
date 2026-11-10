#!/usr/bin/env python3
"""A stand-in for Minos, for the checks that need a *wall* and cannot have a server.

    scripts/fake-minos.py --port 8099              # the recorded picture, and one fix every 5 s
    scripts/fake-minos.py --port 8099 --fix-every 0   # the picture only: no publications

What it serves is **recorded wire data**, `fixtures/rest/*.json`, unedited — the same files the tests read.
Nothing here invents a shape: if the fixtures are wrong, this tool reproduces the wall's reaction to wrong
fixtures, which is the point.

Why it exists: v0.1's definition of done has three items that need a running wall against a live feed —
"the map draws with no network and signs in when the network arrives", "a fix moves its marker within 1 s",
"a 30-minute soak with one reconnect" — and the wall's own account is read-only, so nobody can post a fix to
the real feed from a test. A stub that speaks the two protocols the wall speaks is how those items become
commands instead of ceremony. It is **not** shipped and it is not a dependency: Python's own standard
library, no crates, nothing in `THIRD-PARTY-NOTICES.md` (it is not in the app).

The socket is Centrifugo's JSON protocol (`?cf_protocol=json`), which is what the client speaks when the URL
asks for it; the frames are the ones `docs/spec/realtime-protocol.md` describes, at the depths it names.
"""

import argparse
import base64
import hashlib
import json
import os
import socket
import struct
import sys
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import urlparse, parse_qs

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
FIXTURES = os.path.join(ROOT, "fixtures", "rest")

# The hull the stub moves, and where it starts: the one hull in the recorded picture that has a fix.
MOVING_UNIT = 12


def fixture(name):
    with open(os.path.join(FIXTURES, name), "rb") as handle:
        return handle.read()


def moving_fix(step):
    """One `FeedPositionEvent`, drifting east by `step` half-thousandths of a degree.

    **The shape is the spec's, not a guess**: `docs/spec/realtime-protocol.md` §5 shows the event as a *flat*
    object at `pub.data` — `id_unit`, `latitude`, `longitude`, `speed_kn` … — and §4's envelope note says why
    (`live-feed` is the bare event; a client that reads `pub.id_unit` gets `undefined`). The unit, its name and
    its hull number come from the recorded picture; only the position is moved, so the stub cannot drift from
    the fixture it is standing in for.
    """
    envelope = json.loads(fixture("positions.json"))
    entries = envelope["data"]
    hull = next((entry for entry in entries if entry.get("id_unit") == MOVING_UNIT), None)
    if hull is None:
        raise SystemExit(f"fixtures/rest/positions.json has no unit {MOVING_UNIT}")
    position = hull.get("position") or {}
    now = time.strftime("%Y-%m-%dT%H:%M:%S.000000Z", time.gmtime())
    return {
        "id_unit": hull["id_unit"],
        "name": hull.get("name", ""),
        "hull_number": hull.get("hull_number", ""),
        "latitude": float(position.get("latitude", -6.1)),
        "longitude": float(position.get("longitude", 106.88)) + step * 0.0005,
        "speed_kn": 12.0,
        "speed_ms": 6.172800000000001,
        "course_deg": 90.0,
        "accuracy_m": 5.0,
        "recorded_at": now,
        "received_at": now,
        "backfilled": False,
    }


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *args):  # a stub that prints every request drowns the wall's own log
        pass

    def reply(self, body, status=200, kind="application/json"):
        if isinstance(body, (dict, list)):
            body = json.dumps(body).encode()
        self.send_response(status)
        self.send_header("Content-Type", kind)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_POST(self):
        path = urlparse(self.path).path
        length = int(self.headers.get("Content-Length") or 0)
        if length:
            self.rfile.read(length)
        if path.endswith("/auth/login") or path.endswith("/auth/refresh"):
            return self.reply(fixture("auth-login.json"))
        self.reply({"errors": [{"field": "path", "reason": "the stub has no such route"}]}, 404)

    def do_GET(self):
        parsed = urlparse(self.path)
        path = parsed.path
        # `/api/v1/...` is what a deployment's MINOS_BASE_URL ends in; the stub accepts it with or without.
        if path.startswith("/api/v1"):
            path = path[len("/api/v1"):]
        if path == "/connection/websocket":
            return self.websocket()
        if path.startswith("/live-feed/positions"):
            if path == "/live-feed/positions":
                return self.reply(fixture("positions.json"))
            unit = path.rsplit("/", 1)[-1]
            for entry in json.loads(fixture("positions.json"))["data"]:
                if str(entry.get("id_unit")) == unit:
                    return self.reply({"status_code": 200, "message": "Successfull", "data": entry})
            return self.reply(fixture("error-404.json"), 404)
        if path == "/units":
            # **The page it asked for**, from the recorded pages: answering every page with page 1 is a wall
            # that pages until its cap and reports a truncated register (measured: sixteen rounds a second,
            # `rows=64 total=127 truncated=true`), which is the stub lying about the server rather than the
            # wall being wrong. Page 3 says `has_next_page: false`, so the read ends where the wire's does.
            page = parse_qs(parsed.query).get("page", ["1"])[0]
            name = f"units-page-{page}.json"
            try:
                return self.reply(fixture(name))
            except FileNotFoundError:
                return self.reply(fixture("units-page-3.json"))
        if path.startswith("/units/"):
            rest = path[len("/units/"):]
            unit, _, what = rest.partition("/")
            if what == "commander":
                return self.reply(fixture("commander-empty.json"))
            if what == "specifications":
                return self.reply(fixture("specifications.json"))
            if what:
                return self.reply(fixture("error-404.json"), 404)
            if str(json.loads(fixture("unit.json")).get("id")) == unit or unit == "1":
                return self.reply(fixture("unit.json"))
            return self.reply(fixture("unit.json"))
        self.reply(fixture("error-404.json"), 404)

    # -- the socket ---------------------------------------------------------------------------------
    def websocket(self):
        key = self.headers.get("Sec-WebSocket-Key")
        if not key:
            return self.reply(b"not a websocket", 400, "text/plain")
        accept = base64.b64encode(
            hashlib.sha1((key + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11").encode()).digest()
        ).decode()
        self.wfile.write(
            (
                "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n"
                f"Sec-WebSocket-Accept: {accept}\r\n\r\n"
            ).encode()
        )
        self.wfile.flush()
        self.close_connection = True
        sock = self.connection
        try:
            self.serve_socket(sock)
        except (ConnectionError, OSError):
            pass

    def serve_socket(self, sock):
        subscribed = False
        step = 0
        next_fix = time.time() + ARGS.fix_every if ARGS.fix_every > 0 else None
        sock.settimeout(0.25)
        while True:
            try:
                opcode, payload = read_frame(sock)
            except socket.timeout:
                opcode, payload = None, None
            if opcode is not None:
                if opcode == 0x8:  # close
                    return
                if opcode in (0x1, 0x2):
                    try:
                        message = json.loads(payload.decode())
                    except (ValueError, UnicodeDecodeError):
                        continue
                    if ARGS.trace:
                        print(f"in  {json.dumps(message)[:200]}", flush=True)
                    answer = handle_command(message, subscribed)
                    if answer is not None:
                        send_text(sock, answer)
                        if message.get("subscribe"):
                            subscribed = True
            if next_fix is not None and time.time() >= next_fix:
                step += 1
                if ARGS.trace:
                    print(f"out push step={step}", flush=True)
                send_text(sock, {"push": {"channel": "live-feed", "pub": {"data": moving_fix(step)}}})
                next_fix = time.time() + ARGS.fix_every


ARGS = None


def handle_command(message, subscribed):
    """Centrifugo's JSON protocol, as far as a wall needs it: connect, subscribe, ping.

    The reply says `client` and `version` because a client checks them; `ping` is answered with an empty
    object, which is the whole of the keepalive the protocol asks for.
    """
    if "connect" in message:
        return {"id": message.get("id"), "connect": {"client": "fake-minos", "version": "6.0.0"}}
    if "subscribe" in message:
        return {"id": message.get("id"), "subscribe": {}}
    if "ping" in message:
        return {}
    return {"id": message.get("id"), "error": {"code": 1, "message": "the stub has no such command"}}


def read_frame(sock):
    header = recv_exact(sock, 2)
    b1, b2 = header[0], header[1]
    length = b2 & 0x7F
    if length == 126:
        length = struct.unpack(">H", recv_exact(sock, 2))[0]
    elif length == 127:
        length = struct.unpack(">Q", recv_exact(sock, 8))[0]
    mask = recv_exact(sock, 4) if b2 & 0x80 else b""
    payload = recv_exact(sock, length) if length else b""
    if mask:
        payload = bytes(byte ^ mask[i % 4] for i, byte in enumerate(payload))
    return b1 & 0x0F, payload


def recv_exact(sock, count):
    data = b""
    while len(data) < count:
        chunk = sock.recv(count - len(data))
        if not chunk:
            raise ConnectionError("the client closed")
        data += chunk
    return data


def send_text(sock, message):
    body = json.dumps(message).encode()
    header = bytearray([0x81])
    if len(body) < 126:
        header.append(len(body))
    elif len(body) < (1 << 16):
        header.append(126)
        header += struct.pack(">H", len(body))
    else:
        header.append(127)
        header += struct.pack(">Q", len(body))
    sock.sendall(bytes(header) + body)


def main():
    global ARGS
    parser = argparse.ArgumentParser(description="a stand-in for Minos, for the wall's own checks")
    # **Port 0 by default**: the stub prints the port it was given. A test tool that fights over a fixed
    # port is a test tool that fails on the machine where something else already has it.
    parser.add_argument("--port", type=int, default=0)
    parser.add_argument(
        "--fix-every",
        type=float,
        default=5.0,
        help="seconds between publications of one moving hull (0: never publish)",
    )
    parser.add_argument("--trace", action="store_true", help="print every socket frame")
    ARGS = parser.parse_args()
    server = ThreadingHTTPServer(("127.0.0.1", ARGS.port), Handler)
    server.daemon_threads = True
    port = server.server_address[1]
    # `PORT=<n>` on its own line, for whatever starts this tool to read.
    print(f"PORT={port}", flush=True)
    print(
        f"fake-minos on http://127.0.0.1:{port}/api/v1 "
        f"(socket ws://127.0.0.1:{port}/connection/websocket?cf_protocol=json, "
        f"fixes every {ARGS.fix_every}s)",
        flush=True,
    )
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass


if __name__ == "__main__":
    main()
