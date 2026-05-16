use adw::prelude::*;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use crate::actions::{
    ActionRunSnapshot, SavedWorkspaceAction, action_runner, canonical_workspace_path,
    load_workspace_actions, remove_workspace_action, save_workspace_action,
};
use crate::services::app::chat::AppDb;
use crate::ui::widget_tree;

pub fn build_actions_tab(
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
    root.add_css_class("actions-tab-root");
    root.set_margin_start(10);
    root.set_margin_end(10);
    root.set_margin_top(10);
    root.set_margin_bottom(10);
    root.set_vexpand(true);
    frame.append(&root);

    let header = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    header.add_css_class("actions-tab-header");

    let title_box = gtk::Box::new(gtk::Orientation::Vertical, 2);
    title_box.set_hexpand(true);
    let title = gtk::Label::new(Some("Terminal & Actions"));
    title.add_css_class("actions-tab-title");
    title.set_xalign(0.0);
    let workspace_label = gtk::Label::new(Some("No workspace selected"));
    workspace_label.add_css_class("actions-tab-workspace");
    workspace_label.set_xalign(0.0);
    workspace_label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    title_box.append(&title);
    title_box.append(&workspace_label);

    let refresh_button = gtk::Button::new();
    refresh_button.set_has_frame(false);
    refresh_button.set_icon_name("view-refresh-symbolic");
    refresh_button.add_css_class("app-flat-button");
    refresh_button.add_css_class("actions-tab-icon-button");
    refresh_button.set_tooltip_text(Some("Refresh actions"));

    header.append(&title_box);
    header.append(&refresh_button);

    let command_card = gtk::Box::new(gtk::Orientation::Vertical, 8);
    command_card.add_css_class("actions-tab-command-card");

    let command_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let title_entry = gtk::Entry::new();
    title_entry.set_width_chars(18);
    title_entry.set_placeholder_text(Some("Name"));
    title_entry.add_css_class("actions-tab-entry");

    let command_entry = gtk::Entry::new();
    command_entry.set_hexpand(true);
    command_entry.set_placeholder_text(Some("Command"));
    command_entry.add_css_class("actions-tab-entry");

    let run_button = gtk::Button::with_label("Run");
    run_button.add_css_class("suggested-action");
    run_button.add_css_class("actions-tab-primary-button");

    let save_button = gtk::Button::with_label("Save");
    save_button.add_css_class("actions-tab-secondary-button");

    command_row.append(&title_entry);
    command_row.append(&command_entry);
    command_row.append(&run_button);
    command_row.append(&save_button);
    command_card.append(&command_row);

    let status_label = gtk::Label::new(None);
    status_label.set_xalign(0.0);
    status_label.set_wrap(true);
    status_label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    status_label.add_css_class("actions-tab-status");
    command_card.append(&status_label);

    let body = gtk::Paned::new(gtk::Orientation::Horizontal);
    body.add_css_class("actions-tab-body");
    body.set_wide_handle(true);
    body.set_resize_start_child(true);
    body.set_resize_end_child(true);
    body.set_shrink_start_child(false);
    body.set_shrink_end_child(false);
    body.set_position(420);
    body.set_vexpand(true);

    let saved_panel = panel("Saved Commands");
    let saved_box = gtk::Box::new(gtk::Orientation::Vertical, 7);
    saved_box.add_css_class("actions-tab-list");
    let saved_scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .child(&saved_box)
        .vexpand(true)
        .build();
    saved_scroll.add_css_class("actions-tab-scroll");
    saved_panel.append(&saved_scroll);

    let runs_panel = panel("Recent Output");
    let runs_box = gtk::Box::new(gtk::Orientation::Vertical, 7);
    runs_box.add_css_class("actions-tab-list");
    let runs_scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Automatic)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .child(&runs_box)
        .vexpand(true)
        .build();
    runs_scroll.add_css_class("actions-tab-scroll");
    runs_panel.append(&runs_scroll);

    body.set_start_child(Some(&saved_panel));
    body.set_end_child(Some(&runs_panel));

    root.append(&header);
    root.append(&command_card);
    root.append(&body);
    content_box.append(&frame);

    let last_signature = Rc::new(RefCell::new(String::new()));
    let refresh_handle: Rc<RefCell<Option<Rc<dyn Fn(bool)>>>> = Rc::new(RefCell::new(None));
    let refresh_fn: Rc<dyn Fn(bool)> = {
        let db = db.clone();
        let active_workspace_path = active_workspace_path.clone();
        let workspace_label = workspace_label.clone();
        let saved_box = saved_box.clone();
        let runs_box = runs_box.clone();
        let status_label = status_label.clone();
        let last_signature = last_signature.clone();
        let refresh_handle = refresh_handle.clone();
        Rc::new(move |force: bool| {
            let workspace = current_workspace_path(&active_workspace_path);
            if let Some(path) = workspace.as_deref() {
                workspace_label.set_text(&format!("Workspace: {}", workspace_display_name(path)));
            } else {
                workspace_label.set_text("No workspace selected");
            }

            let saved_actions = workspace
                .as_deref()
                .map(|path| load_workspace_actions(db.as_ref(), path))
                .unwrap_or_default();
            let runs = workspace
                .as_deref()
                .map(|path| action_runner().list_for_workspace(path))
                .unwrap_or_default();
            let signature = signature(workspace.as_deref(), &saved_actions, &runs);
            if !force && *last_signature.borrow() == signature {
                return;
            }
            last_signature.replace(signature);

            widget_tree::clear_box_children(&saved_box);
            widget_tree::clear_box_children(&runs_box);

            if workspace.is_none() {
                status_label.set_text("Select a thread with a workspace first.");
            } else if status_label.text().trim().is_empty() {
                status_label.set_text("");
            }

            if saved_actions.is_empty() {
                saved_box.append(&empty_label("No saved commands yet."));
            } else {
                let runs_by_command = runs
                    .iter()
                    .cloned()
                    .map(|run| (run.command.clone(), run))
                    .collect::<HashMap<String, ActionRunSnapshot>>();
                for action in saved_actions {
                    let latest_run = runs_by_command.get(&action.command);
                    saved_box.append(&saved_action_card(
                        action,
                        latest_run,
                        workspace.clone(),
                        db.clone(),
                        status_label.clone(),
                        refresh_handle.clone(),
                    ));
                }
            }

            if runs.is_empty() {
                runs_box.append(&empty_label("No commands have run in this workspace."));
            } else {
                for run in runs {
                    runs_box.append(&run_card(run, status_label.clone()));
                }
            }
        })
    };
    refresh_handle.replace(Some(refresh_fn.clone()));

    {
        let refresh_fn = refresh_fn.clone();
        refresh_button.connect_clicked(move |_| refresh_fn(true));
    }

    {
        let active_workspace_path = active_workspace_path.clone();
        let command_entry = command_entry.clone();
        let title_entry = title_entry.clone();
        let status_label = status_label.clone();
        let refresh_handle = refresh_handle.clone();
        run_button.connect_clicked(move |_| {
            let Some(workspace) = current_workspace_path(&active_workspace_path) else {
                status_label.set_text("No active workspace.");
                return;
            };
            let command = command_entry.text().trim().to_string();
            if command.is_empty() {
                status_label.set_text("Command cannot be empty.");
                return;
            }
            let title = title_entry.text().trim().to_string();
            match action_runner().start(
                &workspace,
                if title.is_empty() { None } else { Some(&title) },
                &command,
            ) {
                Ok(_) => {
                    status_label.set_text("Command started.");
                    if let Some(refresh) = refresh_handle.borrow().as_ref() {
                        refresh(true);
                    }
                }
                Err(err) => status_label.set_text(&err),
            }
        });
    }

    {
        let db = db.clone();
        let active_workspace_path = active_workspace_path.clone();
        let command_entry = command_entry.clone();
        let title_entry = title_entry.clone();
        let status_label = status_label.clone();
        let refresh_handle = refresh_handle.clone();
        save_button.connect_clicked(move |_| {
            let Some(workspace) = current_workspace_path(&active_workspace_path) else {
                status_label.set_text("No active workspace.");
                return;
            };
            let title = title_entry.text().trim().to_string();
            let command = command_entry.text().trim().to_string();
            match save_workspace_action(
                db.as_ref(),
                &workspace,
                if title.is_empty() { None } else { Some(&title) },
                &command,
            ) {
                Ok(()) => {
                    status_label.set_text("Command saved.");
                    if let Some(refresh) = refresh_handle.borrow().as_ref() {
                        refresh(true);
                    }
                }
                Err(err) => status_label.set_text(&err),
            }
        });
    }

    {
        let run_button = run_button.clone();
        command_entry.connect_activate(move |_| {
            run_button.emit_clicked();
        });
    }

    {
        let refresh_fn = refresh_fn.clone();
        gtk::glib::timeout_add_local(Duration::from_millis(320), move || {
            refresh_fn(false);
            gtk::glib::ControlFlow::Continue
        });
    }

    refresh_fn(true);
    content_box
}

