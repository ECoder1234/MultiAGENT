#!/usr/bin/env python3
import json
import os
import shutil
import socket
import subprocess
import sys
import time


SESSION_ID = "multiagent-codex-chrome"
SERVER_NAME = "Multiagent-chrome"


def socket_path():
    return os.environ.get(
        "MULTIAGENT_CHROME_BRIDGE_SOCKET",
        os.path.join(
            os.environ.get("XDG_RUNTIME_DIR") or "/tmp",
            f"multiagent-codex-chrome-host-{os.geteuid()}.sock",
        ),
    )


def chrome_request(method, params=None, timeout=35):
    params = dict(params or {})
    params.setdefault("session_id", SESSION_ID)
    params.setdefault("turn_id", f"mcp-{int(time.time() * 1000)}")
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


def claim_and_attach(tab_id, turn_id):
    chrome_request("claimUserTab", {"tabId": tab_id, "turn_id": turn_id})
    try:
        chrome_request("attach", {"tabId": tab_id, "turn_id": turn_id})
    except RuntimeError as exc:
        if "Another debugger" not in str(exc):
            raise


def execute_cdp(tab_id, method, command_params=None, turn_id=None, timeout_ms=10000):
    return chrome_request(
        "executeCdp",
        {
            "target": {"tabId": tab_id},
            "method": method,
            "commandParams": command_params or {},
            "timeoutMs": timeout_ms,
            "turn_id": turn_id or f"mcp-{int(time.time() * 1000)}",
        },
    )


def tool_get_tabs(_params):
    return chrome_request("getUserTabs", {})


def tool_create_tab(params):
    url = params.get("url")
    if isinstance(url, str) and url.strip():
        return open_url_in_chrome(url.strip())
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


def tool_eval(params):
    turn_id = f"mcp-{int(time.time() * 1000)}"
    tab_id = require_tab(params)
    expression = params.get("expression")
    if not isinstance(expression, str) or not expression.strip():
        raise ValueError("expression is required")
    claim_and_attach(tab_id, turn_id)
    return execute_cdp(
        tab_id,
        "Runtime.evaluate",
        {"expression": expression, "returnByValue": True, "awaitPromise": True},
        turn_id,
    )


def tool_cdp(params):
    turn_id = f"mcp-{int(time.time() * 1000)}"
    tab_id = require_tab(params)
    method = params.get("method")
    if not isinstance(method, str) or not method.strip():
        raise ValueError("method is required")
    command_params = params.get("params") or {}
    if not isinstance(command_params, dict):
        raise ValueError("params must be an object")
    claim_and_attach(tab_id, turn_id)
    return execute_cdp(tab_id, method, command_params, turn_id)


def tool_multiagent_chrome(params):
    action = params.get("action")
    if action == "get_tabs":
        return tool_get_tabs(params)
    if action == "create_tab":
        return tool_create_tab(params)
    if action == "navigate":
        return tool_navigate(params)
    if action == "eval":
        return tool_eval(params)
    if action == "cdp":
        return tool_cdp(params)
    raise ValueError("action must be one of: get_tabs, create_tab, navigate, eval, cdp")


TOOLS = {
    SERVER_NAME: {
        "description": "Control Google Chrome through the local Multiagent-chrome bridge. Use action=get_tabs, create_tab, navigate, eval, or cdp.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["get_tabs", "create_tab", "navigate", "eval", "cdp"],
                },
                "tabId": {"type": "integer"},
                "url": {"type": "string"},
                "expression": {"type": "string"},
                "method": {"type": "string"},
                "params": {"type": "object"},
            },
            "required": ["action"],
            "additionalProperties": False,
        },
        "handler": tool_multiagent_chrome,
    },
    "chrome_get_tabs": {
        "description": "List open Chrome tabs visible to the Codex Chrome extension.",
        "inputSchema": {"type": "object", "properties": {}, "additionalProperties": False},
        "handler": tool_get_tabs,
    },
    "chrome_create_tab": {
        "description": "Create a new Chrome tab. When url is provided, opens that URL in Chrome.",
        "inputSchema": {
            "type": "object",
            "properties": {"url": {"type": "string"}},
            "additionalProperties": False,
        },
        "handler": tool_create_tab,
    },
    "chrome_navigate": {
        "description": "Open a Chrome tab to a URL without requiring Chrome debugger attachment.",
        "inputSchema": {
            "type": "object",
            "properties": {"tabId": {"type": "integer"}, "url": {"type": "string"}},
            "required": ["url"],
            "additionalProperties": False,
        },
        "handler": tool_navigate,
    },
    "chrome_eval": {
        "description": "Evaluate JavaScript in a controlled Chrome tab using the Chrome DevTools Protocol.",
        "inputSchema": {
            "type": "object",
            "properties": {"tabId": {"type": "integer"}, "expression": {"type": "string"}},
            "required": ["tabId", "expression"],
            "additionalProperties": False,
        },
        "handler": tool_eval,
    },
    "chrome_cdp": {
        "description": "Run a Chrome DevTools Protocol command against a controlled Chrome tab.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "tabId": {"type": "integer"},
                "method": {"type": "string"},
                "params": {"type": "object"},
            },
            "required": ["tabId", "method"],
            "additionalProperties": False,
        },
        "handler": tool_cdp,
    },
}


def handle(method, params):
    if method == "initialize":
        return {
            "protocolVersion": "2024-11-05",
            "capabilities": {"tools": {}},
            "serverInfo": {"name": SERVER_NAME, "version": "0.1.0"},
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
            return text_result(TOOLS[name]["handler"](arguments))
        except Exception as exc:
            return error_result(exc)
    return None


def main():
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
