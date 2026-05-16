#!/usr/bin/env python3
import atexit
import glob
import json
import os
import shutil
import socket
import sqlite3
import subprocess
import sys
import tempfile
import time
from datetime import datetime, timezone


SESSION_ID = os.environ.get("MULTIAGENT_CHROME_SESSION_ID", "multiagent-codex-chrome")
TURN_ID = os.environ.get("MULTIAGENT_CHROME_TURN_ID", f"mcp-{int(time.time() * 1000)}")
SERVER_NAME = "Multiagent-chrome"
DEFAULT_REQUEST_TIMEOUT = float(os.environ.get("MULTIAGENT_CHROME_REQUEST_TIMEOUT", "12"))
DEFAULT_CDP_TIMEOUT_MS = int(os.environ.get("MULTIAGENT_CHROME_CDP_TIMEOUT_MS", "10000"))
MAX_EVENTS = 250
CHROME_EPOCH_OFFSET_SECONDS = 11644473600


def enabled_features():
    raw = os.environ.get("MULTIAGENT_CHROME_FEATURES", "core")
    values = {part.strip().lower() for part in raw.split(",") if part.strip()}
    if "all" in values:
        return {"core", "tabs", "history", "downloads", "cursor", "cdp", "lifecycle"}
    values.add("core")
    return values


FEATURES = enabled_features()


def feature_enabled(name):
    return name == "core" or name in FEATURES


def now_ms():
    return int(time.time() * 1000)


def make_turn_id(prefix="mcp"):
    return f"{prefix}-{now_ms()}"


class Lifecycle:
    def __init__(self):
        self.state = "initializing"
        self.sequence = 0
        self.events = []
        self.emit("initializing", "Bridge process started")

    def emit(self, event, message=None, data=None):
        self.sequence += 1
        item = {
            "sequence": self.sequence,
            "time": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
            "state": self.state,
            "event": event,
        }
        if message is not None:
            item["message"] = message
        if data is not None:
            item["data"] = data
        self.events.append(item)
        if len(self.events) > MAX_EVENTS:
            del self.events[: len(self.events) - MAX_EVENTS]
        return item

    def set_state(self, state, message=None, data=None):
        if self.state != state:
            self.state = state
            return self.emit(f"state:{state}", message, data)
        if message or data:
            return self.emit(f"state:{state}:note", message, data)
        return None

    def status(self):
        return {
            "state": self.state,
            "sequence": self.sequence,
            "features": sorted(FEATURES),
            "serverName": SERVER_NAME,
            "sessionId": SESSION_ID,
        }

    def since(self, sequence=0, limit=100):
        items = [event for event in self.events if event["sequence"] > sequence]
        return items[: max(1, min(int(limit), MAX_EVENTS))]


LIFECYCLE = Lifecycle()


def close_lifecycle():
    if LIFECYCLE.state not in {"closing", "closed"}:
        LIFECYCLE.set_state("closing", "Bridge process exiting")
    if LIFECYCLE.state != "closed":
        LIFECYCLE.set_state("closed", "Bridge process closed")


atexit.register(close_lifecycle)


def socket_path():
    return os.environ.get(
        "MULTIAGENT_CHROME_BRIDGE_SOCKET",
        os.path.join(
            os.environ.get("XDG_RUNTIME_DIR") or "/tmp",
            f"multiagent-codex-chrome-host-{os.geteuid()}.sock",
        ),
    )


def normalize_bridge_params(params):
    params = dict(params or {})
    session_id = params.pop("sessionId", None)
    turn_id = params.pop("turnId", None)
    if isinstance(session_id, str) and session_id:
        params["session_id"] = session_id
    if isinstance(turn_id, str) and turn_id:
        params["turn_id"] = turn_id
    params.setdefault("session_id", SESSION_ID)
    params.setdefault("turn_id", TURN_ID)
    return params


def chrome_request(method, params=None, timeout=DEFAULT_REQUEST_TIMEOUT):
    params = normalize_bridge_params(params)
    path = socket_path()
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as client:
        client.settimeout(timeout)
        client.connect(path)
        client.sendall(
            (json.dumps({"method": method, "params": params}, separators=(",", ":")) + "\n").encode()
        )
        chunks = []
        while True:
            chunk = client.recv(65536)
            if not chunk:
                break
            chunks.append(chunk)
            if b"\n" in chunk:
                break
    raw = b"".join(chunks).split(b"\n", 1)[0]
    if not raw:
        raise RuntimeError("Chrome bridge returned an empty response")
    response = json.loads(raw)
    if "error" in response:
        error = response["error"]
        if isinstance(error, dict):
            raise RuntimeError(error.get("message") or json.dumps(error))
        raise RuntimeError(str(error))
    return response.get("result")


def text_result(value):
    if not isinstance(value, str):
        value = json.dumps(value, indent=2, sort_keys=True)
    return {"content": [{"type": "text", "text": value}], "isError": False}


def error_result(message):
    return {"content": [{"type": "text", "text": str(message)}], "isError": True}


def is_uncertain_bridge_response(exc):
    message = str(exc)
    return isinstance(exc, TimeoutError) or "empty response" in message or "Timed out waiting" in message


def require_tab(params):
    tab_id = params.get("tabId")
    if not isinstance(tab_id, int):
        raise ValueError("tabId must be an integer")
    return tab_id


def require_url(params):
    url = params.get("url")
    if not isinstance(url, str) or not url.strip():
        raise ValueError("url is required")
    return url.strip()


def require_bool(params, name, default=False):
    value = params.get(name, default)
    if not isinstance(value, bool):
        raise ValueError(f"{name} must be a boolean")
    return value


def require_int(params, name, default=None, minimum=None, maximum=None):
    value = params.get(name, default)
    if not isinstance(value, int):
        raise ValueError(f"{name} must be an integer")
    if minimum is not None and value < minimum:
        raise ValueError(f"{name} must be at least {minimum}")
    if maximum is not None and value > maximum:
        raise ValueError(f"{name} must be at most {maximum}")
    return value