fn panel(title: &str) -> gtk::Box {
    let panel = gtk::Box::new(gtk::Orientation::Vertical, 8);
    panel.add_css_class("actions-tab-panel");
    panel.set_vexpand(true);
    panel.set_hexpand(true);
    let label = gtk::Label::new(Some(title));
    label.add_css_class("actions-tab-panel-title");
    label.set_xalign(0.0);
    panel.append(&label);
    panel
}

fn saved_action_card(
    action: SavedWorkspaceAction,
    latest_run: Option<&ActionRunSnapshot>,
    workspace: Option<String>,
    db: Rc<AppDb>,
    status_label: gtk::Label,
    refresh_handle: Rc<RefCell<Option<Rc<dyn Fn(bool)>>>>,
) -> gtk::Box {
    let card = gtk::Box::new(gtk::Orientation::Vertical, 5);
    card.add_css_class("actions-tab-card");

    let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let title = gtk::Label::new(Some(
        action
            .title
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("Command"),
    ));
    title.add_css_class("actions-tab-card-title");
    title.set_xalign(0.0);
    title.set_hexpand(true);
    row.append(&title);

    if let Some(run) = latest_run {
        let state = gtk::Label::new(Some(&run.status_text));
        state.add_css_class("actions-tab-run-state");
        row.append(&state);
    }

    let run_button = gtk::Button::with_label("Run");
    run_button.add_css_class("app-flat-button");
    run_button.add_css_class("actions-tab-mini-button");
    row.append(&run_button);

    let delete_button = gtk::Button::new();
    delete_button.set_has_frame(false);
    delete_button.set_icon_name("user-trash-symbolic");
    delete_button.add_css_class("app-flat-button");
    delete_button.add_css_class("actions-tab-mini-button");
    delete_button.set_tooltip_text(Some("Remove action"));
    row.append(&delete_button);

    let command = gtk::Label::new(Some(&action.command));
    command.set_xalign(0.0);
    command.set_wrap(true);
    command.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    command.add_css_class("actions-tab-command-text");

    card.append(&row);
    card.append(&command);

    {
        let action = action.clone();
        let workspace = workspace.clone();
        let status_label = status_label.clone();
        let refresh_handle = refresh_handle.clone();
        run_button.connect_clicked(move |_| {
            let Some(workspace) = workspace.clone() else {
                status_label.set_text("No active workspace.");
                return;
            };
            match action_runner().start(&workspace, action.title.as_deref(), &action.command) {
                Ok(_) => {
                    status_label.set_text("Command started.");
                    if let Some(refresh) = refresh_handle.borrow().as_ref() {
                        refresh(true);
                    }
                }
                Err(err) => status_label.set_text(&err),
            }
        });
    }

    {
        let action = action.clone();
        let workspace = workspace.clone();
        let status_label = status_label.clone();
        let refresh_handle = refresh_handle.clone();
        delete_button.connect_clicked(move |_| {
            let Some(workspace) = workspace.clone() else {
                status_label.set_text("No active workspace.");
                return;
            };
            match remove_workspace_action(db.as_ref(), &workspace, &action.id) {
                Ok(()) => {
                    status_label.set_text("Action removed.");
                    if let Some(refresh) = refresh_handle.borrow().as_ref() {
                        refresh(true);
                    }
                }
                Err(err) => status_label.set_text(&err),
            }
        });
    }

    card
}

