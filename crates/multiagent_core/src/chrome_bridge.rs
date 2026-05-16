use std::env;
use std::fs;
use std::io::{self, Read, Write};
#[cfg(unix)]
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::sync::atomic::{AtomicI64, Ordering};
#[cfg(unix)]
use std::sync::{Arc, Mutex, mpsc};
#[cfg(unix)]
use std::time::{Duration, SystemTime, UNIX_EPOCH};
#[cfg(unix)]
use std::{
    collections::{HashMap, VecDeque},
    thread,
};

use serde_json::{Value, json};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
#[cfg(unix)]
use std::os::unix::net::{UnixListener, UnixStream};

pub const CHROME_EXTENSION_ID: &str = "hehggadaopoacecdllhhajmbjkdcmajg";
pub const NATIVE_HOST_NAME: &str = "com.openai.codexextension";
pub const CHROME_PLUGIN_NAME: &str = "chrome@openai-bundled";
pub const BROWSER_USE_PLUGIN_NAME: &str = "browser-use@openai-bundled";
pub const APP_NATIVE_HOST_BINARY_NAME: &str = "multiagent-codex-chrome-host";
pub const CHROME_MCP_SERVER_NAME: &str = "Multiagent-chrome";
pub const BUNDLED_PLUGIN_VERSION: &str = "0.1.0";

const MAX_NATIVE_MESSAGE_BYTES: usize = 64 * 1024 * 1024;
const CHROME_MCP_SERVER_SOURCE: &str = include_str!("chrome_mcp_server.py");
const BUNDLED_MARKETPLACE_NAME: &str = "openai-bundled";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChromeBridgeStatus {
    pub extension_installed: bool,
    pub extension_paths: Vec<PathBuf>,
    pub native_manifest_paths: Vec<PathBuf>,
    pub native_manifest_installed: bool,
    pub native_manifest_host_paths: Vec<PathBuf>,
    pub native_manifest_live: bool,
    pub codex_config_path: PathBuf,
    pub codex_plugins_enabled: bool,
    pub codex_plugins_installed: bool,
    pub codex_plugin_paths: Vec<PathBuf>,
    pub host_candidate: Option<PathBuf>,
    pub notes: Vec<String>,
}

pub fn status() -> ChromeBridgeStatus {
    let extension_paths = find_extension_paths();
    let native_manifest_paths = native_manifest_paths();
    let native_manifest_installed = native_manifest_paths.iter().any(|path| path.is_file());
    let native_manifest_host_paths = manifest_host_paths(&native_manifest_paths);
    let native_manifest_live = native_manifest_host_paths.iter().any(|path| path.is_file());
    let codex_config_path = codex_config_path();
    let codex_plugins_enabled = codex_config_has_plugin(&codex_config_path, CHROME_PLUGIN_NAME)
        && codex_config_has_plugin(&codex_config_path, BROWSER_USE_PLUGIN_NAME);
    let codex_plugin_paths = openai_bundled_plugin_paths();
    let codex_plugins_installed = codex_plugin_paths.iter().all(|path| {
        path.join(".codex-plugin").join("plugin.json").is_file()
            && (path.file_name().and_then(|name| name.to_str()) != Some("chrome")
                || path.join(".mcp.json").is_file())
    });
    let host_candidate = find_native_host_candidate();
    let mut notes = Vec::new();

    if extension_paths.is_empty() {
        notes.push(
            "Official Codex Chrome extension was not found in Chrome or Chromium profiles."
                .to_string(),
        );
    }
    if !native_manifest_installed {
        notes.push("Chrome native messaging host manifest is missing.".to_string());
    } else if !native_manifest_live {
        notes.push("Chrome native messaging manifest points to a missing host.".to_string());
    }
    if !codex_plugins_enabled {
        notes.push("Codex Chrome plugin config is not enabled yet.".to_string());
    }
    if !codex_plugins_installed {
        notes.push(
            "Codex Chrome plugin files are missing from the openai-bundled cache or local marketplace.".to_string(),
        );
    }
    if host_candidate.is_none() {
        notes.push(
            "App native-host wrapper has not been installed yet. Use Enable Linux bridge."
                .to_string(),
        );
    }

    ChromeBridgeStatus {
        extension_installed: !extension_paths.is_empty(),
        extension_paths,
        native_manifest_paths,
        native_manifest_installed,
        native_manifest_host_paths,
        native_manifest_live,
        codex_config_path,
        codex_plugins_enabled,
        codex_plugins_installed,
        codex_plugin_paths,
        host_candidate,
        notes,
    }
}

