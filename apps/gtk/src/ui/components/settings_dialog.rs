use crate::services::app::CodexProfileManager;
use crate::services::app::chat::AppDb;
use adw::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

use super::chat;
use super::chrome_settings;
use super::copilot_settings;
use super::feature_hub;
use super::remote_settings;
use super::settings;
use super::skills_mcp_settings;
use super::style_picker;
use crate::ui::widget_tree;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsPage {
    Codex,
    OpenCode,
    Features,
    Copilot,
    Browser,
    VoiceInput,
    SkillsMcp,
    Remote,
    Interface,
    About,
}

impl SettingsPage {
    fn title(self) -> &'static str {
        match self {
            SettingsPage::Codex => "Codex",
            SettingsPage::OpenCode => "OpenCode",
            SettingsPage::Features => "Features",
            SettingsPage::Copilot => "Copilot",
            SettingsPage::Browser => "Browser",
            SettingsPage::VoiceInput => "Voice Input",
            SettingsPage::SkillsMcp => "Skills & MCP",
            SettingsPage::Remote => "Remote",
            SettingsPage::Interface => "Interface",
            SettingsPage::About => "About",
        }
    }

    fn stack_name(self) -> &'static str {
        match self {
            SettingsPage::Codex => "codex",
            SettingsPage::OpenCode => "opencode",
            SettingsPage::Features => "features",
            SettingsPage::Copilot => "copilot",
            SettingsPage::Browser => "browser",
            SettingsPage::VoiceInput => "voice-input",
            SettingsPage::SkillsMcp => "skills-mcp",
            SettingsPage::Remote => "remote",
            SettingsPage::Interface => "interface",
            SettingsPage::About => "about",
        }
    }

    fn list_index(self) -> i32 {
        match self {
            SettingsPage::Codex => 0,
            SettingsPage::OpenCode => 1,
            SettingsPage::Features => 2,
            SettingsPage::Copilot => 3,
            SettingsPage::Browser => 4,
            SettingsPage::VoiceInput => 5,
            SettingsPage::SkillsMcp => 6,
            SettingsPage::Remote => 7,
            SettingsPage::Interface => 8,
            SettingsPage::About => 9,
        }
    }

    fn icon_name(self) -> &'static str {
        match self {
            SettingsPage::Codex => "provider-codex",
            SettingsPage::OpenCode => "provider-opencode",
            SettingsPage::Features => "view-list-symbolic",
            SettingsPage::Copilot => "github-symbolic",
            SettingsPage::Browser => "web-browser-symbolic",
            SettingsPage::VoiceInput => "mic-symbolic",
            SettingsPage::SkillsMcp => "3d-box-symbolic",
            SettingsPage::Remote => "waves-and-screen-symbolic",
            SettingsPage::Interface => "cogged-wheel-big-symbolic",
            SettingsPage::About => "globe-symbolic",
        }
    }
}

thread_local! {
    static SETTINGS_DIALOG_WINDOW: RefCell<Option<gtk::glib::WeakRef<gtk::Window>>> =
        RefCell::new(None);
}

fn apply_initial_page_selection(dialog: &gtk::Window, page: SettingsPage) {
    let root_widget: gtk::Widget = dialog.clone().upcast();
    let Some(nav_list) = widget_tree::find_widget_by_name(&root_widget, "settings-nav-list")
        .and_then(|widget| widget.downcast::<gtk::ListBox>().ok())
    else {
        return;
    };
    if let Some(row) = nav_list.row_at_index(page.list_index()) {
        nav_list.select_row(Some(&row));
    }
}

fn nav_row(icon_name: &str, label: &str) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    row.add_css_class("settings-nav-row");
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    content.set_margin_start(12);
    content.set_margin_end(12);
    content.set_margin_top(9);
    content.set_margin_bottom(9);
    let icon = gtk::Image::from_icon_name(icon_name);
    icon.set_pixel_size(15);
    icon.add_css_class("settings-nav-icon");
    let text = gtk::Label::new(Some(label));
    text.set_xalign(0.0);
    text.add_css_class("settings-nav-label");
    content.append(&icon);
    content.append(&text);
    row.set_child(Some(&content));
    row
}

