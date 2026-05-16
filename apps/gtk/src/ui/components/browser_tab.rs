use adw::prelude::*;
use reqwest::header::CONTENT_TYPE;
use sourceview5::prelude::{BufferExt, ViewExt};
use sourceview5::{
    Buffer as SourceBuffer, LanguageManager, StyleSchemeManager, View as SourceView,
};
use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use crate::services::app::chat::AppDb;

const BROWSER_LAST_TARGET: &str = "browser_last_target";
const MAX_TEXT_PREVIEW_BYTES: usize = 768 * 1024;
const BROWSER_STYLE_SCHEME_ID: &str = "multiagent-preview-dark";

struct BrowserPage {
    title: String,
    meta: String,
    body: String,
    language_id: Option<String>,
    image_path: Option<PathBuf>,
}

pub fn build_browser_tab(
    db: Rc<AppDb>,
    active_workspace_path: Rc<RefCell<Option<String>>>,
) -> gtk::Box {
    let content_box = gtk::Box::new(gtk::Orientation::Vertical, 10);
    content_box.set_margin_start(0);
    content_box.set_margin_end(14);
    content_box.set_margin_top(0);
    content_box.set_margin_bottom(0);
    content_box.set_vexpand(true);

    let frame = gtk::Box::new(gtk::Orientation::Vertical, 0);
    frame.add_css_class("chat-frame");
    frame.set_vexpand(true);

    let root = gtk::Box::new(gtk::Orientation::Vertical, 10);
    root.add_css_class("browser-tab-root");
    root.set_margin_start(10);
    root.set_margin_end(10);
    root.set_margin_top(10);
    root.set_margin_bottom(10);
    root.set_vexpand(true);
    frame.append(&root);

    let header = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    header.add_css_class("browser-tab-header");

    let back_button = icon_button("go-previous-symbolic", "Back");
    let forward_button = icon_button("go-next-symbolic", "Forward");
    let reload_button = icon_button("view-refresh-symbolic", "Reload");

    let target_entry = gtk::Entry::new();
    target_entry.set_hexpand(true);
    target_entry.add_css_class("browser-url-entry");
    target_entry.set_placeholder_text(Some("https://example.com, localhost:5173, or a file path"));

    let go_button = gtk::Button::with_label("Go");
    go_button.add_css_class("suggested-action");
    go_button.add_css_class("browser-go-button");

    let open_external_button = icon_button("web-browser-symbolic", "Open externally");

    header.append(&back_button);
    header.append(&forward_button);
    header.append(&reload_button);
    header.append(&target_entry);
    header.append(&go_button);
    header.append(&open_external_button);

    let shortcut_row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    shortcut_row.add_css_class("browser-shortcuts");
    shortcut_row.append(&shortcut_button("localhost:5173", "http://localhost:5173"));
    shortcut_row.append(&shortcut_button("localhost:3000", "http://localhost:3000"));
    shortcut_row.append(&shortcut_button("localhost:8080", "http://localhost:8080"));
    if let Some(workspace) = active_workspace_path.borrow().clone() {
        shortcut_row.append(&shortcut_button("Workspace", &workspace));
    }

    let title_label = gtk::Label::new(Some("Browser"));
    title_label.set_xalign(0.0);
    title_label.set_hexpand(true);
    title_label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    title_label.add_css_class("browser-title");

    let meta_label = gtk::Label::new(Some("Ready"));
    meta_label.set_xalign(1.0);
    meta_label.add_css_class("browser-meta");

    let meta_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    meta_row.add_css_class("browser-meta-row");
    meta_row.append(&title_label);
    meta_row.append(&meta_label);

    let source_buffer = SourceBuffer::new(None);
    source_buffer.set_highlight_syntax(true);
    source_buffer.set_highlight_matching_brackets(false);
    apply_browser_style_scheme(&source_buffer);
    source_buffer.set_text("Enter a URL or local file path to preview it here.");

    let source_view = SourceView::with_buffer(&source_buffer);
    source_view.set_editable(false);
    source_view.set_cursor_visible(false);
    source_view.set_monospace(true);
    source_view.set_show_line_numbers(false);
    source_view.set_tab_width(4);
    source_view.add_css_class("browser-source");

    let text_scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Automatic)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .hexpand(true)
        .vexpand(true)
        .child(&source_view)
        .build();
    text_scroll.add_css_class("browser-content-scroll");

    let image_view = gtk::Picture::new();
    image_view.set_can_shrink(true);
    image_view.set_content_fit(gtk::ContentFit::Contain);
    image_view.add_css_class("browser-image");

    let image_scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Automatic)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .hexpand(true)
        .vexpand(true)
        .child(&image_view)
        .build();
    image_scroll.add_css_class("browser-content-scroll");

    let preview_stack = gtk::Stack::new();
    preview_stack.add_named(&text_scroll, Some("text"));
    preview_stack.add_named(&image_scroll, Some("image"));
    preview_stack.set_visible_child_name("text");
    preview_stack.set_vexpand(true);
    preview_stack.set_hexpand(true);

    root.append(&header);
    root.append(&shortcut_row);
    root.append(&meta_row);
    root.append(&preview_stack);
    content_box.append(&frame);

    if let Ok(Some(last_target)) = db.get_setting(BROWSER_LAST_TARGET) {
        target_entry.set_text(&last_target);
    }

    let history = Rc::new(RefCell::new(Vec::<String>::new()));
    let history_index = Rc::new(RefCell::new(None::<usize>));
    let receiver_slot = Rc::new(RefCell::new(
        None::<mpsc::Receiver<Result<BrowserPage, String>>>,
    ));

    let update_nav: Rc<dyn Fn()> = {
        let history = history.clone();
        let history_index = history_index.clone();
        let back_button = back_button.clone();
        let forward_button = forward_button.clone();
        Rc::new(move || {
            let idx = *history_index.borrow();
            let len = history.borrow().len();
            back_button.set_sensitive(idx.map(|value| value > 0).unwrap_or(false));
            forward_button.set_sensitive(idx.map(|value| value + 1 < len).unwrap_or(false));
        })
    };
    update_nav();

    let start_load: Rc<dyn Fn(String, bool)> = {
        let db = db.clone();
        let active_workspace_path = active_workspace_path.clone();
        let source_buffer = source_buffer.clone();
        let preview_stack = preview_stack.clone();
        let title_label = title_label.clone();
        let meta_label = meta_label.clone();
        let target_entry = target_entry.clone();
        let history = history.clone();
        let history_index = history_index.clone();
        let update_nav = update_nav.clone();
        let receiver_slot = receiver_slot.clone();
        Rc::new(move |raw_target: String, push_history: bool| {
            let target = normalize_target(&raw_target, active_workspace_path.borrow().as_deref());
            if target.trim().is_empty() {
                meta_label.set_text("No target");
                return;
            }

            target_entry.set_text(&target);
            let _ = db.set_setting(BROWSER_LAST_TARGET, &target);

            if push_history {
                let mut history_ref = history.borrow_mut();
                if let Some(idx) = *history_index.borrow() {
                    history_ref.truncate(idx + 1);
                }
                if history_ref
                    .last()
                    .map(|item| item != &target)
                    .unwrap_or(true)
                {
                    history_ref.push(target.clone());
                    history_index.replace(Some(history_ref.len().saturating_sub(1)));
                }
            }
            update_nav();

            title_label.set_text("Loading");
            meta_label.set_text(&target);
            preview_stack.set_visible_child_name("text");
            source_buffer.set_language(None::<&sourceview5::Language>);
            source_buffer.set_text("Loading...");

            let (tx, rx) = mpsc::channel();
            receiver_slot.replace(Some(rx));
            thread::spawn(move || {
                let _ = tx.send(load_browser_page(&target));
            });
        })
    };

    {
        let start_load = start_load.clone();
        let target_entry = target_entry.clone();
        go_button.connect_clicked(move |_| {
            start_load(target_entry.text().to_string(), true);
        });
    }

    {
        let start_load = start_load.clone();
        target_entry.connect_activate(move |entry| {
            start_load(entry.text().to_string(), true);
        });
    }

    {
        let start_load = start_load.clone();
        let target_entry = target_entry.clone();
        reload_button.connect_clicked(move |_| {
            start_load(target_entry.text().to_string(), false);
        });
    }

    {
        let start_load = start_load.clone();
        let history = history.clone();
        let history_index = history_index.clone();
        back_button.connect_clicked(move |_| {
            let Some(idx) = *history_index.borrow() else {
                return;
            };
            if idx == 0 {
                return;
            }
            let next_idx = idx - 1;
            history_index.replace(Some(next_idx));
            if let Some(target) = history.borrow().get(next_idx).cloned() {
                start_load(target, false);
            }
        });
    }

    {
        let start_load = start_load.clone();
        let history = history.clone();
        let history_index = history_index.clone();
        forward_button.connect_clicked(move |_| {
            let Some(idx) = *history_index.borrow() else {
                return;
            };
            let next_idx = idx + 1;
            if let Some(target) = history.borrow().get(next_idx).cloned() {
                history_index.replace(Some(next_idx));
                start_load(target, false);
            }
        });
    }

    {
        let target_entry = target_entry.clone();
        let meta_label = meta_label.clone();
        open_external_button.connect_clicked(move |_| {
            let target = normalize_target(&target_entry.text(), None);
            match external_uri_for_target(&target) {
                Some(uri) => {
                    if gtk::gio::AppInfo::launch_default_for_uri(
                        &uri,
                        None::<&gtk::gio::AppLaunchContext>,
                    )
                    .is_err()
                    {
                        meta_label.set_text("Unable to open externally");
                    }
                }
                None => meta_label.set_text("No target to open"),
            }
        });
    }

    for child in shortcut_row.observe_children().snapshot() {
        let Ok(button) = child.downcast::<gtk::Button>() else {
            continue;
        };
        if let Some(target) = button.tooltip_text() {
            let start_load = start_load.clone();
            button.connect_clicked(move |_| {
                start_load(target.to_string(), true);
            });
        }
    }

    {
        let receiver_slot = receiver_slot.clone();
        let source_buffer = source_buffer.clone();
        let image_view = image_view.clone();
        let preview_stack = preview_stack.clone();
        let title_label = title_label.clone();
        let meta_label = meta_label.clone();
        gtk::glib::timeout_add_local(Duration::from_millis(80), move || {
            let result = receiver_slot
                .borrow()
                .as_ref()
                .and_then(|rx| match rx.try_recv() {
                    Ok(result) => Some(result),
                    Err(mpsc::TryRecvError::Empty) => None,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        Some(Err("Browser worker disconnected.".to_string()))
                    }
                });

            if let Some(result) = result {
                receiver_slot.replace(None);
                match result {
                    Ok(page) => {
                        title_label.set_text(&page.title);
                        meta_label.set_text(&page.meta);
                        if let Some(image_path) = page.image_path {
                            image_view.set_filename(Some(&image_path));
                            preview_stack.set_visible_child_name("image");
                        } else {
                            let manager = LanguageManager::default();
                            source_buffer.set_language(
                                page.language_id
                                    .as_deref()
                                    .and_then(|id| manager.language(id))
                                    .as_ref(),
                            );
                            source_buffer.set_text(&page.body);
                            preview_stack.set_visible_child_name("text");
                        }
                    }
                    Err(err) => {
                        title_label.set_text("Load failed");
                        meta_label.set_text("Error");
                        source_buffer.set_language(None::<&sourceview5::Language>);
                        source_buffer.set_text(&err);
                        preview_stack.set_visible_child_name("text");
                    }
                }
            }

            gtk::glib::ControlFlow::Continue
        });
    }

    content_box
}