pub fn enable_codex_plugin_config() -> Result<(), String> {
    let path = codex_config_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            format!(
                "Could not create Codex config directory {}: {err}",
                parent.display()
            )
        })?;
    }

    let mut config = fs::read_to_string(&path).unwrap_or_default();
    ensure_plugin_stanza(&mut config, BROWSER_USE_PLUGIN_NAME);
    ensure_plugin_stanza(&mut config, CHROME_PLUGIN_NAME);
    fs::write(&path, config)
        .map_err(|err| format!("Could not write Codex config {}: {err}", path.display()))
}

pub fn install_native_manifest_if_possible() -> Result<Option<Vec<PathBuf>>, String> {
    let host_path = ensure_app_native_host_wrapper().or_else(|wrapper_err| {
        find_native_host_candidate().ok_or_else(|| {
            format!("{wrapper_err} No existing Codex native host was found to use as a fallback.")
        })
    })?;
    let mut written = Vec::new();
    for manifest_path in native_manifest_paths() {
        if let Some(parent) = manifest_path.parent() {
            fs::create_dir_all(parent).map_err(|err| {
                format!(
                    "Could not create native messaging directory {}: {err}",
                    parent.display()
                )
            })?;
        }
        let payload = native_manifest_json(&host_path);
        fs::write(&manifest_path, payload).map_err(|err| {
            format!(
                "Could not write native messaging manifest {}: {err}",
                manifest_path.display()
            )
        })?;
        written.push(manifest_path);
    }
    Ok(Some(written))
}

pub fn run_native_host_stdio() -> Result<(), String> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    run_native_host(stdin.lock(), stdout)
}

pub fn enable_linux_bridge() -> Result<ChromeBridgeStatus, String> {
    enable_codex_plugin_config()?;
    install_openai_bundled_plugins()?;
    let _ = install_native_manifest_if_possible()?;
    Ok(status())
}

fn ensure_plugin_stanza(config: &mut String, plugin_name: &str) {
    let header = format!("[plugins.\"{plugin_name}\"]");
    if config.contains(&header) {
        return;
    }
    if !config.ends_with('\n') && !config.is_empty() {
        config.push('\n');
    }
    config.push('\n');
    config.push_str(&header);
    config.push_str("\nenabled = true\n");
}

fn codex_config_has_plugin(path: &Path, plugin_name: &str) -> bool {
    let Ok(config) = fs::read_to_string(path) else {
        return false;
    };
    let header = format!("[plugins.\"{plugin_name}\"]");
    let Some(start) = config.find(&header) else {
        return false;
    };
    let section = &config[start + header.len()..];
    let section = section.split("\n[").next().unwrap_or(section);
    section.lines().any(|line| {
        let trimmed = line.trim();
        trimmed == "enabled = true" || trimmed == "enabled=true"
    })
}

fn native_manifest_json(host_path: &Path) -> String {
    let escaped_path = host_path
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    format!(
        "{{\n  \"name\": \"{NATIVE_HOST_NAME}\",\n  \"description\": \"Codex Chrome native host\",\n  \"path\": \"{escaped_path}\",\n  \"type\": \"stdio\",\n  \"allowed_origins\": [\n    \"chrome-extension://{CHROME_EXTENSION_ID}/\"\n  ]\n}}\n"
    )
}