fn run_card(run: ActionRunSnapshot, status_label: gtk::Label) -> gtk::Box {
    let card = gtk::Box::new(gtk::Orientation::Vertical, 6);
    card.add_css_class("actions-tab-card");
    card.add_css_class("actions-tab-run-card");

    let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let title = gtk::Label::new(Some(&run.title));
    title.set_xalign(0.0);
    title.set_hexpand(true);
    title.add_css_class("actions-tab-card-title");
    row.append(&title);

    let state = gtk::Label::new(Some(&run.status_text));
    state.add_css_class("actions-tab-run-state");
    row.append(&state);

    if run.is_running {
        let kill_button = gtk::Button::with_label("Kill");
        kill_button.add_css_class("app-flat-button");
        kill_button.add_css_class("actions-tab-mini-button");
        let run_id = run.id;
        kill_button.connect_clicked(move |_| match action_runner().kill(run_id) {
            Ok(()) => status_label.set_text("Stopping command..."),
            Err(err) => status_label.set_text(&err),
        });
        row.append(&kill_button);
    }

    let command = gtk::Label::new(Some(&run.command));
    command.set_xalign(0.0);
    command.set_wrap(true);
    command.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    command.add_css_class("actions-tab-command-text");

    let output = gtk::Label::new(Some(if run.output.trim().is_empty() {
        "(no output yet)"
    } else {
        &run.output
    }));
    output.set_xalign(0.0);
    output.set_yalign(0.0);
    output.set_selectable(true);
    output.set_wrap(false);
    output.add_css_class("actions-tab-output");

    card.append(&row);
    card.append(&command);
    card.append(&output);
    card
}

fn empty_label(text: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    label.set_xalign(0.0);
    label.add_css_class("dim-label");
    label.add_css_class("actions-tab-empty");
    label
}

fn current_workspace_path(active_workspace_path: &Rc<RefCell<Option<String>>>) -> Option<String> {
    active_workspace_path
        .borrow()
        .clone()
        .and_then(|path| canonical_workspace_path(&path))
}

fn workspace_display_name(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.trim().is_empty())
        .map(|name| name.to_string())
        .unwrap_or_else(|| path.to_string())
}

fn signature(
    workspace: Option<&str>,
    saved_actions: &[SavedWorkspaceAction],
    runs: &[ActionRunSnapshot],
) -> String {
    let saved = saved_actions
        .iter()
        .map(|item| {
            format!(
                "{}:{}:{}",
                item.id,
                item.title.as_deref().unwrap_or(""),
                item.command
            )
        })
        .collect::<Vec<_>>()
        .join("|");
    let runs = runs
        .iter()
        .map(|item| {
            format!(
                "{}:{}:{}:{}",
                item.id,
                item.is_running,
                item.status_text,
                item.output.len()
            )
        })
        .collect::<Vec<_>>()
        .join("|");
    format!("{};{};{}", workspace.unwrap_or(""), saved, runs)
}