fn icon_button(icon_name: &str, tooltip: &str) -> gtk::Button {
    let button = gtk::Button::new();
    button.set_has_frame(false);
    button.add_css_class("app-flat-button");
    button.add_css_class("browser-icon-button");
    button.set_icon_name(icon_name);
    button.set_tooltip_text(Some(tooltip));
    button
}

fn shortcut_button(label: &str, target: &str) -> gtk::Button {
    let button = gtk::Button::with_label(label);
    button.add_css_class("app-flat-button");
    button.add_css_class("browser-shortcut-button");
    button.set_tooltip_text(Some(target));
    button
}

fn normalize_target(raw: &str, workspace_path: Option<&str>) -> String {
    let target = raw.trim();
    if target.is_empty() {
        return workspace_path.unwrap_or("").to_string();
    }

    if target.starts_with("http://")
        || target.starts_with("https://")
        || target.starts_with("file://")
    {
        return target.to_string();
    }

    let path = PathBuf::from(target);
    if path.exists() {
        return path.to_string_lossy().to_string();
    }

    if target.starts_with("localhost:") || target.starts_with("127.0.0.1:") {
        return format!("http://{target}");
    }

    if target.contains('.') && !target.contains(' ') {
        return format!("https://{target}");
    }

    let query = target.split_whitespace().collect::<Vec<_>>().join("+");
    format!("https://duckduckgo.com/?q={query}")
}

