use crate::services::app::CodexProfileManager;
use crate::services::app::chat::AppDb;
use adw::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

use super::settings;
use super::settings_dialog;

struct PaletteCommand {
    icon_name: &'static str,
    title: &'static str,
    subtitle: &'static str,
    keywords: &'static str,
    action: Rc<dyn Fn()>,
}

fn active_parent_window() -> Option<gtk::Window> {
    gtk::Application::default()
        .active_window()
        .and_then(|window| window.downcast::<gtk::Window>().ok())
}

fn command_matches(command: &PaletteCommand, query: &str) -> bool {
    let query = query.trim().to_ascii_lowercase();
    if query.is_empty() {
        return true;
    }
    let haystack = format!(
        "{} {} {}",
        command.title, command.subtitle, command.keywords
    )
    .to_ascii_lowercase();
    query.split_whitespace().all(|part| haystack.contains(part))
}

fn build_command_row(command: Rc<PaletteCommand>, window: &gtk::Window) -> gtk::Button {
    let row = gtk::Button::new();
    row.set_has_frame(false);
    row.add_css_class("command-palette-row");
    row.set_halign(gtk::Align::Fill);
    row.set_hexpand(true);

    let content = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    content.set_hexpand(true);

    let icon = gtk::Image::from_icon_name(command.icon_name);
    icon.set_pixel_size(15);
    icon.add_css_class("command-palette-row-icon");
    content.append(&icon);

    let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
    text.set_hexpand(true);
    let title = gtk::Label::new(Some(command.title));
    title.add_css_class("command-palette-row-title");
    title.set_xalign(0.0);
    let subtitle = gtk::Label::new(Some(command.subtitle));
    subtitle.add_css_class("command-palette-row-subtitle");
    subtitle.set_xalign(0.0);
    subtitle.set_ellipsize(gtk::pango::EllipsizeMode::End);
    text.append(&title);
    text.append(&subtitle);
    content.append(&text);

    row.set_child(Some(&content));

    let window = window.clone();
    row.connect_clicked(move |_| {
        (command.action)();
        window.close();
    });

    row
}

fn render_commands(
    list: &gtk::Box,
    commands: Rc<Vec<Rc<PaletteCommand>>>,
    query: &str,
    window: &gtk::Window,
) {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }

    let mut count = 0usize;
    for command in commands
        .iter()
        .filter(|command| command_matches(command, query))
    {
        list.append(&build_command_row(command.clone(), window));
        count += 1;
    }

    if count == 0 {
        let empty = gtk::Label::new(Some("No matching commands"));
        empty.add_css_class("command-palette-empty");
        empty.set_xalign(0.0);
        list.append(&empty);
    }
}