def parse_tab_ids(params, name="tabIds"):
    value = params.get(name)
    if not isinstance(value, list) or not value:
        raise ValueError(f"{name} must be a non-empty array of integer tab IDs")
    tab_ids = []
    for tab_id in value:
        if not isinstance(tab_id, int):
            raise ValueError(f"{name} must contain only integers")
        tab_ids.append(tab_id)
    return tab_ids


def find_chrome_binary():
    configured = os.environ.get("MULTIAGENT_CHROME_BINARY") or os.environ.get("CHROME_BINARY")
    candidates = [
        configured,
        "google-chrome",
        "google-chrome-stable",
        "chromium-browser",
        "chromium",
        "brave-browser",
        "microsoft-edge",
    ]
    for candidate in candidates:
        if not candidate:
            continue
        resolved = shutil.which(candidate)
        if resolved:
            return resolved
    raise RuntimeError("Could not find a Chrome/Chromium browser binary")


def tool_get_tabs(_params):
    return chrome_request("getUserTabs", {})


def find_tab_by_url(url):
    try:
        tabs = tool_get_tabs({})
    except Exception:
        return None
    if not isinstance(tabs, list):
        return None
    for tab in tabs:
        if tab.get("url") == url:
            return tab
    return None


def open_url_in_chrome(url):
    before = find_tab_by_url(url)
    if before is not None:
        return {"tabId": before.get("id"), "url": before.get("url"), "title": before.get("title")}
    subprocess.Popen(
        [find_chrome_binary(), "--new-tab", url],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        start_new_session=True,
    )
    for _ in range(40):
        time.sleep(0.25)
        tab = find_tab_by_url(url)
        if tab is not None:
            return {"tabId": tab.get("id"), "url": tab.get("url"), "title": tab.get("title")}
    return {"tabId": None, "url": url, "opened": True}


class CdpSessionManager:
    def __init__(self):
        self.sessions = {}

    def mark(self, tab_id, state, error=None):
        item = self.sessions.get(tab_id, {"tabId": tab_id, "attachedAt": None, "lastUsedAt": None})
        item["state"] = state
        item["lastUsedAt"] = datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")
        if state == "attached" and item.get("attachedAt") is None:
            item["attachedAt"] = item["lastUsedAt"]
        if error:
            item["error"] = str(error)
        elif "error" in item:
            del item["error"]
        self.sessions[tab_id] = item
        LIFECYCLE.emit("cdp:state", data=item)
        return item

    def attach(self, tab_id, turn_id=None, retries=2, timeout=8):
        turn_id = turn_id or TURN_ID
        last_error = None
        chrome_request("claimUserTab", {"tabId": tab_id, "turn_id": turn_id}, timeout=timeout)
        for attempt in range(max(0, retries) + 1):
            try:
                chrome_request("attach", {"tabId": tab_id, "turn_id": turn_id}, timeout=timeout)
                return self.mark(tab_id, "attached")
            except RuntimeError as exc:
                last_error = exc
                if "Another debugger" in str(exc):
                    state = self.mark(tab_id, "attached", "Another debugger is already attached")
                    state["sharedDebugger"] = True
                    return state
                time.sleep(min(0.25 * (attempt + 1), 1.0))
        self.mark(tab_id, "detached", last_error)
        raise last_error

    def detach(self, tab_id, turn_id=None):
        try:
            result = chrome_request(
                "detach",
                {"tabId": tab_id, "turn_id": turn_id or TURN_ID},
                timeout=8,
            )
        finally:
            self.mark(tab_id, "detached")
        return result

    def detach_all_best_effort(self):
        results = []
        for tab_id in list(self.sessions):
            try:
                results.append({"tabId": tab_id, "result": self.detach(tab_id)})
            except Exception as exc:
                results.append({"tabId": tab_id, "error": str(exc)})
        return results

    def execute(self, tab_id, method, command_params=None, turn_id=None, timeout_ms=None, retries=2):
        turn_id = turn_id or TURN_ID
        timeout_ms = timeout_ms or DEFAULT_CDP_TIMEOUT_MS
        command_params = command_params or {}
        last_error = None
        for attempt in range(max(0, retries) + 1):
            try:
                self.attach(tab_id, turn_id=turn_id, retries=1)
                result = chrome_request(
                    "executeCdp",
                    {
                        "target": {"tabId": tab_id},
                        "method": method,
                        "commandParams": command_params,
                        "timeoutMs": timeout_ms,
                        "turn_id": turn_id,
                    },
                    timeout=max(3, min((timeout_ms / 1000) + 2, 30)),
                )
                self.mark(tab_id, "attached")
                return result
            except Exception as exc:
                last_error = exc
                lowered = str(exc).lower()
                if "crash" in lowered or "target closed" in lowered:
                    self.mark(tab_id, "crashed", exc)
                else:
                    self.mark(tab_id, "detached", exc)
                if attempt < retries:
                    time.sleep(min(0.4 * (attempt + 1), 1.5))
                    continue
        raise last_error

    def status(self, tab_id=None):
        if tab_id is None:
            return {"sessions": list(self.sessions.values())}
        return self.sessions.get(tab_id, {"tabId": tab_id, "state": "detached"})


CDP = CdpSessionManager()


def tool_create_tab(params):
    url = params.get("url")
    if isinstance(url, str) and url.strip():
        opened = open_url_in_chrome(url.strip())
        if opened.get("tabId") is not None and require_bool(params, "claim", False):
            tool_claim_tab({"tabId": opened["tabId"]})
        return opened
    return chrome_request("createTab", {})


def tool_navigate(params):
    url = require_url(params)
    tab_id = params.get("tabId")
    if tab_id is not None and not isinstance(tab_id, int):
        raise ValueError("tabId must be an integer")
    opened = open_url_in_chrome(url)
    if tab_id is not None and opened.get("tabId") != tab_id:
        opened["requestedTabId"] = tab_id
    return opened