fn install_openai_bundled_plugins() -> Result<(), String> {
    let chrome_plugin_path = openai_bundled_plugin_path("chrome")
        .ok_or_else(|| "Could not resolve Codex plugin cache directory.".to_string())?;
    let browser_use_plugin_path = openai_bundled_plugin_path("browser-use")
        .ok_or_else(|| "Could not resolve Codex plugin cache directory.".to_string())?;
    let chrome_home_plugin_path = home_local_plugin_path("chrome")
        .ok_or_else(|| "Could not resolve home plugin directory.".to_string())?;
    let browser_use_home_plugin_path = home_local_plugin_path("browser-use")
        .ok_or_else(|| "Could not resolve home plugin directory.".to_string())?;
    let marketplace_path = bundled_marketplace_path()
        .ok_or_else(|| "Could not resolve home plugin marketplace path.".to_string())?;
    let mcp_server_path = chrome_mcp_server_path()
        .ok_or_else(|| "Could not resolve Chrome MCP server install path.".to_string())?;

    write_file(
        &mcp_server_path,
        CHROME_MCP_SERVER_SOURCE,
        Some(0o755),
        "Chrome MCP server",
    )?;
    install_chrome_plugin_at(&chrome_plugin_path, &mcp_server_path)?;
    install_chrome_plugin_at(&chrome_home_plugin_path, &mcp_server_path)?;
    install_browser_use_plugin_at(&browser_use_plugin_path)?;
    install_browser_use_plugin_at(&browser_use_home_plugin_path)?;
    write_file(
        &marketplace_path,
        &bundled_marketplace_json(),
        None,
        "openai-bundled marketplace",
    )?;
    Ok(())
}

fn install_chrome_plugin_at(plugin_path: &Path, mcp_server_path: &Path) -> Result<(), String> {
    write_file(
        &plugin_path.join(".codex-plugin").join("plugin.json"),
        &chrome_plugin_manifest_json(),
        None,
        "Chrome plugin manifest",
    )?;
    write_file(
        &plugin_path.join(".mcp.json"),
        &chrome_mcp_config_json(mcp_server_path),
        None,
        "Chrome MCP config",
    )
}

fn install_browser_use_plugin_at(plugin_path: &Path) -> Result<(), String> {
    write_file(
        &plugin_path.join(".codex-plugin").join("plugin.json"),
        &browser_use_plugin_manifest_json(),
        None,
        "browser-use plugin manifest",
    )
}

fn write_file(path: &Path, contents: &str, mode: Option<u32>, label: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            format!(
                "Could not create {label} directory {}: {err}",
                parent.display()
            )
        })?;
    }
    fs::write(path, contents)
        .map_err(|err| format!("Could not write {label} {}: {err}", path.display()))?;
    #[cfg(unix)]
    if let Some(mode) = mode {
        let mut permissions = fs::metadata(path)
            .map_err(|err| format!("Could not read {label} metadata {}: {err}", path.display()))?
            .permissions();
        permissions.set_mode(mode);
        fs::set_permissions(path, permissions).map_err(|err| {
            format!(
                "Could not set {label} permissions {}: {err}",
                path.display()
            )
        })?;
    }
    Ok(())
}

fn chrome_plugin_manifest_json() -> String {
    r##"{
  "name": "chrome",
  "version": "0.1.0",
  "description": "Control Google Chrome through the local Multiagent-chrome bridge.",
  "author": {
    "name": "MultiAGENT"
  },
  "homepage": "https://developers.openai.com/codex/app/chrome-extension",
  "license": "MIT",
  "keywords": ["chrome", "browser", "cdp", "computer-use"],
  "mcpServers": "./.mcp.json",
  "interface": {
    "displayName": "Multiagent-chrome",
    "shortDescription": "Control Chrome through the Multiagent-chrome bridge",
    "longDescription": "Provides Chrome browser tools backed by the locally installed Codex Chrome extension and native messaging host.",
    "developerName": "MultiAGENT",
    "category": "Productivity",
    "capabilities": ["Interactive", "Read", "Write"],
    "websiteURL": "https://developers.openai.com/codex/app/chrome-extension",
    "defaultPrompt": ["Use Multiagent-chrome"],
    "brandColor": "#4285F4",
    "screenshots": []
  }
}
"##
    .to_string()
}