fn show_palette(
    stack: &adw::ViewStack,
    db: Rc<AppDb>,
    manager: Rc<CodexProfileManager>,
    active_workspace_path: Rc<RefCell<Option<String>>>,
    browser_split_toggle: Option<gtk::ToggleButton>,
) {
    let window = gtk::Window::builder()
        .title("Command Palette")
        .default_width(640)
        .default_height(460)
        .modal(true)
        .destroy_with_parent(true)
        .build();
    window.add_css_class("command-palette-window");
    window.set_hide_on_close(true);
    if let Some(parent) = active_parent_window() {
        window.set_transient_for(Some(&parent));
    }

    let root = gtk::Box::new(gtk::Orientation::Vertical, 10);
    root.add_css_class("command-palette");
    root.set_margin_start(14);
    root.set_margin_end(14);
    root.set_margin_top(14);
    root.set_margin_bottom(14);

    let search = gtk::SearchEntry::new();
    search.add_css_class("command-palette-search");
    search.set_placeholder_text(Some("Search commands"));
    root.append(&search);

    let hint_row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    hint_row.add_css_class("command-palette-hint-row");
    let hint = gtk::Label::new(Some("Ctrl+K opens commands"));
    hint.add_css_class("command-palette-hint");
    hint.set_xalign(0.0);
    hint.set_hexpand(true);
    hint_row.append(&hint);
    let badge = gtk::Label::new(Some("MultiAGENT"));
    badge.add_css_class("command-palette-badge");
    hint_row.append(&badge);
    root.append(&hint_row);

    let list = gtk::Box::new(gtk::Orientation::Vertical, 4);
    list.add_css_class("command-palette-list");
    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .vexpand(true)
        .child(&list)
        .build();
    scroll.set_has_frame(false);
    root.append(&scroll);

    let stack_chat = stack.clone();
    let stack_review = stack.clone();
    let stack_actions = stack.clone();
    let db_settings = db.clone();
    let manager_settings = manager.clone();
    let db_browser = db.clone();
    let manager_browser = manager.clone();
    let db_remote = db.clone();
    let manager_remote = manager.clone();
    let active_workspace_for_copy = active_workspace_path.clone();
    let browser_toggle_for_action = browser_split_toggle.clone();

    let commands: Rc<Vec<Rc<PaletteCommand>>> = Rc::new(vec![
        Rc::new(PaletteCommand {
            icon_name: "chat-new-symbolic",
            title: "Open Chat",
            subtitle: "Focus the current agent conversation",
            keywords: "thread conversation agent output",
            action: Rc::new(move || stack_chat.set_visible_child_name("chat")),
        }),
        Rc::new(PaletteCommand {
            icon_name: "git-symbolic",
            title: "Open Review",
            subtitle: "Inspect changed files, diffs, staging, and commits",
            keywords: "git diff stage commit review",
            action: Rc::new(move || stack_review.set_visible_child_name("git")),
        }),
        Rc::new(PaletteCommand {
            icon_name: "terminal-symbolic",
            title: "Open Actions",
            subtitle: "Run saved project commands",
            keywords: "terminal command actions scripts",
            action: Rc::new(move || stack_actions.set_visible_child_name("actions")),
        }),
        Rc::new(PaletteCommand {
            icon_name: "web-browser-symbolic",
            title: "Toggle Browser Split",
            subtitle: "Open or close the embedded browser panel",
            keywords: "browser split preview localhost",
            action: Rc::new(move || {
                if let Some(toggle) = browser_toggle_for_action.as_ref() {
                    toggle.set_active(!toggle.is_active());
                }
            }),
        }),
        Rc::new(PaletteCommand {
            icon_name: "cogged-wheel-big-symbolic",
            title: "Open Settings",
            subtitle: "Runtime, browser, shortcuts, appearance, and credits",
            keywords: "settings preferences shortcuts appearance credits",
            action: Rc::new(move || {
                settings::show(
                    active_parent_window().as_ref(),
                    db_settings.clone(),
                    manager_settings.clone(),
                )
            }),
        }),
        Rc::new(PaletteCommand {
            icon_name: "web-browser-symbolic",
            title: "Browser Settings",
            subtitle: "Configure Chrome bridge and browser integration",
            keywords: "chrome browser bridge settings",
            action: Rc::new(move || {
                settings_dialog::show(
                    active_parent_window().as_ref(),
                    db_browser.clone(),
                    manager_browser.clone(),
                    settings_dialog::SettingsPage::Browser,
                );
            }),
        }),
        Rc::new(PaletteCommand {
            icon_name: "waves-and-screen-symbolic",
            title: "Remote Settings",
            subtitle: "Configure Discord or Telegram remote mode",
            keywords: "remote telegram discord bot",
            action: Rc::new(move || {
                settings_dialog::show(
                    active_parent_window().as_ref(),
                    db_remote.clone(),
                    manager_remote.clone(),
                    settings_dialog::SettingsPage::Remote,
                );
            }),
        }),
        Rc::new(PaletteCommand {
            icon_name: "edit-copy-symbolic",
            title: "Copy Workspace Path",
            subtitle: "Copy the active project path to the clipboard",
            keywords: "copy workspace project path",
            action: Rc::new(move || {
                let Some(path) = active_workspace_for_copy.borrow().clone() else {
                    return;
                };
                if let Some(display) = gtk::gdk::Display::default() {
                    display.clipboard().set_text(&path);
                }
            }),
        }),
    ]);

    render_commands(&list, commands.clone(), "", &window);
    {
        let list = list.clone();
        let commands = commands.clone();
        let window = window.clone();
        search.connect_search_changed(move |entry| {
            render_commands(&list, commands.clone(), entry.text().as_str(), &window);
        });
    }

    window.set_child(Some(&root));
    window.present();
    search.grab_focus();
}

pub fn install(
    root: &impl IsA<gtk::Widget>,
    stack: &adw::ViewStack,
    db: Rc<AppDb>,
    manager: Rc<CodexProfileManager>,
    active_workspace_path: Rc<RefCell<Option<String>>>,
    browser_split_toggle: Option<&gtk::ToggleButton>,
) {
    let key = gtk::EventControllerKey::new();
    key.set_propagation_phase(gtk::PropagationPhase::Capture);
    let stack = stack.clone();
    let browser_split_toggle = browser_split_toggle.cloned();
    key.connect_key_pressed(move |_, keyval, _, state| {
        let is_ctrl_k =
            keyval == gtk::gdk::Key::K && state.contains(gtk::gdk::ModifierType::CONTROL_MASK);
        if !is_ctrl_k {
            return gtk::glib::Propagation::Proceed;
        }
        show_palette(
            &stack,
            db.clone(),
            manager.clone(),
            active_workspace_path.clone(),
            browser_split_toggle.clone(),
        );
        gtk::glib::Propagation::Stop
    });
    root.add_controller(key);
}