def tool_claim_tab(params):
    tab_id = require_tab(params)
    try:
        result = chrome_request("claimUserTab", {"tabId": tab_id})
    except Exception as exc:
        if not is_uncertain_bridge_response(exc):
            raise
        LIFECYCLE.emit("tab:claim-timeout", str(exc), {"tabId": tab_id})
        return {
            "tabId": tab_id,
            "claimed": None,
            "timedOut": True,
            "message": "The extension did not return a claim response before the bridge timeout.",
        }
    LIFECYCLE.emit("tab:claimed", data={"tabId": tab_id})
    return result or {"tabId": tab_id, "claimed": True}


def tool_group_tabs(params):
    tab_ids = parse_tab_ids(params)
    title = params.get("title") or params.get("name")
    if title is not None and not isinstance(title, str):
        raise ValueError("title must be a string")
    claimed = []
    warnings = []
    for tab_id in tab_ids:
        try:
            chrome_request("claimUserTab", {"tabId": tab_id})
        except Exception as exc:
            if not is_uncertain_bridge_response(exc):
                raise
            warnings.append(f"claimUserTab({tab_id}) returned uncertain response: {exc}")
        claimed.append(tab_id)
    if title and title.strip():
        try:
            chrome_request("nameSession", {"name": title.strip()})
        except Exception as exc:
            if not is_uncertain_bridge_response(exc):
                raise
            warnings.append(f"nameSession returned uncertain response: {exc}")
    tabs = tool_get_tabs({})
    grouped = [
        tab for tab in tabs
        if tab.get("id") in claimed and (not title or tab.get("tabGroup") == title.strip())
    ] if isinstance(tabs, list) else []
    LIFECYCLE.emit("tabs:grouped", data={"tabIds": claimed, "title": title, "warnings": warnings})
    return {
        "sessionId": SESSION_ID,
        "claimedTabIds": claimed,
        "title": title,
        "tabs": tabs,
        "verifiedGroupedTabIds": [tab.get("id") for tab in grouped],
        "warnings": warnings,
        "note": "Tabs were moved into the extension-managed Chrome session tab group.",
    }


def tool_finalize_tabs(params):
    keep = params.get("keep", [])
    if not isinstance(keep, list):
        raise ValueError("keep must be an array")
    normalized = []
    for item in keep:
        if not isinstance(item, dict):
            raise ValueError("keep entries must be objects")
        tab_id = item.get("tabId")
        status = item.get("status", "handoff")
        if not isinstance(tab_id, int):
            raise ValueError("keep entries require integer tabId")
        if status not in {"handoff", "deliverable"}:
            raise ValueError("keep status must be handoff or deliverable")
        normalized.append({"tabId": tab_id, "status": status})
    warnings = []
    try:
        result = chrome_request("finalizeTabs", {"keep": normalized})
    except Exception as exc:
        if not is_uncertain_bridge_response(exc):
            raise
        result = None
        warnings.append(f"finalizeTabs returned uncertain response: {exc}")
    CDP.detach_all_best_effort()
    tabs = tool_get_tabs({})
    LIFECYCLE.emit("tabs:finalized", data={"keep": normalized, "warnings": warnings})
    return result or {
        "finalized": not warnings,
        "finalizedUnknown": bool(warnings),
        "keep": normalized,
        "tabs": tabs,
        "warnings": warnings,
    }


def tool_name_session(params):
    name = params.get("name") or params.get("title")
    if not isinstance(name, str) or not name.strip():
        raise ValueError("name is required")
    result = chrome_request("nameSession", {"name": name.strip()})
    LIFECYCLE.emit("tabs:named", data={"name": name.strip()})
    return result or {"sessionId": SESSION_ID, "name": name.strip()}


def parse_date_ms(value, label):
    if value is None:
        return None
    if isinstance(value, (int, float)):
        return float(value)
    if not isinstance(value, str):
        raise ValueError(f"{label} must be a date string or timestamp")
    text = value.strip()
    if not text:
        return None
    if text.isdigit():
        return float(text)
    try:
        parsed = datetime.fromisoformat(text.replace("Z", "+00:00"))
    except ValueError as exc:
        raise ValueError(f"{label} must be a valid ISO date") from exc
    if parsed.tzinfo is None:
        parsed = parsed.replace(tzinfo=timezone.utc)
    return parsed.timestamp() * 1000


def iso_from_ms(value):
    if value is None:
        return None
    return datetime.fromtimestamp(float(value) / 1000, tz=timezone.utc).isoformat().replace("+00:00", "Z")


def chrome_time_to_ms(value):
    if value is None:
        return None
    try:
        value = int(value)
    except (TypeError, ValueError):
        return None
    if value <= 0:
        return None
    return (value / 1000) - (CHROME_EPOCH_OFFSET_SECONDS * 1000)


def ms_to_chrome_time(value):
    return int((float(value) + (CHROME_EPOCH_OFFSET_SECONDS * 1000)) * 1000)


def chrome_profile_roots():
    configured = os.environ.get("MULTIAGENT_CHROME_PROFILE")
    if configured:
        return [configured]
    home = os.path.expanduser("~")
    roots = [
        os.path.join(home, ".config", "google-chrome"),
        os.path.join(home, ".config", "chromium"),
        os.path.join(home, ".config", "BraveSoftware", "Brave-Browser"),
        os.path.join(home, ".config", "microsoft-edge"),
    ]
    return [root for root in roots if os.path.isdir(root)]


def candidate_history_paths():
    configured = os.environ.get("MULTIAGENT_CHROME_HISTORY_DB")
    if configured:
        return [configured]
    paths = []
    for root in chrome_profile_roots():
        paths.extend(glob.glob(os.path.join(root, "Default", "History")))
        paths.extend(glob.glob(os.path.join(root, "Profile *", "History")))
    seen = set()
    result = []
    for path in paths:
        if path not in seen and os.path.isfile(path):
            seen.add(path)
            result.append(path)
    return result


