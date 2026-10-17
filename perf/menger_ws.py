"""ACCESS-52: the perf sheets' client — menger over its WebSocket, signed in.

ACCESS-24 took the REST bridge and the open mode away, so a perf script that
spawns menger (perf/filter-sheet.py, tree-sheet.py, large-parent.py) starts
it with `--auth-provider key --admin <a key of its own>` and speaks the
socket the way web/index.html and the probes do (`menger.call`), carrying a
session minted with that key as `Authorization: Bearer mgs_…` at the
upgrade. Python's standard library only: RFC 6455's client side (the
handshake, masked text frames, extended lengths, fragments, ping → pong,
close) and the key and session through scripts/throwaway-login.mjs (Node:
Ed25519 is not in Python's standard library).

    from menger_ws import Throwaway, Socket, wait_ready, signed_in
    who = Throwaway(tmpdir)                    # tmpdir/operator.key
    p = subprocess.Popen([bin, *who.args(), "--port", str(ws), …])
    wait_ready(p, ws)                          # an anonymous auth.providers answers
    token = signed_in(who, ws)                 # a session minted with the key
    s = Socket(ws, token)
    ms, items = s.timed("tickets.health", {})  # the data items' contents
    for raw, item in Socket(ws, token).stream("tickets.watch", {}): …
"""
import base64, json, os, socket, struct, subprocess, time

HERE = os.path.dirname(os.path.abspath(__file__))
LOGIN = os.path.join(os.path.dirname(HERE), "scripts", "throwaway-login.mjs")


class Throwaway:
    """A throwaway server's operator: a key in `directory` (never ~/.plexus)
    and the sessions minted with it."""

    def __init__(self, directory, agent="perf"):
        os.makedirs(directory, exist_ok=True)
        self.key = os.path.join(directory, "operator.key")
        self.agent = agent
        self.dir = directory
        self.principal = subprocess.run(["node", LOGIN, "key", self.key], check=True,
                                        capture_output=True, text=True).stdout.strip()

    def args(self):
        """The flags a server runs under this operator (and no REST bridge:
        `--no-rest` is inert since ACCESS-24, and keeps an older binary off
        its default REST port)."""
        return ["--auth-provider", "key", "--admin", self.principal, "--no-rest"]

    def try_session(self, port):
        """A session on :port, or None while it does not answer."""
        out = os.path.join(self.dir, f"session-{port}")
        r = subprocess.run(["node", LOGIN, "session", "--url", f"ws://127.0.0.1:{port}", "--key", self.key,
                            "--out", out, "--agent", self.agent], capture_output=True, text=True)
        if r.returncode:
            return None
        return open(out).read().strip()

    def session_file(self, port):
        return os.path.join(self.dir, f"session-{port}")