fn browser_use_plugin_manifest_json() -> String {
    r##"{
  "name": "browser-use",
  "version": "0.1.0",
  "description": "Compatibility plugin entry for Codex browser-use configuration on Linux.",
  "author": {
    "name": "MultiAGENT"
  },
  "license": "MIT",
  "keywords": ["browser", "chrome", "computer-use"],
  "interface": {
    "displayName": "Browser Use",
    "shortDescription": "Compatibility shim for Chrome browser-use setup",
    "longDescription": "Keeps Codex from treating browser-use@openai-bundled as missing when using the Linux Chrome bridge.",
    "developerName": "MultiAGENT",
    "category": "Productivity",
    "capabilities": ["Interactive"],
    "defaultPrompt": ["Use the browser"],
    "brandColor": "#4285F4",
    "screenshots": []
  }
}
"##
    .to_string()
}

fn chrome_mcp_config_json(server_path: &Path) -> String {
    json!({
        "mcpServers": {
            CHROME_MCP_SERVER_NAME: {
                "command": "python3",
                "args": [server_path.to_string_lossy().to_string()],
                "env": {
                    "MULTIAGENT_CHROME_BRIDGE_SOCKET": broker_socket_path().to_string_lossy().to_string(),
                    "MULTIAGENT_CHROME_FEATURES": "core,tabs,history,downloads,cursor,cdp,lifecycle"
                }
            }
        }
    })
    .to_string()
}

fn bundled_marketplace_json() -> String {
    json!({
        "name": BUNDLED_MARKETPLACE_NAME,
        "interface": {
            "displayName": "OpenAI Bundled"
        },
        "plugins": [
            {
                "name": "chrome",
                "source": {
                    "source": "local",
                    "path": "./plugins/chrome"
                },
                "policy": {
                    "installation": "INSTALLED_BY_DEFAULT",
                    "authentication": "ON_USE"
                },
                "category": "Productivity"
            },
            {
                "name": "browser-use",
                "source": {
                    "source": "local",
                    "path": "./plugins/browser-use"
                },
                "policy": {
                    "installation": "INSTALLED_BY_DEFAULT",
                    "authentication": "ON_USE"
                },
                "category": "Productivity"
            }
        ]
    })
    .to_string()
}

fn openai_bundled_plugin_paths() -> Vec<PathBuf> {
    ["browser-use", "chrome"]
        .into_iter()
        .flat_map(|name| {
            [
                openai_bundled_plugin_path(name),
                home_local_plugin_path(name),
            ]
        })
        .flatten()
        .collect()
}

fn openai_bundled_plugin_path(name: &str) -> Option<PathBuf> {
    codex_home_dir().map(|home| {
        home.join("plugins")
            .join("cache")
            .join("openai-bundled")
            .join(name)
            .join(BUNDLED_PLUGIN_VERSION)
    })
}

fn home_local_plugin_path(name: &str) -> Option<PathBuf> {
    home_dir().map(|home| home.join("plugins").join(name))
}

fn bundled_marketplace_path() -> Option<PathBuf> {
    home_dir().map(|home| {
        home.join(".agents")
            .join("plugins")
            .join("marketplace.json")
    })
}

fn chrome_mcp_server_path() -> Option<PathBuf> {
    xdg_data_home().map(|path| {
        path.join("multiagent")
            .join("chrome-plugin")
            .join("chrome_mcp_server.py")
    })
}

fn manifest_host_paths(manifest_paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut paths = manifest_paths
        .iter()
        .filter_map(|path| read_manifest_host_path(path))
        .collect::<Vec<_>>();
    paths.sort();
    paths.dedup();
    paths
}

fn read_manifest_host_path(path: &Path) -> Option<PathBuf> {
    let value: Value = serde_json::from_str(&fs::read_to_string(path).ok()?).ok()?;
    value.get("path").and_then(Value::as_str).map(PathBuf::from)
}

fn codex_config_path() -> PathBuf {
    codex_home_dir()
        .unwrap_or_else(|| PathBuf::from(".codex"))
        .join("config.toml")
}

fn codex_home_dir() -> Option<PathBuf> {
    env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| home_dir().map(|home| home.join(".codex")))
}

fn native_manifest_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(home) = home_dir() {
        paths.push(
            home.join(".config")
                .join("google-chrome")
                .join("NativeMessagingHosts")
                .join(format!("{NATIVE_HOST_NAME}.json")),
        );
        paths.push(
            home.join(".config")
                .join("chromium")
                .join("NativeMessagingHosts")
                .join(format!("{NATIVE_HOST_NAME}.json")),
        );
    }
    paths
}