def open_history_db(write=False):
    paths = candidate_history_paths()
    if not paths:
        raise RuntimeError("Could not find a Chrome History database")
    errors = []
    for path in paths:
        try:
            if write:
                return sqlite3.connect(path), path
            uri = f"file:{path}?mode=ro&immutable=1"
            return sqlite3.connect(uri, uri=True), path
        except sqlite3.Error as exc:
            errors.append(f"{path}: {exc}")
    raise RuntimeError("Could not open Chrome History database: " + "; ".join(errors))


def query_history_db(params):
    query = params.get("query")
    url_filter = params.get("url")
    title_filter = params.get("title")
    min_visit_count = params.get("minVisitCount")
    limit = require_int(params, "limit", 100, minimum=1, maximum=1000)
    from_ms = parse_date_ms(params.get("from"), "from")
    to_ms = parse_date_ms(params.get("to"), "to")
    clauses = []
    values = []
    if isinstance(query, str) and query.strip():
        clauses.append("(url LIKE ? OR title LIKE ?)")
        values.extend([f"%{query.strip()}%", f"%{query.strip()}%"])
    if isinstance(url_filter, str) and url_filter.strip():
        clauses.append("url LIKE ?")
        values.append(f"%{url_filter.strip()}%")
    if isinstance(title_filter, str) and title_filter.strip():
        clauses.append("title LIKE ?")
        values.append(f"%{title_filter.strip()}%")
    if isinstance(min_visit_count, int):
        clauses.append("visit_count >= ?")
        values.append(min_visit_count)
    if from_ms is not None:
        clauses.append("last_visit_time >= ?")
        values.append(ms_to_chrome_time(from_ms))
    if to_ms is not None:
        clauses.append("last_visit_time <= ?")
        values.append(ms_to_chrome_time(to_ms))
    where = " WHERE " + " AND ".join(clauses) if clauses else ""
    sql = (
        "SELECT url, title, visit_count, typed_count, last_visit_time "
        f"FROM urls{where} ORDER BY last_visit_time DESC LIMIT ?"
    )
    values.append(limit)
    conn, path = open_history_db(write=False)
    try:
        rows = conn.execute(sql, values).fetchall()
    finally:
        conn.close()
    items = []
    for url, title, visit_count, typed_count, last_visit_time in rows:
        last_ms = chrome_time_to_ms(last_visit_time)
        item = {
            "url": url,
            "title": title or "",
            "visitCount": visit_count,
            "typedCount": typed_count,
            "source": "history-db",
            "profileHistoryPath": path,
        }
        if last_ms is not None:
            item["dateVisited"] = iso_from_ms(last_ms)
        items.append(item)
    return items


def tool_history_search(params):
    source = params.get("source", "auto")
    if source not in {"auto", "extension", "history-db"}:
        raise ValueError("source must be auto, extension, or history-db")
    if source == "auto" and isinstance(params.get("minVisitCount"), int):
        source = "history-db"
    if source in {"auto", "extension"}:
        try:
            payload = {
                "query": params.get("query") or params.get("url") or params.get("title") or "",
                "limit": require_int(params, "limit", 100, minimum=1, maximum=1000),
            }
            if params.get("from") is not None:
                payload["from"] = params.get("from")
            if params.get("to") is not None:
                payload["to"] = params.get("to")
            items = chrome_request("getUserHistory", payload)
            if source == "extension":
                return {"items": filter_history_items(items, params), "source": "extension"}
        except Exception as exc:
            if source == "extension":
                raise
            LIFECYCLE.emit("history:extension-fallback", str(exc))
    items = query_history_db(params)
    return {"items": filter_history_items(items, params), "source": "history-db"}


def filter_history_items(items, params):
    if not isinstance(items, list):
        return []
    url_filter = params.get("url")
    title_filter = params.get("title")
    min_visit_count = params.get("minVisitCount")
    from_ms = parse_date_ms(params.get("from"), "from")
    to_ms = parse_date_ms(params.get("to"), "to")
    filtered = []
    for item in items:
        if not isinstance(item, dict):
            continue
        if isinstance(url_filter, str) and url_filter.strip():
            if url_filter.strip().lower() not in str(item.get("url", "")).lower():
                continue
        if isinstance(title_filter, str) and title_filter.strip():
            if title_filter.strip().lower() not in str(item.get("title", "")).lower():
                continue
        if isinstance(min_visit_count, int) and item.get("visitCount") is not None:
            if int(item.get("visitCount", 0)) < min_visit_count:
                continue
        date_value = parse_date_ms(item.get("dateVisited"), "dateVisited") if item.get("dateVisited") else None
        if from_ms is not None and date_value is not None and date_value < from_ms:
            continue
        if to_ms is not None and date_value is not None and date_value > to_ms:
            continue
        filtered.append(item)
    return filtered


def tool_history_clear(params):
    if os.environ.get("MULTIAGENT_CHROME_ALLOW_HISTORY_MUTATION") != "1":
        return {
            "cleared": False,
            "requiresOptIn": True,
            "message": "Set MULTIAGENT_CHROME_ALLOW_HISTORY_MUTATION=1 to allow destructive history changes.",
        }
    url = params.get("url")
    all_entries = require_bool(params, "all", False)
    from_ms = parse_date_ms(params.get("from"), "from")
    to_ms = parse_date_ms(params.get("to"), "to")
    if not all_entries and not url and from_ms is None and to_ms is None:
        raise ValueError("history_clear requires all=true, url, from, or to")
    clauses = []
    values = []
    if not all_entries:
        if isinstance(url, str) and url.strip():
            clauses.append("url LIKE ?")
            values.append(f"%{url.strip()}%")
        if from_ms is not None:
            clauses.append("last_visit_time >= ?")
            values.append(ms_to_chrome_time(from_ms))
        if to_ms is not None:
            clauses.append("last_visit_time <= ?")
            values.append(ms_to_chrome_time(to_ms))
    where = " WHERE " + " AND ".join(clauses) if clauses else ""
    conn, path = open_history_db(write=True)
    try:
        ids = [row[0] for row in conn.execute(f"SELECT id FROM urls{where}", values).fetchall()]
        if ids:
            placeholders = ",".join(["?"] * len(ids))
            conn.execute(f"DELETE FROM visits WHERE url IN ({placeholders})", ids)
            conn.execute(f"DELETE FROM urls WHERE id IN ({placeholders})", ids)
            conn.commit()
    finally:
        conn.close()
    LIFECYCLE.emit("history:cleared", data={"count": len(ids), "path": path})
    return {"cleared": True, "count": len(ids), "profileHistoryPath": path}


