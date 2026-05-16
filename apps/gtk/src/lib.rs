pub use multiagent::services;

pub mod actions;
pub mod ui;

pub const APP_ID: &str = "dev.multiagent.multiagent";
pub const APP_ICON: &str = "dev.multiagent.multiagent";

pub fn app_flavor() -> &'static str {
    if let Ok(value) = std::env::var("MULTIAGENT_APP_FLAVOR") {
        if value.eq_ignore_ascii_case("opencode") {
            return "opencode";
        }
        if value.eq_ignore_ascii_case("codex") {
            return "codex";
        }
    }
    if std::env::args().any(|arg| arg == "--opencode") {
        return "opencode";
    }
    if std::env::args().any(|arg| arg == "--codex") {
        return "codex";
    }
    std::env::args()
        .next()
        .and_then(|arg| {
            std::path::Path::new(&arg)
                .file_name()
                .map(|name| name.to_string_lossy().to_ascii_lowercase())
        })
        .map(|name| {
            if name.contains("opencode") {
                "opencode"
            } else {
                "codex"
            }
        })
        .unwrap_or("codex")
}

pub fn app_id() -> &'static str {
    match app_flavor() {
        "opencode" => "dev.multiagent.multiagent",
        _ => "dev.multiagent.multiagent",
    }
}

pub fn app_name() -> &'static str {
    match app_flavor() {
        "opencode" => "MultiAGENT",
        _ => "MultiAGENT",
    }
}

pub fn default_backend_kind() -> &'static str {
    app_flavor()
}