fn find_extension_paths() -> Vec<PathBuf> {
    let mut found = Vec::new();
    let Some(home) = home_dir() else {
        return found;
    };
    for browser_root in [
        home.join(".config").join("google-chrome"),
        home.join(".config").join("chromium"),
    ] {
        collect_extension_paths(&browser_root, 0, &mut found);
    }
    found.sort();
    found.dedup();
    found
}

fn collect_extension_paths(path: &Path, depth: usize, found: &mut Vec<PathBuf>) {
    if depth > 5 || !path.is_dir() {
        return;
    }
    let candidate = path.join("Extensions").join(CHROME_EXTENSION_ID);
    if candidate.is_dir() {
        found.push(candidate);
    }
    let Ok(entries) = fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten().take(256) {
        let child = entry.path();
        if child.is_dir() {
            collect_extension_paths(&child, depth + 1, found);
        }
    }
}

fn find_native_host_candidate() -> Option<PathBuf> {
    let home = home_dir()?;
    if let Some(path) = existing_app_native_host_wrapper() {
        return Some(path);
    }
    if let Some(path) = native_manifest_paths()
        .iter()
        .filter_map(|path| read_manifest_host_path(path))
        .find(|path| path.is_file())
    {
        return Some(path);
    }
    let roots = [
        home.join(".codex").join("plugins").join("cache"),
        home.join(".vscode").join("extensions"),
        home.join(".cursor").join("extensions"),
    ];
    for root in roots {
        if let Some(path) = find_host_under(&root, 0) {
            return Some(path);
        }
    }
    None
}

fn existing_app_native_host_wrapper() -> Option<PathBuf> {
    let path = app_native_host_wrapper_path()?;
    path.is_file().then_some(path)
}

fn ensure_app_native_host_wrapper() -> Result<PathBuf, String> {
    let launcher_path = current_app_launcher_path().ok_or_else(|| {
        "Could not resolve the app executable for the Chrome native host.".to_string()
    })?;
    let wrapper_path = app_native_host_wrapper_path().ok_or_else(|| {
        "Could not resolve a user data directory for the Chrome native host.".to_string()
    })?;
    if let Some(parent) = wrapper_path.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            format!(
                "Could not create native host wrapper directory {}: {err}",
                parent.display()
            )
        })?;
    }
    let script = native_host_wrapper_script(&launcher_path);
    fs::write(&wrapper_path, script).map_err(|err| {
        format!(
            "Could not write native host wrapper {}: {err}",
            wrapper_path.display()
        )
    })?;
    #[cfg(unix)]
    {
        let mut permissions = fs::metadata(&wrapper_path)
            .map_err(|err| {
                format!(
                    "Could not read native host wrapper metadata {}: {err}",
                    wrapper_path.display()
                )
            })?
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&wrapper_path, permissions).map_err(|err| {
            format!(
                "Could not make native host wrapper executable {}: {err}",
                wrapper_path.display()
            )
        })?;
    }
    Ok(wrapper_path)
}

fn native_host_wrapper_script(launcher_path: &Path) -> String {
    let launcher = shell_quote_path(launcher_path);
    format!(
        "#!/bin/sh\n\
control_socket=\"${{CODEX_APP_SERVER_CONTROL_SOCKET:-${{HOME:-}}/.codex/app-server-control/app-server-control.sock}}\"\n\
if [ -n \"$control_socket\" ] && [ -S \"$control_socket\" ] && command -v codex >/dev/null 2>&1; then\n\
  exec codex app-server proxy --sock \"$control_socket\"\n\
fi\n\
exec {launcher} --chrome-native-host \"$@\"\n"
    )
}

fn shell_quote_path(path: &Path) -> String {
    let raw = path.to_string_lossy();
    format!("'{}'", raw.replace('\'', "'\"'\"'"))
}

fn current_app_launcher_path() -> Option<PathBuf> {
    env::var_os("APPIMAGE")
        .map(PathBuf::from)
        .filter(|path| path.is_file())
        .or_else(|| env::current_exe().ok().filter(|path| path.is_file()))
}