def downloads_columns(conn):
    try:
        return {row[1] for row in conn.execute("PRAGMA table_info(downloads)").fetchall()}
    except sqlite3.Error:
        return set()


def tool_downloads_list(params):
    limit = require_int(params, "limit", 100, minimum=1, maximum=1000)
    conn, path = open_history_db(write=False)
    try:
        columns = downloads_columns(conn)
        if not columns:
            return {"items": [], "source": "history-db", "profileHistoryPath": path}
        selected = [name for name in [
            "id",
            "target_path",
            "tab_url",
            "referrer",
            "start_time",
            "end_time",
            "received_bytes",
            "total_bytes",
            "state",
            "danger_type",
            "interrupt_reason",
            "mime_type",
        ] if name in columns]
        rows = conn.execute(
            f"SELECT {', '.join(selected)} FROM downloads ORDER BY start_time DESC LIMIT ?",
            [limit],
        ).fetchall()
        items = []
        for row in rows:
            item = dict(zip(selected, row))
            start_ms = chrome_time_to_ms(item.get("start_time"))
            end_ms = chrome_time_to_ms(item.get("end_time"))
            if start_ms is not None:
                item["startedAt"] = iso_from_ms(start_ms)
            if end_ms is not None:
                item["endedAt"] = iso_from_ms(end_ms)
            item["status"] = download_status(item.get("state"))
            item["source"] = "history-db"
            items.append(item)
    finally:
        conn.close()
    return {"items": items, "source": "history-db", "profileHistoryPath": path}


def download_status(state):
    if state == 0:
        return "in_progress"
    if state == 1:
        return "complete"
    if state == 2:
        return "canceled_or_interrupted"
    return "unknown"


def tool_download_start(params):
    url = require_url(params)
    opened = open_url_in_chrome(url)
    LIFECYCLE.emit("download:started", data={"url": url, "tab": opened})
    return {"started": True, "url": url, "tab": opened}


def tool_download_events(params):
    limit = require_int(params, "limit", 100, minimum=1, maximum=500)
    result = chrome_request(
        "__multiagent_poll_notifications",
        {"kind": "onDownloadChange", "limit": limit},
        timeout=3,
    )
    return result or {"notifications": []}


def tool_download_action(params):
    action = params.get("downloadAction") or params.get("download_action") or params.get("command")
    if action not in {"pause", "resume", "cancel"}:
        raise ValueError("downloadAction must be pause, resume, or cancel")
    guid = params.get("guid")
    if action == "cancel" and isinstance(guid, str) and guid:
        return CDP.execute(
            require_tab(params),
            "Browser.cancelDownload",
            {"guid": guid},
            retries=require_int(params, "retries", 1, minimum=0, maximum=5),
        )
    return {
        "ok": False,
        "unsupported": True,
        "action": action,
        "message": "The installed official extension does not expose pause/resume/cancel download requests to native messaging. Listing, triggering, and progress polling are available.",
    }


def tool_move_mouse(params):
    tab_id = require_tab(params)
    x = params.get("x")
    y = params.get("y")
    if not isinstance(x, (int, float)) or not isinstance(y, (int, float)):
        raise ValueError("x and y must be finite numbers")
    wait = require_bool(params, "waitForArrival", True)
    payload = {
        "tabId": tab_id,
        "x": float(x),
        "y": float(y),
        "waitForArrival": wait,
    }
    try:
        result = chrome_request("moveMouse", payload, timeout=DEFAULT_REQUEST_TIMEOUT)
    except TimeoutError:
        LIFECYCLE.emit("cursor:timeout", data=payload)
        return {
            **payload,
            "visible": None,
            "timedOut": True,
            "message": "The Chrome extension did not finish its cursor overlay request before the bridge timeout.",
        }
    LIFECYCLE.emit("cursor:moved", data={"tabId": tab_id, "x": x, "y": y})
    return result or {"tabId": tab_id, "x": x, "y": y, "visible": True}


def tool_cursor_overlay(params):
    enabled = require_bool(params, "enabled", True)
    if enabled:
        tab_id = require_tab(params)
        x = params.get("x", 16)
        y = params.get("y", 16)
        return tool_move_mouse({"tabId": tab_id, "x": x, "y": y, "waitForArrival": False})
    result = chrome_request("turnEnded", {}, timeout=5)
    LIFECYCLE.emit("cursor:hidden")
    return result or {"visible": False}


CURSOR_STREAM_SCRIPT = r"""
(() => {
  if (window.__multiagentChromeCursorStream) {
    return { enabled: true, alreadyEnabled: true, queued: window.__multiagentChromeEvents.length };
  }
  const queue = window.__multiagentChromeEvents = window.__multiagentChromeEvents || [];
  const push = (event) => {
    const item = {
      type: event.type,
      x: Number.isFinite(event.clientX) ? event.clientX : null,
      y: Number.isFinite(event.clientY) ? event.clientY : null,
      button: event.button ?? null,
      buttons: event.buttons ?? null,
      scrollX: window.scrollX,
      scrollY: window.scrollY,
      time: Date.now(),
    };
    queue.push(item);
    while (queue.length > 500) queue.shift();
  };
  const handlers = {
    mousemove: push,
    click: push,
    scroll: () => push({ type: "scroll", clientX: null, clientY: null, button: null, buttons: null }),
  };
  for (const [type, handler] of Object.entries(handlers)) {
    window.addEventListener(type, handler, { capture: true, passive: true });
  }
  window.__multiagentChromeCursorStream = {
    handlers,
    disable() {
      for (const [type, handler] of Object.entries(handlers)) {
        window.removeEventListener(type, handler, { capture: true });
      }
      delete window.__multiagentChromeCursorStream;
    },
  };
  return { enabled: true, queued: queue.length };
})()
"""