fn about_link_button(
    icon_name: &str,
    title: &str,
    subtitle: &str,
    uri: &'static str,
) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class("about-link-button");
    button.set_halign(gtk::Align::Fill);
    button.set_hexpand(true);

    let content = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    content.set_halign(gtk::Align::Start);
    content.set_hexpand(true);

    let icon = gtk::Image::from_icon_name(icon_name);
    icon.set_pixel_size(16);
    icon.add_css_class("about-link-icon");

    let text_box = gtk::Box::new(gtk::Orientation::Vertical, 2);
    text_box.set_hexpand(true);

    let title_label = gtk::Label::new(Some(title));
    title_label.set_xalign(0.0);
    title_label.add_css_class("about-link-title");

    let subtitle_label = gtk::Label::new(Some(subtitle));
    subtitle_label.set_xalign(0.0);
    subtitle_label.add_css_class("about-link-subtitle");
    subtitle_label.set_wrap(true);
    subtitle_label.set_wrap_mode(gtk::pango::WrapMode::WordChar);

    text_box.append(&title_label);
    text_box.append(&subtitle_label);
    content.append(&icon);
    content.append(&text_box);
    button.set_child(Some(&content));

    button.connect_clicked(move |_| {
        let _ = gtk::gio::AppInfo::launch_default_for_uri(uri, None::<&gtk::gio::AppLaunchContext>);
    });

    button
}

fn shortcut_row(keys: &str, action: &str) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    row.add_css_class("shortcut-row");

    let key_label = gtk::Label::new(Some(keys));
    key_label.add_css_class("shortcut-key");
    key_label.set_xalign(0.0);
    key_label.set_width_chars(14);
    row.append(&key_label);

    let action_label = gtk::Label::new(Some(action));
    action_label.add_css_class("shortcut-action");
    action_label.set_xalign(0.0);
    action_label.set_hexpand(true);
    row.append(&action_label);

    row
}

fn build_interface_page(db: Rc<AppDb>) -> gtk::Box {
    let page = gtk::Box::new(gtk::Orientation::Vertical, 12);
    page.set_hexpand(true);
    page.set_vexpand(true);

    let identity = gtk::Box::new(gtk::Orientation::Vertical, 8);
    identity.add_css_class("profile-settings-section");
    let title = gtk::Label::new(Some("MultiAGENT"));
    title.add_css_class("profile-section-title");
    title.set_xalign(0.0);
    let credit = gtk::Label::new(Some(
        "MultiAGENT is built on top of enzim-coder by enz1m, which serves as the source first iteration and main backbone of this project.",
    ));
    credit.add_css_class("about-subtitle");
    credit.set_xalign(0.0);
    credit.set_wrap(true);
    credit.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    identity.append(&title);
    identity.append(&credit);

    let appearance = gtk::Box::new(gtk::Orientation::Vertical, 8);
    appearance.add_css_class("profile-settings-section");
    let appearance_title = gtk::Label::new(Some("Appearance"));
    appearance_title.add_css_class("profile-section-title");
    appearance_title.set_xalign(0.0);
    appearance.append(&appearance_title);

    let theme_row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    let theme_text = gtk::Box::new(gtk::Orientation::Vertical, 2);
    theme_text.set_hexpand(true);
    let theme_label = gtk::Label::new(Some("Dark theme"));
    theme_label.add_css_class("settings-row-title");
    theme_label.set_xalign(0.0);
    let theme_hint = gtk::Label::new(Some(
        "Default glassy dark interface with blue-purple accents.",
    ));
    theme_hint.add_css_class("settings-row-subtitle");
    theme_hint.set_xalign(0.0);
    theme_hint.set_wrap(true);
    theme_text.append(&theme_label);
    theme_text.append(&theme_hint);
    let theme_switch = gtk::Switch::new();
    theme_switch.set_active(style_picker::dark_theme_enabled(&db));
    theme_row.append(&theme_text);
    theme_row.append(&theme_switch);
    appearance.append(&theme_row);
    {
        let db = db.clone();
        theme_switch.connect_active_notify(move |switch| {
            style_picker::set_dark_theme_enabled(&db, switch.is_active());
        });
    }

    let shortcuts = gtk::Box::new(gtk::Orientation::Vertical, 8);
    shortcuts.add_css_class("profile-settings-section");
    let shortcuts_title = gtk::Label::new(Some("Keyboard Shortcuts"));
    shortcuts_title.add_css_class("profile-section-title");
    shortcuts_title.set_xalign(0.0);
    shortcuts.append(&shortcuts_title);
    shortcuts.append(&shortcut_row("Ctrl+K", "Open command palette"));
    shortcuts.append(&shortcut_row(
        "Enter",
        "Send prompt or push when Review is focused",
    ));
    shortcuts.append(&shortcut_row(
        "@",
        "Open file mention picker in the composer",
    ));
    shortcuts.append(&shortcut_row("Esc", "Close popovers and transient panels"));

    page.append(&identity);
    page.append(&appearance);
    page.append(&shortcuts);
    page
}