fn external_uri_for_target(target: &str) -> Option<String> {
    if target.trim().is_empty() {
        return None;
    }
    if target.starts_with("http://")
        || target.starts_with("https://")
        || target.starts_with("file://")
    {
        return Some(target.to_string());
    }
    let path = PathBuf::from(target);
    if path.exists() {
        return Some(format!("file://{}", path.to_string_lossy()));
    }
    Some(normalize_target(target, None))
}

fn load_browser_page(target: &str) -> Result<BrowserPage, String> {
    if target.starts_with("http://") || target.starts_with("https://") {
        load_http_page(target)
    } else {
        let path = target
            .strip_prefix("file://")
            .map(|value| value.replace("%20", " "))
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(target));
        load_file_page(&path)
    }
}

fn load_http_page(target: &str) -> Result<BrowserPage, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(25))
        .redirect(reqwest::redirect::Policy::limited(8))
        .build()
        .map_err(|err| format!("Browser setup failed: {err}"))?;
    let response = client
        .get(target)
        .header(reqwest::header::USER_AGENT, "MultiAGENT/0.1")
        .send()
        .map_err(|err| format!("Failed to load {target}: {err}"))?;
    let status = response.status();
    let final_url = response.url().to_string();
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_string();
    let bytes = response
        .bytes()
        .map_err(|err| format!("Failed to read response body: {err}"))?;
    let mut body = String::from_utf8_lossy(&bytes[..bytes.len().min(MAX_TEXT_PREVIEW_BYTES)])
        .replace('\0', "");
    if bytes.len() > MAX_TEXT_PREVIEW_BYTES {
        body.push_str("\n\n--- Preview truncated ---");
    }
    if body.trim().is_empty() {
        body = format!("No readable text body.\n\nContent-Type: {content_type}");
    }

    let title = extract_html_title(&body).unwrap_or_else(|| final_url.clone());
    Ok(BrowserPage {
        title,
        meta: format!(
            "HTTP {} • {}",
            status.as_u16(),
            content_type_or_unknown(&content_type)
        ),
        language_id: language_for_content_type(&content_type),
        body,
        image_path: None,
    })
}