def tool_cursor_stream(params):
    tab_id = require_tab(params)
    enabled = require_bool(params, "enabled", True)
    expression = CURSOR_STREAM_SCRIPT if enabled else """
(() => {
  if (window.__multiagentChromeCursorStream) {
    window.__multiagentChromeCursorStream.disable();
  }
  return { enabled: false, queued: (window.__multiagentChromeEvents || []).length };
})()
"""
    return CDP.execute(
        tab_id,
        "Runtime.evaluate",
        {"expression": expression, "returnByValue": True, "awaitPromise": True},
        retries=require_int(params, "retries", 1, minimum=0, maximum=5),
    )


def tool_cursor_events(params):
    tab_id = require_tab(params)
    limit = require_int(params, "limit", 100, minimum=1, maximum=500)
    expression = f"""
(() => {{
  const queue = window.__multiagentChromeEvents || [];
  const items = queue.splice(0, {limit});
  return {{ items, remaining: queue.length }};
}})()
"""
    return CDP.execute(
        tab_id,
        "Runtime.evaluate",
        {"expression": expression, "returnByValue": True, "awaitPromise": True},
        retries=require_int(params, "retries", 1, minimum=0, maximum=5),
    )


def tool_eval(params):
    tab_id = require_tab(params)
    expression = params.get("expression")
    if not isinstance(expression, str) or not expression.strip():
        raise ValueError("expression is required")
    return CDP.execute(
        tab_id,
        "Runtime.evaluate",
        {"expression": expression, "returnByValue": True, "awaitPromise": True},
        retries=require_int(params, "retries", 2, minimum=0, maximum=5),
    )


def tool_cdp(params):
    tab_id = require_tab(params)
    method = params.get("method")
    if not isinstance(method, str) or not method.strip():
        raise ValueError("method is required")
    command_params = params.get("params") or {}
    if not isinstance(command_params, dict):
        raise ValueError("params must be an object")
    return CDP.execute(
        tab_id,
        method,
        command_params,
        timeout_ms=require_int(params, "timeoutMs", DEFAULT_CDP_TIMEOUT_MS, minimum=1),
        retries=require_int(params, "retries", 2, minimum=0, maximum=5),
    )


def tool_cdp_attach(params):
    return CDP.attach(
        require_tab(params),
        retries=require_int(params, "retries", 2, minimum=0, maximum=5),
    )


def tool_cdp_detach(params):
    return CDP.detach(require_tab(params))


def tool_cdp_status(params):
    tab_id = params.get("tabId")
    if tab_id is not None and not isinstance(tab_id, int):
        raise ValueError("tabId must be an integer")
    return CDP.status(tab_id)


def tool_lifecycle_status(_params):
    return LIFECYCLE.status()


def tool_lifecycle_events(params):
    return {
        "events": LIFECYCLE.since(
            require_int(params, "since", 0, minimum=0),
            require_int(params, "limit", 100, minimum=1, maximum=MAX_EVENTS),
        )
    }


def tool_feature_status(_params):
    return {
        "serverName": SERVER_NAME,
        "sessionId": SESSION_ID,
        "enabledFeatures": sorted(FEATURES),
        "capabilities": {
            "tabs": {
                "status": "supported" if feature_enabled("tabs") else "disabled",
                "actions": ["claim_tab", "group_tabs", "name_session", "finalize_tabs", "release_tabs"],
                "backend": "official-extension tab/session group APIs",
                "notes": [
                    "claim_tab rejects tabs already owned by a different extension session",
                    "group_tabs uses Chrome tabGroups through the extension-managed session group",
                ],
            },
            "history": {
                "status": "supported" if feature_enabled("history") else "disabled",
                "actions": ["history_search", "history_clear"],
                "backend": "Chrome History API with read-only SQLite fallback",
                "notes": [
                    "visitCount filtering uses the local Chrome History database",
                    "history_clear is destructive and requires MULTIAGENT_CHROME_ALLOW_HISTORY_MUTATION=1",
                ],
            },
            "downloads": {
                "status": "partial" if feature_enabled("downloads") else "disabled",
                "actions": ["downloads_list", "download_start", "download_events", "download_action"],
                "backend": "Chrome downloads notifications plus History downloads metadata",
                "notes": [
                    "download progress events are queued from onDownloadChange native notifications",
                    "CDP supports Browser.cancelDownload when a download GUID and working CDP session are available",
                    "Chrome CDP/native messaging does not expose a stable pause/resume command through the installed official extension",
                ],
            },
            "cursor": {
                "status": "partial" if feature_enabled("cursor") else "disabled",
                "actions": ["move_mouse", "cursor_overlay", "cursor_stream", "cursor_events"],
                "backend": "official extension cursor overlay plus optional CDP page event injection",
                "notes": [
                    "move_mouse returns a structured timeout instead of hanging if the extension content script does not answer",
                    "cursor_stream and cursor_events require the centralized CDP session to attach successfully",
                ],
            },
            "cdp": {
                "status": "supported" if feature_enabled("cdp") else "disabled",
                "actions": ["cdp_attach", "cdp_detach", "cdp_status", "cdp", "eval"],
                "backend": "centralized CdpSessionManager",
                "notes": [
                    "commands use timeout and retry logic",
                    "session state is reported as attached, detached, or crashed",
                ],
            },
            "lifecycle": {
                "status": "supported" if feature_enabled("lifecycle") else "disabled",
                "actions": ["lifecycle_status", "lifecycle_events", "restart", "shutdown", "feature_status"],
                "backend": "in-process lifecycle event ring buffer",
                "notes": [
                    "states are initializing, ready, active, idle, closing, and closed",
                    "consumers poll lifecycle_events over MCP",
                ],
            },
        },
    }


