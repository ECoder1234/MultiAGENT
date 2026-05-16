#!/usr/bin/env python3
import json
import os
import socket
import struct
import sys
import threading
import time


MAX_MESSAGE_BYTES = 64 * 1024 * 1024
pending = {}
pending_lock = threading.Lock()
write_lock = threading.Lock()
next_id = 10000
next_id_lock = threading.Lock()


def socket_path():
    return os.path.join(
        os.environ.get("XDG_RUNTIME_DIR") or "/tmp",
        f"multiagent-codex-chrome-host-{os.geteuid()}.sock",
    )


def read_native_message():
    header = sys.stdin.buffer.read(4)
    if not header:
        return None
    if len(header) != 4:
        raise RuntimeError("incomplete native message header")
    length = struct.unpack("<I", header)[0]
    if length > MAX_MESSAGE_BYTES:
        raise RuntimeError(f"native message too large: {length}")
    payload = sys.stdin.buffer.read(length)
    if len(payload) != length:
        raise RuntimeError("incomplete native message payload")
    return json.loads(payload.decode("utf-8"))


def write_native_message(message):
    payload = json.dumps(message, separators=(",", ":")).encode("utf-8")
    with write_lock:
        sys.stdout.buffer.write(struct.pack("<I", len(payload)))
        sys.stdout.buffer.write(payload)
        sys.stdout.buffer.flush()


def allocate_id():
    global next_id
    with next_id_lock:
        value = next_id
        next_id += 1
        return value


def send_to_extension(method, params):
    request_id = allocate_id()
    event = threading.Event()
    slot = {"response": None}
    with pending_lock:
        pending[request_id] = (event, slot)
    write_native_message(
        {"jsonrpc": "2.0", "id": request_id, "method": method, "params": params or {}}
    )
    if not event.wait(30):
        with pending_lock:
            pending.pop(request_id, None)
        return {
            "jsonrpc": "2.0",
            "id": request_id,
            "error": {
                "code": -32000,
                "message": "Timed out waiting for Chrome extension response",
            },
        }
    return slot["response"]


def handle_client(conn):
    with conn:
        raw = b""
        while b"\n" not in raw:
            chunk = conn.recv(65536)
            if not chunk:
                return
            raw += chunk
        request = json.loads(raw.split(b"\n", 1)[0].decode("utf-8"))
        response = send_to_extension(request.get("method"), request.get("params") or {})
        conn.sendall(json.dumps(response, separators=(",", ":")).encode("utf-8") + b"\n")


def broker_loop():
    path = socket_path()
    try:
        os.unlink(path)
    except FileNotFoundError:
        pass
    server = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    server.bind(path)
    os.chmod(path, 0o600)
    server.listen(16)
    while True:
        conn, _ = server.accept()
        threading.Thread(target=handle_client, args=(conn,), daemon=True).start()


def handle_incoming(message):
    if "method" not in message and ("result" in message or "error" in message):
        request_id = message.get("id")
        with pending_lock:
            item = pending.pop(request_id, None)
        if item:
            event, slot = item
            slot["response"] = message
            event.set()
        return

    method = message.get("method")
    request_id = message.get("id")
    if request_id is None:
        return
    if method == "ping":
        write_native_message({"jsonrpc": "2.0", "id": request_id, "result": "pong"})
        return
    write_native_message(
        {
            "jsonrpc": "2.0",
            "id": request_id,
            "error": {"code": -32601, "message": f"No handler registered for method: {method}"},
        }
    )


def main():
    threading.Thread(target=broker_loop, daemon=True).start()
    while True:
        message = read_native_message()
        if message is None:
            return
        handle_incoming(message)


if __name__ == "__main__":
    try:
        main()
    except Exception as exc:
        print(f"chrome native host failed: {exc}", file=sys.stderr, flush=True)
        time.sleep(0.1)
        raise