class Socket:
    """One connection to the hub on :port, signed in with `token`."""

    def __init__(self, port, token=None, timeout=600):
        self.sock = socket.create_connection(("127.0.0.1", port), timeout=timeout)
        key = base64.b64encode(os.urandom(16)).decode()
        head = [f"GET / HTTP/1.1", f"Host: 127.0.0.1:{port}", "Upgrade: websocket", "Connection: Upgrade",
                f"Sec-WebSocket-Key: {key}", "Sec-WebSocket-Version: 13"]
        if token:
            head.append(f"Authorization: Bearer {token}")
        self.sock.sendall(("\r\n".join(head) + "\r\n\r\n").encode())
        buf = b""
        while b"\r\n\r\n" not in buf:
            chunk = self.sock.recv(4096)
            if not chunk:
                raise ConnectionError("closed during the handshake")
            buf += chunk
        status, _, rest = buf.partition(b"\r\n\r\n")
        if not status.startswith(b"HTTP/1.1 101"):
            raise ConnectionError(f"upgrade refused: {status.splitlines()[0].decode(errors='replace')}")
        self.buf = rest
        self.id = 0

    # ── frames ──
    def _send(self, op, payload):
        n = len(payload)
        head = bytes([0x80 | op])
        if n < 126:
            head += bytes([0x80 | n])
        elif n < 65536:
            head += bytes([0x80 | 126]) + struct.pack(">H", n)
        else:
            head += bytes([0x80 | 127]) + struct.pack(">Q", n)
        mask = os.urandom(4)
        body = bytes(b ^ mask[i & 3] for i, b in enumerate(payload)) if n < 4096 else _xor(payload, mask)
        self.sock.sendall(head + mask + body)

    def _need(self, n):
        while len(self.buf) < n:
            chunk = self.sock.recv(1 << 16)
            if not chunk:
                raise ConnectionError("the socket closed")
            self.buf += chunk
        out, self.buf = self.buf[:n], self.buf[n:]
        return out

    def recv(self):
        """The next text message (fragments joined; pings answered)."""
        parts = []
        while True:
            b0, b1 = self._need(2)
            fin, op, n = b0 & 0x80, b0 & 0x0F, b1 & 0x7F
            if n == 126:
                n = struct.unpack(">H", self._need(2))[0]
            elif n == 127:
                n = struct.unpack(">Q", self._need(8))[0]
            mask = self._need(4) if b1 & 0x80 else None
            data = self._need(n)
            if mask:
                data = _xor(data, mask)
            if op == 0x9:
                self._send(0xA, data)
                continue
            if op == 0xA:
                continue
            if op == 0x8:
                raise ConnectionError(f"closed by menger: {data[2:].decode(errors='replace')}")
            parts.append(data)
            if fin:
                return b"".join(parts)

    def close(self):
        try:
            self._send(0x8, b"\x03\xe8")
        except OSError:
            pass
        self.sock.close()

    # ── menger.call ──
    def stream(self, method, params):
        """Each item of a `menger.call` subscription as (raw bytes, item),
        until `done` (an `error` item is the last)."""
        self.id += 1
        rid = self.id
        self._send(0x1, json.dumps({"jsonrpc": "2.0", "id": rid, "method": "menger.call",
                                    "params": {"method": method, "params": params}}).encode())
        sub = None
        while True:
            raw = self.recv()
            msg = json.loads(raw)
            if msg.get("id") == rid:
                if "error" in msg:
                    raise RuntimeError(f"{method}: {msg['error']}")
                sub = msg["result"]
                continue
            p = msg.get("params") or {}
            if sub is None or p.get("subscription") != sub:
                continue
            item = p.get("result") or {}
            if item.get("type") == "done":
                return
            yield raw, item
            if item.get("type") == "error":
                return

    def call(self, method, params):
        """The contents of the data items (an in-band refusal raises)."""
        return self.timed(method, params)[1]

    def timed(self, method, params):
        """(ms from the send to `done`, the data items' contents, bytes received)."""
        t = time.perf_counter()
        out, size = [], 0
        for raw, item in self.stream(method, params):
            size += len(raw)
            if item.get("type") == "error":
                raise RuntimeError(f"{method}: {item}")
            if item.get("type") == "data":
                out.append(item.get("content"))
        self.last_bytes = size
        return (time.perf_counter() - t) * 1000, out


def _xor(data, mask):
    n = len(data)
    m = (mask * (n // 4 + 1))[:n]
    return (int.from_bytes(data, "big") ^ int.from_bytes(m, "big")).to_bytes(n, "big")


def wait_ready(proc, port, deadline_s=120, log=None):
    """Returns once the hub on :port answers a call — `auth.providers`, the
    anonymous view's, so no sign-in rides the timing (it was `health` over
    REST before ACCESS-24). Raises if the process dies first or the
    deadline passes."""
    deadline = time.monotonic() + deadline_s
    while True:
        if proc.poll() is not None:
            raise SystemExit(f"server exited ({proc.returncode}) before serving; see {log}")
        if time.monotonic() > deadline:
            raise SystemExit(f"server never came up; see {log}")
        try:
            s = Socket(port, None, timeout=5)
            try:
                s.call("auth.providers", {})
                return
            finally:
                s.close()
        except (OSError, ConnectionError, RuntimeError, ValueError):
            time.sleep(0.05)


def signed_in(who, port):
    """A session on :port for `who` (a Throwaway), checked with `health`."""
    token = who.try_session(port)
    if not token:
        raise SystemExit(f"could not sign in to :{port} with {who.key}")
    s = Socket(port, token, timeout=30)
    try:
        s.call("tickets.health", {})
    finally:
        s.close()
    return token