def tool_shutdown(params):
    keep = params.get("keep")
    LIFECYCLE.set_state("closing", "Graceful shutdown requested")
    detached = CDP.detach_all_best_effort()
    finalized = None
    if keep is not None:
        finalized = tool_finalize_tabs({"keep": keep})
    LIFECYCLE.set_state("closed", "Graceful shutdown complete")
    return {"state": "closed", "detached": detached, "finalized": finalized}


def tool_restart(_params):
    LIFECYCLE.set_state("initializing", "Bridge restart requested")
    CDP.sessions.clear()
    LIFECYCLE.set_state("ready", "Bridge ready after restart")
    return LIFECYCLE.status()


ACTION_HANDLERS = {
    "get_tabs": tool_get_tabs,
    "create_tab": tool_create_tab,
    "navigate": tool_navigate,
    "claim_tab": tool_claim_tab,
    "group_tabs": tool_group_tabs,
    "finalize_tabs": tool_finalize_tabs,
    "release_tabs": tool_finalize_tabs,
    "name_session": tool_name_session,
    "history_search": tool_history_search,
    "history_clear": tool_history_clear,
    "downloads_list": tool_downloads_list,
    "download_start": tool_download_start,
    "download_events": tool_download_events,
    "download_action": tool_download_action,
    "move_mouse": tool_move_mouse,
    "cursor_overlay": tool_cursor_overlay,
    "cursor_stream": tool_cursor_stream,
    "cursor_events": tool_cursor_events,
    "eval": tool_eval,
    "cdp": tool_cdp,
    "cdp_attach": tool_cdp_attach,
    "cdp_detach": tool_cdp_detach,
    "cdp_status": tool_cdp_status,
    "lifecycle_status": tool_lifecycle_status,
    "lifecycle_events": tool_lifecycle_events,
    "feature_status": tool_feature_status,
    "shutdown": tool_shutdown,
    "restart": tool_restart,
}


ACTION_FEATURES = {
    "get_tabs": "core",
    "create_tab": "core",
    "navigate": "core",
    "claim_tab": "tabs",
    "group_tabs": "tabs",
    "finalize_tabs": "tabs",
    "release_tabs": "tabs",
    "name_session": "tabs",
    "history_search": "history",
    "history_clear": "history",
    "downloads_list": "downloads",
    "download_start": "downloads",
    "download_events": "downloads",
    "download_action": "downloads",
    "move_mouse": "cursor",
    "cursor_overlay": "cursor",
    "cursor_stream": "cursor",
    "cursor_events": "cursor",
    "eval": "cdp",
    "cdp": "cdp",
    "cdp_attach": "cdp",
    "cdp_detach": "cdp",
    "cdp_status": "cdp",
    "lifecycle_status": "lifecycle",
    "lifecycle_events": "lifecycle",
    "feature_status": "lifecycle",
    "shutdown": "lifecycle",
    "restart": "lifecycle",
}


def enabled_actions():
    return [
        action
        for action in ACTION_HANDLERS
        if feature_enabled(ACTION_FEATURES.get(action, "core"))
    ]


def tool_multiagent_chrome(params):
    action = params.get("action")
    actions = enabled_actions()
    if action not in actions:
        raise ValueError(f"action must be one of: {', '.join(actions)}")
    return ACTION_HANDLERS[action](params)


def schema_for_action(action):
    base = {
        "type": "object",
        "properties": {
            "tabId": {"type": "integer"},
            "tabIds": {"type": "array", "items": {"type": "integer"}},
            "url": {"type": "string"},
            "title": {"type": "string"},
            "name": {"type": "string"},
            "expression": {"type": "string"},
            "method": {"type": "string"},
            "params": {"type": "object"},
            "query": {"type": "string"},
            "from": {"type": "string"},
            "to": {"type": "string"},
            "limit": {"type": "integer"},
            "x": {"type": "number"},
            "y": {"type": "number"},
            "enabled": {"type": "boolean"},
            "keep": {"type": "array", "items": {"type": "object"}},
            "retries": {"type": "integer"},
            "timeoutMs": {"type": "integer"},
        },
        "additionalProperties": True,
    }
    return base


def make_tool(description, handler, schema=None):
    return {
        "description": description,
        "inputSchema": schema or {"type": "object", "properties": {}, "additionalProperties": True},
        "handler": handler,
    }