fn app_native_host_wrapper_path() -> Option<PathBuf> {
    xdg_data_home().map(|path| {
        path.join("multiagent")
            .join("chrome-native-host")
            .join(APP_NATIVE_HOST_BINARY_NAME)
    })
}

fn xdg_data_home() -> Option<PathBuf> {
    env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| home_dir().map(|home| home.join(".local").join("share")))
}

fn find_host_under(path: &Path, depth: usize) -> Option<PathBuf> {
    if depth > 8 || !path.is_dir() {
        return None;
    }
    let entries = fs::read_dir(path).ok()?;
    for entry in entries.flatten().take(512) {
        let child = entry.path();
        if child.is_dir() {
            if let Some(found) = find_host_under(&child, depth + 1) {
                return Some(found);
            }
            continue;
        }
        let Some(name) = child.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let lowered = name.to_ascii_lowercase();
        if lowered.contains("codexextension") || lowered.contains("chrome-native-host") {
            return Some(child);
        }
    }
    None
}

fn home_dir() -> Option<PathBuf> {
    env::var_os("HOME").map(PathBuf::from)
}

fn broker_socket_path() -> PathBuf {
    let file_name = format!("multiagent-codex-chrome-host-{}.sock", current_uid());
    env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| env::temp_dir())
        .join(file_name)
}

fn current_uid() -> u32 {
    #[cfg(unix)]
    {
        unsafe { libc::geteuid() }
    }
    #[cfg(not(unix))]
    {
        0
    }
}

type PendingBrokerRequests = Arc<Mutex<HashMap<i64, mpsc::Sender<Value>>>>;
type NativeNotifications = Arc<Mutex<VecDeque<Value>>>;
const MAX_NATIVE_NOTIFICATIONS: usize = 512;

fn run_native_host<R: Read, W: Write + Send + 'static>(
    mut reader: R,
    writer: W,
) -> Result<(), String> {
    let writer = Arc::new(Mutex::new(writer));
    let pending: PendingBrokerRequests = Arc::new(Mutex::new(HashMap::new()));
    let notifications: NativeNotifications = Arc::new(Mutex::new(VecDeque::new()));
    start_native_host_broker(writer.clone(), pending.clone(), notifications.clone());

    loop {
        let mut length_header = [0_u8; 4];
        match reader.read_exact(&mut length_header) {
            Ok(()) => {}
            Err(err) if err.kind() == io::ErrorKind::UnexpectedEof => return Ok(()),
            Err(err) => return Err(format!("Could not read native message header: {err}")),
        }
        let length = u32::from_le_bytes(length_header) as usize;
        if length > MAX_NATIVE_MESSAGE_BYTES {
            return Err(format!(
                "Native message is too large: {length} bytes exceeds {MAX_NATIVE_MESSAGE_BYTES}"
            ));
        }
        let mut payload = vec![0_u8; length];
        reader
            .read_exact(&mut payload)
            .map_err(|err| format!("Could not read native message payload: {err}"))?;
        let message: Value = serde_json::from_slice(&payload)
            .map_err(|err| format!("Could not parse native message JSON: {err}"))?;
        if let Some(response) = handle_native_host_message(message, Some(&notifications)) {
            if response.get("method").is_none() && response.get("result").is_some()
                || response.get("error").is_some()
            {
                if let Some(id) = response.get("id").and_then(Value::as_i64) {
                    if let Some(tx) = pending
                        .lock()
                        .ok()
                        .and_then(|mut pending| pending.remove(&id))
                    {
                        let _ = tx.send(response);
                        continue;
                    }
                }
            }
            let mut writer = writer
                .lock()
                .map_err(|_| "Native writer lock was poisoned.".to_string())?;
            write_native_message(&mut *writer, &response)?;
        }
    }
}

fn handle_native_host_message(
    message: Value,
    notifications: Option<&NativeNotifications>,
) -> Option<Value> {
    if message.get("method").is_none()
        && (message.get("result").is_some() || message.get("error").is_some())
    {
        return Some(message);
    }

    let method = message.get("method").and_then(Value::as_str)?;
    let id = message.get("id").cloned();
    let Some(id) = id else {
        return match method {
            "onCDPEvent" | "onDownloadChange" => {
                record_native_notification(notifications, method, message.get("params").cloned());
                None
            }
            _ => None,
        };
    };

    match method {
        "ping" => Some(json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": "pong",
        })),
        _ => Some(json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {
                "code": -32601,
                "message": format!("No handler registered for method: {method}"),
            },
        })),
    }
}