fn load_file_page(path: &Path) -> Result<BrowserPage, String> {
    if !path.exists() {
        return Err(format!("File not found: {}", path.display()));
    }
    let title = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("File")
        .to_string();
    if is_image_path(path) {
        return Ok(BrowserPage {
            title,
            meta: path.display().to_string(),
            body: String::new(),
            language_id: None,
            image_path: Some(path.to_path_buf()),
        });
    }

    let bytes =
        fs::read(path).map_err(|err| format!("Unable to read {}: {err}", path.display()))?;
    if bytes.iter().take(512).any(|byte| *byte == 0) {
        return Ok(BrowserPage {
            title,
            meta: format!("{} • binary", path.display()),
            body: "Binary file preview is not supported.".to_string(),
            language_id: None,
            image_path: None,
        });
    }
    let mut body = String::from_utf8_lossy(&bytes[..bytes.len().min(MAX_TEXT_PREVIEW_BYTES)])
        .replace('\0', "");
    if bytes.len() > MAX_TEXT_PREVIEW_BYTES {
        body.push_str("\n\n--- Preview truncated ---");
    }
    let manager = LanguageManager::default();
    let language_id = manager
        .guess_language(path.to_str(), None::<&str>)
        .map(|lang| lang.id().to_string());

    Ok(BrowserPage {
        title,
        meta: path.display().to_string(),
        body,
        language_id,
        image_path: None,
    })
}

fn is_image_path(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.to_ascii_lowercase())
            .as_deref(),
        Some("png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp" | "svg" | "ico")
    )
}

fn content_type_or_unknown(content_type: &str) -> &str {
    if content_type.trim().is_empty() {
        "unknown content"
    } else {
        content_type
    }
}

fn language_for_content_type(content_type: &str) -> Option<String> {
    let lower = content_type.to_ascii_lowercase();
    if lower.contains("html") {
        Some("html".to_string())
    } else if lower.contains("json") {
        Some("json".to_string())
    } else if lower.contains("css") {
        Some("css".to_string())
    } else if lower.contains("javascript") || lower.contains("ecmascript") {
        Some("js".to_string())
    } else if lower.contains("xml") {
        Some("xml".to_string())
    } else {
        None
    }
}

fn extract_html_title(body: &str) -> Option<String> {
    let lower = body.to_ascii_lowercase();
    let start = lower.find("<title")?;
    let after_open = lower[start..].find('>')? + start + 1;
    let end = lower[after_open..].find("</title>")? + after_open;
    let title = body[after_open..end].trim();
    if title.is_empty() {
        None
    } else {
        Some(title.replace('\n', " ").replace('\t', " "))
    }
}

fn apply_browser_style_scheme(buffer: &SourceBuffer) {
    let manager = StyleSchemeManager::default();
    if manager.scheme(BROWSER_STYLE_SCHEME_ID).is_none() {
        manager.append_search_path("/usr/share/gtksourceview-5/styles");
    }
    if let Some(style) = manager
        .scheme(BROWSER_STYLE_SCHEME_ID)
        .or_else(|| manager.scheme("Adwaita-dark"))
        .or_else(|| manager.scheme("classic"))
    {
        buffer.set_style_scheme(Some(&style));
    }
}