fn build_about_page() -> gtk::Box {
    let page = gtk::Box::new(gtk::Orientation::Vertical, 12);
    page.set_hexpand(true);
    page.set_vexpand(true);

    let hero = gtk::Box::new(gtk::Orientation::Horizontal, 14);
    hero.add_css_class("profile-settings-section");
    hero.add_css_class("about-hero-card");

    let app_icon = gtk::Image::from_icon_name("dev.multiagent.multiagent");
    app_icon.set_pixel_size(56);
    app_icon.add_css_class("about-app-icon");

    let hero_text = gtk::Box::new(gtk::Orientation::Vertical, 6);
    hero_text.set_hexpand(true);

    let title = gtk::Label::new(Some(crate::app_name()));
    title.add_css_class("profile-section-title");
    title.add_css_class("about-title");
    title.set_xalign(0.0);

    let subtitle = gtk::Label::new(Some(
        "Local-first multi-agent coding workspace with threads, Git, files, diffs, and local agent sessions.",
    ));
    subtitle.set_xalign(0.0);
    subtitle.set_wrap(true);
    subtitle.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    subtitle.add_css_class("about-subtitle");

    let meta = gtk::Box::new(gtk::Orientation::Vertical, 4);

    let version = gtk::Label::new(Some(&format!("Version {}", env!("CARGO_PKG_VERSION"))));
    version.set_xalign(0.0);
    version.set_selectable(true);
    version.add_css_class("about-meta-line");

    let app_id = gtk::Label::new(Some(&format!("App ID: {}", crate::app_id())));
    app_id.set_xalign(0.0);
    app_id.set_selectable(true);
    app_id.add_css_class("about-meta-line");

    meta.append(&version);
    meta.append(&app_id);

    let credit = gtk::Label::new(Some(
        "MultiAGENT is built on top of enzim-coder by enz1m, which serves as the source first iteration and main backbone of this project.",
    ));
    credit.set_xalign(0.0);
    credit.set_wrap(true);
    credit.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    credit.add_css_class("about-credit");

    hero_text.append(&title);
    hero_text.append(&subtitle);
    hero_text.append(&meta);
    hero_text.append(&credit);

    hero.append(&app_icon);
    hero.append(&hero_text);

    let links = gtk::Box::new(gtk::Orientation::Vertical, 8);
    links.add_css_class("profile-settings-section");
    links.add_css_class("about-links-card");

    let links_title = gtk::Label::new(Some("Links"));
    links_title.set_xalign(0.0);
    links_title.add_css_class("profile-section-title");

    let website_button = about_link_button(
        "globe-symbolic",
        "Website",
        "OpenAI Codex Chrome docs",
        "https://developers.openai.com/codex/app/chrome-extension",
    );
    let github_button = about_link_button(
        "github-symbolic",
        "Source Backbone",
        "enzim-coder by enz1m",
        "https://github.com/enz1m/enzim-coder",
    );

    links.append(&links_title);
    links.append(&website_button);
    links.append(&github_button);

    page.append(&hero);
    page.append(&links);
    page
}