fn record_native_notification(
    notifications: Option<&NativeNotifications>,
    method: &str,
    params: Option<Value>,
) {
    let Some(notifications) = notifications else {
        return;
    };
    let received_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default();
    if let Ok(mut notifications) = notifications.lock() {
        notifications.push_back(json!({
            "method": method,
            "params": params.unwrap_or_else(|| json!({})),
            "receivedAt": received_at,
        }));
        while notifications.len() > MAX_NATIVE_NOTIFICATIONS {
            notifications.pop_front();
        }
    }
}

#[cfg(unix)]
fn start_native_host_broker<W: Write + Send + 'static>(
    writer: Arc<Mutex<W>>,
    pending: PendingBrokerRequests,
    notifications: NativeNotifications,
) {
    let socket_path = broker_socket_path();
    let _ = fs::remove_file(&socket_path);
    let Ok(listener) = UnixListener::bind(&socket_path) else {
        return;
    };
    let _ = fs::set_permissions(&socket_path, fs::Permissions::from_mode(0o600));
    thread::spawn(move || {
        let next_id = Arc::new(AtomicI64::new(10_000));
        for stream in listener.incoming().flatten() {
            let writer = writer.clone();
            let pending = pending.clone();
            let notifications = notifications.clone();
            let next_id = next_id.clone();
            thread::spawn(move || {
                let _ = handle_broker_client(stream, writer, pending, notifications, next_id);
            });
        }
    });
}

#[cfg(not(unix))]
fn start_native_host_broker<W: Write + Send + 'static>(
    _writer: Arc<Mutex<W>>,
    _pending: PendingBrokerRequests,
    _notifications: NativeNotifications,
) {
}

#[cfg(unix)]
fn handle_broker_client<W: Write + Send + 'static>(
    mut stream: UnixStream,
    writer: Arc<Mutex<W>>,
    pending: PendingBrokerRequests,
    notifications: NativeNotifications,
    next_id: Arc<AtomicI64>,
) -> Result<(), String> {
    let mut line = String::new();
    let mut reader = BufReader::new(
        stream
            .try_clone()
            .map_err(|err| format!("Could not clone broker stream: {err}"))?,
    );
    reader
        .read_line(&mut line)
        .map_err(|err| format!("Could not read broker request: {err}"))?;
    let request: Value =
        serde_json::from_str(&line).map_err(|err| format!("Invalid broker JSON: {err}"))?;
    let method = request
        .get("method")
        .and_then(Value::as_str)
        .ok_or_else(|| "Broker request missing method.".to_string())?;
    let params = request.get("params").cloned().unwrap_or_else(|| json!({}));
    if method == "__multiagent_poll_notifications" {
        return handle_notification_poll(stream, notifications, params);
    }
    let id = next_id.fetch_add(1, Ordering::Relaxed);
    let native_request = json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": method,
        "params": params,
    });
    let (tx, rx) = mpsc::channel();
    pending
        .lock()
        .map_err(|_| "Pending request lock was poisoned.".to_string())?
        .insert(id, tx);
    {
        let mut writer = writer
            .lock()
            .map_err(|_| "Native writer lock was poisoned.".to_string())?;
        write_native_message(&mut *writer, &native_request)?;
    }
    let response = rx
        .recv_timeout(Duration::from_secs(30))
        .unwrap_or_else(|_| {
            let _ = pending.lock().map(|mut pending| pending.remove(&id));
            json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {
                    "code": -32000,
                    "message": "Timed out waiting for Chrome extension response"
                }
            })
        });
    let payload = serde_json::to_string(&response)
        .map_err(|err| format!("Could not serialize broker response: {err}"))?;
    stream
        .write_all(payload.as_bytes())
        .and_then(|_| stream.write_all(b"\n"))
        .and_then(|_| stream.flush())
        .map_err(|err| format!("Could not write broker response: {err}"))
}

