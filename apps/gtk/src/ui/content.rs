use adw::prelude::*;
use serde_json::Value;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use crate::services::app::CodexProfileManager;
use crate::services::app::chat::AppDb;
use crate::services::app::runtime::RuntimeClient;
use crate::ui::settings;

use crate::ui::components::{
    actions_tab, bottom_bar::build_bottom_bar, browser_tab, chat, command_palette, git_tab, top_bar,
};

fn parse_thread_drop_payload(raw: &str) -> Option<(Option<i64>, Option<String>, Option<String>)> {
    let parsed: Value = serde_json::from_str(raw).ok()?;
    let local_thread_id = parsed
        .get("localThreadId")
        .or_else(|| parsed.get("local_thread_id"))
        .and_then(Value::as_i64);
    let thread_id = parsed
        .get("threadId")
        .or_else(|| parsed.get("codexThreadId"))
        .and_then(Value::as_str)
        .map(|value| value.to_string());
    let workspace_path = parsed
        .get("workspacePath")
        .and_then(Value::as_str)
        .map(|value| value.to_string());
    Some((local_thread_id, thread_id, workspace_path))
}

fn focus_thread_from_payload(
    db: &AppDb,
    active_thread_id: &Rc<RefCell<Option<String>>>,
    active_workspace_path: &Rc<RefCell<Option<String>>>,
    raw: String,
) -> bool {
    let Some((local_thread_id, remote_thread_id, payload_workspace)) =
        parse_thread_drop_payload(&raw)
    else {
        return false;
    };
    let thread = local_thread_id
        .and_then(|id| db.get_thread_record(id).ok().flatten())
        .or_else(|| {
            remote_thread_id
                .as_deref()
                .and_then(|id| db.get_thread_record_by_remote_thread_id(id).ok().flatten())
        });
    let Some(thread) = thread else {
        return false;
    };

    settings::force_single_thread_mode(db);
    let _ = db.set_runtime_profile_id(thread.profile_id);
    let _ = db.set_active_profile_id(thread.profile_id);
    let _ = db.set_current_profile_account_identity(
        thread.remote_account_type(),
        thread.remote_account_email(),
    );

    let workspace_path = payload_workspace
        .or_else(|| {
            thread
                .worktree_path
                .as_deref()
                .map(str::trim)
                .filter(|path| thread.worktree_active && !path.is_empty())
                .map(|path| path.to_string())
        })
        .or_else(|| db.workspace_path_for_local_thread(thread.id).ok().flatten());
    if let Some(path) = workspace_path {
        active_workspace_path.replace(Some(path.clone()));
        let _ = db.set_setting("last_active_workspace_path", &path);
    }

    let _ = db.set_setting("last_active_thread_id", &thread.id.to_string());
    if let Some(remote_id) = thread.remote_thread_id_owned() {
        active_thread_id.replace(Some(remote_id));
        let _ = db.set_setting("pending_profile_thread_id", "");
    } else {
        active_thread_id.replace(None);
        let _ = db.set_setting("pending_profile_thread_id", &thread.id.to_string());
    }
    true
}