def build_tools():
    actions = enabled_actions()
    tools = {
        SERVER_NAME: make_tool(
            "Control Google Chrome through the local Multiagent-chrome bridge. "
            "Use action to choose tabs/history/downloads/cursor/CDP/lifecycle operations.",
            tool_multiagent_chrome,
            {
                "type": "object",
                "properties": {
                    "action": {"type": "string", "enum": actions},
                    **schema_for_action("all")["properties"],
                },
                "required": ["action"],
                "additionalProperties": True,
            },
        ),
        "chrome_get_tabs": make_tool(
            "List open Chrome tabs visible to the Codex Chrome extension.",
            tool_get_tabs,
            {"type": "object", "properties": {}, "additionalProperties": False},
        ),
        "chrome_create_tab": make_tool(
            "Create a new Chrome tab. When url is provided, opens that URL in Chrome.",
            tool_create_tab,
            {
                "type": "object",
                "properties": {"url": {"type": "string"}, "claim": {"type": "boolean"}},
                "additionalProperties": False,
            },
        ),
        "chrome_navigate": make_tool(
            "Open a Chrome tab to a URL without requiring Chrome debugger attachment.",
            tool_navigate,
            {
                "type": "object",
                "properties": {"tabId": {"type": "integer"}, "url": {"type": "string"}},
                "required": ["url"],
                "additionalProperties": False,
            },
        ),
    }
    optional_tools = {
        "tabs": {
            "chrome_claim_tab": make_tool("Claim a tab into the current Chrome bridge session.", tool_claim_tab, schema_for_action("claim_tab")),
            "chrome_group_tabs": make_tool("Move tabs into the current extension-managed Chrome tab group.", tool_group_tabs, schema_for_action("group_tabs")),
            "chrome_finalize_tabs": make_tool("Finalize claimed tabs as handoff or deliverable tabs.", tool_finalize_tabs, schema_for_action("finalize_tabs")),
        },
        "history": {
            "chrome_history_search": make_tool("Search Chrome history by query, URL, title, date range, and visit count.", tool_history_search, schema_for_action("history_search")),
            "chrome_history_clear": make_tool("Clear Chrome history entries. Requires MULTIAGENT_CHROME_ALLOW_HISTORY_MUTATION=1.", tool_history_clear, schema_for_action("history_clear")),
        },
        "downloads": {
            "chrome_downloads_list": make_tool("List active and completed Chrome downloads from Chrome history metadata.", tool_downloads_list, schema_for_action("downloads_list")),
            "chrome_download_start": make_tool("Trigger a download or URL open in Chrome.", tool_download_start, schema_for_action("download_start")),
            "chrome_download_events": make_tool("Poll download progress notifications emitted by the Chrome extension.", tool_download_events, schema_for_action("download_events")),
            "chrome_download_action": make_tool("Attempt a supported download action such as cancel by CDP download GUID.", tool_download_action, schema_for_action("download_action")),
        },
        "cursor": {
            "chrome_move_mouse": make_tool("Move the visible Codex cursor overlay in a claimed tab.", tool_move_mouse, schema_for_action("move_mouse")),
            "chrome_cursor_overlay": make_tool("Enable or disable the cursor overlay for the bridge session.", tool_cursor_overlay, schema_for_action("cursor_overlay")),
            "chrome_cursor_stream": make_tool("Enable or disable injected page mouse/click/scroll event capture.", tool_cursor_stream, schema_for_action("cursor_stream")),
            "chrome_cursor_events": make_tool("Read queued mouse/click/scroll events from an injected page stream.", tool_cursor_events, schema_for_action("cursor_events")),
        },
        "cdp": {
            "chrome_eval": make_tool("Evaluate JavaScript in a controlled Chrome tab using CDP.", tool_eval, schema_for_action("eval")),
            "chrome_cdp": make_tool("Run a Chrome DevTools Protocol command against a controlled Chrome tab.", tool_cdp, schema_for_action("cdp")),
            "chrome_cdp_attach": make_tool("Attach the centralized CDP session manager to a tab.", tool_cdp_attach, schema_for_action("cdp_attach")),
            "chrome_cdp_detach": make_tool("Detach the centralized CDP session manager from a tab.", tool_cdp_detach, schema_for_action("cdp_detach")),
            "chrome_cdp_status": make_tool("Read centralized CDP session state.", tool_cdp_status, schema_for_action("cdp_status")),
        },
        "lifecycle": {
            "chrome_lifecycle_status": make_tool("Read bridge lifecycle state.", tool_lifecycle_status, schema_for_action("lifecycle_status")),
            "chrome_lifecycle_events": make_tool("Poll bridge lifecycle events.", tool_lifecycle_events, schema_for_action("lifecycle_events")),
            "chrome_feature_status": make_tool("Read a structured capability matrix for the Chrome bridge.", tool_feature_status, schema_for_action("feature_status")),
        },
    }
    for feature, feature_tools in optional_tools.items():
        if feature_enabled(feature):
            tools.update(feature_tools)
    return tools


TOOLS = build_tools()


def handle(method, params):
    if method == "initialize":
        return {
            "protocolVersion": "2024-11-05",
            "capabilities": {"tools": {}},
            "serverInfo": {"name": SERVER_NAME, "version": "0.2.0"},
        }
    if method == "resources/list":
        return {"resources": []}
    if method == "resources/templates/list":
        return {"resourceTemplates": []}
    if method == "prompts/list":
        return {"prompts": []}
    if method == "tools/list":
        return {
            "tools": [
                {
                    "name": name,
                    "title": name,
                    "description": spec["description"],
                    "inputSchema": spec["inputSchema"],
                }
                for name, spec in TOOLS.items()
            ]
        }
    if method == "tools/call":
        name = params.get("name")
        arguments = params.get("arguments") or {}
        if name not in TOOLS:
            return error_result(f"Unknown Chrome tool: {name}")
        try:
            LIFECYCLE.set_state("active", f"Running {name}", {"tool": name})
            value = TOOLS[name]["handler"](arguments)
            if LIFECYCLE.state == "active":
                LIFECYCLE.set_state("idle", f"Finished {name}", {"tool": name})
            return text_result(value)
        except Exception as exc:
            LIFECYCLE.emit("tool:error", str(exc), {"tool": name})
            if LIFECYCLE.state == "active":
                LIFECYCLE.set_state("idle", f"Failed {name}", {"tool": name})
            return error_result(exc)
    return None


def main():
    LIFECYCLE.set_state("ready", "MCP server ready")
    for line in sys.stdin:
        if not line.strip():
            continue
        request = json.loads(line)
        if "id" not in request:
            continue
        result = handle(request.get("method"), request.get("params") or {})
        if result is None:
            response = {
                "jsonrpc": "2.0",
                "id": request["id"],
                "error": {"code": -32601, "message": f"Method not found: {request.get('method')}"},
            }
        else:
            response = {"jsonrpc": "2.0", "id": request["id"], "result": result}
        print(json.dumps(response, separators=(",", ":")), flush=True)


if __name__ == "__main__":
    main()