#[cfg(unix)]
fn handle_notification_poll(
    mut stream: UnixStream,
    notifications: NativeNotifications,
    params: Value,
) -> Result<(), String> {
    let limit = params
        .get("limit")
        .and_then(Value::as_u64)
        .unwrap_or(100)
        .clamp(1, MAX_NATIVE_NOTIFICATIONS as u64) as usize;
    let kind = params
        .get("kind")
        .and_then(Value::as_str)
        .map(str::to_string);
    let mut drained = Vec::new();
    if let Ok(mut notifications) = notifications.lock() {
        let mut kept = VecDeque::new();
        while let Some(item) = notifications.pop_front() {
            let matches_kind = kind
                .as_deref()
                .map(|kind| item.get("method").and_then(Value::as_str) == Some(kind))
                .unwrap_or(true);
            if matches_kind && drained.len() < limit {
                drained.push(item);
            } else {
                kept.push_back(item);
            }
        }
        *notifications = kept;
    }
    let payload = serde_json::to_string(&json!({
        "jsonrpc": "2.0",
        "id": null,
        "result": {
            "notifications": drained,
        }
    }))
    .map_err(|err| format!("Could not serialize broker notification response: {err}"))?;
    stream
        .write_all(payload.as_bytes())
        .and_then(|_| stream.write_all(b"\n"))
        .and_then(|_| stream.flush())
        .map_err(|err| format!("Could not write broker notification response: {err}"))
}

fn write_native_message<W: Write>(writer: &mut W, response: &Value) -> Result<(), String> {
    let payload = serde_json::to_vec(response)
        .map_err(|err| format!("Could not serialize native response: {err}"))?;
    let length = u32::try_from(payload.len())
        .map_err(|_| "Native response is too large to send to Chrome.".to_string())?;
    writer
        .write_all(&length.to_le_bytes())
        .and_then(|_| writer.write_all(&payload))
        .and_then(|_| writer.flush())
        .map_err(|err| format!("Could not write native response: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_host_responds_to_ping() {
        let response = handle_native_host_message(
            json!({
                "jsonrpc": "2.0",
                "id": 7,
                "method": "ping"
            }),
            None,
        )
        .expect("ping should receive a response");

        assert_eq!(response["id"], json!(7));
        assert_eq!(response["result"], json!("pong"));
    }

    #[test]
    fn native_host_ignores_notifications() {
        assert!(
            handle_native_host_message(
                json!({
                    "jsonrpc": "2.0",
                    "method": "onCDPEvent",
                    "params": {}
                }),
                None
            )
            .is_none()
        );
    }

    #[test]
    fn native_host_records_notifications_when_store_is_available() {
        let notifications: NativeNotifications = Arc::new(Mutex::new(VecDeque::new()));
        assert!(
            handle_native_host_message(
                json!({
                    "jsonrpc": "2.0",
                    "method": "onDownloadChange",
                    "params": {"id": "1", "status": "complete"}
                }),
                Some(&notifications)
            )
            .is_none()
        );

        let notifications = notifications.lock().expect("notifications lock");
        assert_eq!(notifications.len(), 1);
        assert_eq!(notifications[0]["method"], json!("onDownloadChange"));
        assert_eq!(notifications[0]["params"]["status"], json!("complete"));
    }

    #[test]
    fn native_host_returns_method_not_found_for_unknown_requests() {
        let response = handle_native_host_message(
            json!({
                "jsonrpc": "2.0",
                "id": "abc",
                "method": "unknown"
            }),
            None,
        )
        .expect("unknown request should receive an error response");

        assert_eq!(response["id"], json!("abc"));
        assert_eq!(response["error"]["code"], json!(-32601));
    }

    #[test]
    fn native_manifest_allows_official_extension() {
        let manifest = native_manifest_json(Path::new("/tmp/multiagent-codex-chrome-host"));

        assert!(manifest.contains(NATIVE_HOST_NAME));
        assert!(manifest.contains(&format!("chrome-extension://{CHROME_EXTENSION_ID}/")));
        assert!(manifest.contains("\"type\": \"stdio\""));
    }
}