fn build_classic_content(
    db: Rc<AppDb>,
    profile_manager: Rc<CodexProfileManager>,
    codex: Option<Arc<RuntimeClient>>,
    active_thread_id: Rc<RefCell<Option<String>>>,
    active_workspace_path: Rc<RefCell<Option<String>>>,
) -> gtk::Box {
    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.set_vexpand(true);
    root.set_hexpand(true);

    let stack = adw::ViewStack::new();
    stack.set_widget_name("main-content-view-stack");
    stack.set_vexpand(true);
    stack.set_hexpand(true);
    stack.add_named(
        &chat::build_chat_tab(
            db.clone(),
            profile_manager.clone(),
            codex.clone(),
            active_thread_id.clone(),
            active_workspace_path.clone(),
        ),
        Some("chat"),
    );
    stack.add_named(
        &git_tab::build_git_tab(db.clone(), active_workspace_path.clone()),
        Some("git"),
    );
    stack.add_named(
        &actions_tab::build_actions_tab(db.clone(), active_workspace_path.clone()),
        Some("actions"),
    );
    stack.set_visible_child_name("chat");

    let browser_split_toggle = gtk::ToggleButton::new();
    browser_split_toggle.set_icon_name("web-browser-symbolic");
    browser_split_toggle.set_has_frame(false);
    browser_split_toggle.set_focus_on_click(false);
    browser_split_toggle.set_tooltip_text(Some("Open Browser Split"));
    browser_split_toggle.add_css_class("app-flat-button");
    browser_split_toggle.add_css_class("topbar-browser-split-button");

    let browser_split_revealer = gtk::Revealer::new();
    browser_split_revealer.set_transition_type(gtk::RevealerTransitionType::SlideLeft);
    browser_split_revealer.set_transition_duration(180);
    browser_split_revealer.set_reveal_child(false);
    browser_split_revealer.set_hexpand(false);
    browser_split_revealer.set_vexpand(true);
    browser_split_revealer.add_css_class("browser-split-revealer");

    let browser_split_panel = gtk::Box::new(gtk::Orientation::Vertical, 0);
    browser_split_panel.add_css_class("browser-split-panel");
    browser_split_panel.set_width_request(440);
    browser_split_panel.set_size_request(380, -1);
    browser_split_panel.set_vexpand(true);
    browser_split_panel.set_hexpand(false);
    browser_split_panel.append(&browser_tab::build_browser_tab(
        db.clone(),
        active_workspace_path.clone(),
    ));
    browser_split_revealer.set_child(Some(&browser_split_panel));

    {
        let browser_split_revealer = browser_split_revealer.clone();
        browser_split_toggle.connect_toggled(move |toggle| {
            let is_open = toggle.is_active();
            browser_split_revealer.set_reveal_child(is_open);
            toggle.set_tooltip_text(Some(if is_open {
                "Close Browser Split"
            } else {
                "Open Browser Split"
            }));
        });
    }

    let top = top_bar::build_top_bar(
        Some(&stack),
        db.clone(),
        profile_manager.clone(),
        active_workspace_path.clone(),
        Some(&browser_split_toggle),
    );
    root.append(&top);
    command_palette::install(
        &root,
        &stack,
        db.clone(),
        profile_manager.clone(),
        active_workspace_path.clone(),
        Some(&browser_split_toggle),
    );

    let editor_area = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    editor_area.add_css_class("focused-editor-area");
    editor_area.set_hexpand(true);
    editor_area.set_vexpand(true);
    editor_area.append(&stack);
    editor_area.append(&browser_split_revealer);
    root.append(&editor_area);

    let handle_drop_payload: Rc<dyn Fn(String) -> bool> = Rc::new({
        let db = db.clone();
        let active_thread_id = active_thread_id.clone();
        let active_workspace_path = active_workspace_path.clone();
        let stack = stack.clone();
        move |raw: String| {
            let focused = focus_thread_from_payload(
                db.as_ref(),
                &active_thread_id,
                &active_workspace_path,
                raw,
            );
            if focused {
                stack.set_visible_child_name("chat");
            }
            focused
        }
    });

    let drop_target_root = gtk::DropTarget::new(String::static_type(), gtk::gdk::DragAction::COPY);
    drop_target_root.connect_drop({
        let handle_drop_payload = handle_drop_payload.clone();
        move |_, value, _, _| {
            let Ok(raw) = value.get::<String>() else {
                return false;
            };
            handle_drop_payload(raw)
        }
    });
    root.add_controller(drop_target_root);

    let drop_target_stack = gtk::DropTarget::new(String::static_type(), gtk::gdk::DragAction::COPY);
    drop_target_stack.connect_drop({
        let handle_drop_payload = handle_drop_payload.clone();
        move |_, value, _, _| {
            let Ok(raw) = value.get::<String>() else {
                return false;
            };
            handle_drop_payload(raw)
        }
    });
    stack.add_controller(drop_target_stack);

    root
}

pub fn build_content(
    db: Rc<AppDb>,
    profile_manager: Rc<CodexProfileManager>,
    codex: Option<Arc<RuntimeClient>>,
    active_thread_id: Rc<RefCell<Option<String>>>,
    active_workspace_path: Rc<RefCell<Option<String>>>,
) -> adw::ToolbarView {
    settings::force_single_thread_mode(db.as_ref());

    let toolbar = adw::ToolbarView::new();
    toolbar.set_top_bar_style(adw::ToolbarStyle::Flat);
    toolbar.set_bottom_bar_style(adw::ToolbarStyle::Flat);
    toolbar.add_css_class("content-area");

    let content_shell = gtk::Box::new(gtk::Orientation::Vertical, 0);
    content_shell.add_css_class("content-shell");
    content_shell.set_hexpand(true);
    content_shell.set_vexpand(true);

    content_shell.append(&build_classic_content(
        db.clone(),
        profile_manager.clone(),
        codex.clone(),
        active_thread_id,
        active_workspace_path,
    ));
    toolbar.set_content(Some(&content_shell));

    let bottom = build_bottom_bar(db.clone(), profile_manager);
    toolbar.add_bottom_bar(&bottom);
    toolbar
}