pub fn show(
    parent: Option<&gtk::Window>,
    db: Rc<AppDb>,
    manager: Rc<CodexProfileManager>,
    initial_page: SettingsPage,
) {
    if let Some(existing) =
        SETTINGS_DIALOG_WINDOW.with(|slot| slot.borrow().as_ref().and_then(|weak| weak.upgrade()))
    {
        if let Some(parent) = parent {
            existing.set_transient_for(Some(parent));
        }
        apply_initial_page_selection(&existing, initial_page);
        existing.present();
        return;
    }

    let dialog = gtk::Window::builder()
        .title("Settings")
        .default_width(760)
        .default_height(640)
        .modal(true)
        .build();
    dialog.add_css_class("settings-window");
    if let Some(parent) = parent {
        dialog.set_transient_for(Some(parent));
    }

    let root = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    root.add_css_class("settings-root");

    let nav_shell = gtk::Box::new(gtk::Orientation::Vertical, 0);
    nav_shell.set_width_request(196);
    nav_shell.add_css_class("settings-nav-shell");

    let nav_list = gtk::ListBox::new();
    nav_list.set_widget_name("settings-nav-list");
    nav_list.set_selection_mode(gtk::SelectionMode::Single);
    nav_list.set_activate_on_single_click(true);
    nav_list.add_css_class("navigation-sidebar");
    nav_list.add_css_class("settings-nav-list");
    nav_list.set_margin_top(12);
    nav_list.append(&nav_row("provider-codex", "Codex"));
    nav_list.append(&nav_row("provider-opencode", "OpenCode"));
    nav_list.append(&nav_row("view-list-symbolic", "Features"));
    nav_list.append(&nav_row("github-symbolic", "Copilot"));
    nav_list.append(&nav_row("web-browser-symbolic", "Browser"));
    nav_list.append(&nav_row("mic-symbolic", "Voice input"));
    nav_list.append(&nav_row("3d-box-symbolic", "Skills & MCP"));
    nav_list.append(&nav_row("waves-and-screen-symbolic", "Remote"));
    nav_list.append(&nav_row("cogged-wheel-big-symbolic", "Interface"));
    nav_list.append(&nav_row("globe-symbolic", "About"));
    nav_shell.append(&nav_list);

    let content_shell = gtk::Box::new(gtk::Orientation::Vertical, 0);
    content_shell.set_hexpand(true);
    content_shell.set_vexpand(true);
    content_shell.add_css_class("settings-content-shell");

    let header = gtk::CenterBox::new();
    header.add_css_class("settings-page-header");
    header.set_margin_start(14);
    header.set_margin_end(14);
    header.set_margin_top(14);
    header.set_margin_bottom(6);
    let title_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let page_icon = gtk::Image::from_icon_name(initial_page.icon_name());
    page_icon.set_pixel_size(16);
    page_icon.add_css_class("settings-page-title-icon");
    let page_title = gtk::Label::new(Some(initial_page.title()));
    page_title.add_css_class("settings-page-title");
    page_title.set_xalign(0.0);
    page_title.set_valign(gtk::Align::Center);
    title_row.append(&page_icon);
    title_row.append(&page_title);
    header.set_start_widget(Some(&title_row));
    let profiles_create_button = gtk::Button::with_label("Create New");
    profiles_create_button.add_css_class("profile-create-button");
    profiles_create_button.set_visible(initial_page == SettingsPage::Codex);
    header.set_end_widget(Some(&profiles_create_button));
    content_shell.append(&header);

    let stack = gtk::Stack::new();
    stack.add_css_class("settings-page-stack");
    stack.set_margin_start(14);
    stack.set_margin_end(14);
    stack.set_margin_bottom(14);
    stack.set_hexpand(true);
    stack.set_vexpand(true);
    stack.set_transition_type(gtk::StackTransitionType::Crossfade);
    stack.set_transition_duration(140);

    let (codex_page, profiles_create_action) =
        settings::codex::build_settings_page(&dialog, db.clone(), manager.clone());
    let (opencode_page, _opencode_create_action) =
        settings::opencode::build_settings_page(&dialog, db.clone(), manager.clone());
    let features_page = feature_hub::build_settings_page();
    let copilot_page = copilot_settings::build_settings_page();
    let browser_page = chrome_settings::build_settings_page();
    let voice_page = chat::composer::voice::build_settings_page(&dialog, db.clone(), None, false);
    let skills_mcp_page = skills_mcp_settings::build_settings_page(&dialog, db.clone(), manager);
    let remote_page = remote_settings::build_settings_page(&dialog, db.clone());
    let interface_page = build_interface_page(db.clone());
    let about_page = build_about_page();
    let skills_mcp_scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .min_content_height(220)
        .max_content_height(540)
        .propagate_natural_height(false)
        .build();
    skills_mcp_scroll.set_has_frame(false);
    skills_mcp_scroll.set_hexpand(true);
    skills_mcp_scroll.set_vexpand(true);
    skills_mcp_scroll.set_child(Some(&skills_mcp_page));
    stack.add_named(&codex_page, Some(SettingsPage::Codex.stack_name()));
    stack.add_named(&opencode_page, Some(SettingsPage::OpenCode.stack_name()));
    stack.add_named(&features_page, Some(SettingsPage::Features.stack_name()));
    stack.add_named(&copilot_page, Some(SettingsPage::Copilot.stack_name()));
    stack.add_named(&browser_page, Some(SettingsPage::Browser.stack_name()));
    stack.add_named(&voice_page, Some(SettingsPage::VoiceInput.stack_name()));
    stack.add_named(
        &skills_mcp_scroll,
        Some(SettingsPage::SkillsMcp.stack_name()),
    );
    stack.add_named(&remote_page, Some(SettingsPage::Remote.stack_name()));
    stack.add_named(&interface_page, Some(SettingsPage::Interface.stack_name()));
    stack.add_named(&about_page, Some(SettingsPage::About.stack_name()));
    stack.set_visible_child_name(initial_page.stack_name());
    content_shell.append(&stack);

    {
        let stack = stack.clone();
        let page_icon = page_icon.clone();
        let page_title = page_title.clone();
        let profiles_create_button = profiles_create_button.clone();
        nav_list.connect_row_selected(move |_, row| {
            let Some(row) = row else {
                return;
            };
            match row.index() {
                0 => {
                    stack.set_visible_child_name(SettingsPage::Codex.stack_name());
                    page_icon.set_icon_name(Some(SettingsPage::Codex.icon_name()));
                    page_title.set_text(SettingsPage::Codex.title());
                    profiles_create_button.set_visible(true);
                }
                1 => {
                    stack.set_visible_child_name(SettingsPage::OpenCode.stack_name());
                    page_icon.set_icon_name(Some(SettingsPage::OpenCode.icon_name()));
                    page_title.set_text(SettingsPage::OpenCode.title());
                    profiles_create_button.set_visible(false);
                }
                2 => {
                    stack.set_visible_child_name(SettingsPage::Features.stack_name());
                    page_icon.set_icon_name(Some(SettingsPage::Features.icon_name()));
                    page_title.set_text(SettingsPage::Features.title());
                    profiles_create_button.set_visible(false);
                }
                3 => {
                    stack.set_visible_child_name(SettingsPage::Copilot.stack_name());
                    page_icon.set_icon_name(Some(SettingsPage::Copilot.icon_name()));
                    page_title.set_text(SettingsPage::Copilot.title());
                    profiles_create_button.set_visible(false);
                }
                4 => {
                    stack.set_visible_child_name(SettingsPage::Browser.stack_name());
                    page_icon.set_icon_name(Some(SettingsPage::Browser.icon_name()));
                    page_title.set_text(SettingsPage::Browser.title());
                    profiles_create_button.set_visible(false);
                }
                5 => {
                    stack.set_visible_child_name(SettingsPage::VoiceInput.stack_name());
                    page_icon.set_icon_name(Some(SettingsPage::VoiceInput.icon_name()));
                    page_title.set_text(SettingsPage::VoiceInput.title());
                    profiles_create_button.set_visible(false);
                }
                6 => {
                    stack.set_visible_child_name(SettingsPage::SkillsMcp.stack_name());
                    page_icon.set_icon_name(Some(SettingsPage::SkillsMcp.icon_name()));
                    page_title.set_text(SettingsPage::SkillsMcp.title());
                    profiles_create_button.set_visible(false);
                }
                7 => {
                    stack.set_visible_child_name(SettingsPage::Remote.stack_name());
                    page_icon.set_icon_name(Some(SettingsPage::Remote.icon_name()));
                    page_title.set_text(SettingsPage::Remote.title());
                    profiles_create_button.set_visible(false);
                }
                8 => {
                    stack.set_visible_child_name(SettingsPage::Interface.stack_name());
                    page_icon.set_icon_name(Some(SettingsPage::Interface.icon_name()));
                    page_title.set_text(SettingsPage::Interface.title());
                    profiles_create_button.set_visible(false);
                }
                9 => {
                    stack.set_visible_child_name(SettingsPage::About.stack_name());
                    page_icon.set_icon_name(Some(SettingsPage::About.icon_name()));
                    page_title.set_text(SettingsPage::About.title());
                    profiles_create_button.set_visible(false);
                }
                _ => {}
            }
        });
    }

    {
        let profiles_create_action = profiles_create_action.clone();
        profiles_create_button.connect_clicked(move |_| {
            profiles_create_action.emit_clicked();
        });
    }

    if let Some(row) = nav_list.row_at_index(initial_page.list_index()) {
        nav_list.select_row(Some(&row));
    }

    root.append(&nav_shell);
    root.append(&content_shell);

    dialog.set_child(Some(&root));
    dialog.connect_close_request(move |win| {
        win.set_visible(false);
        gtk::glib::Propagation::Stop
    });
    {
        let weak = dialog.downgrade();
        SETTINGS_DIALOG_WINDOW.with(|slot| {
            slot.borrow_mut().replace(weak);
        });
    }
    dialog.present();
}
