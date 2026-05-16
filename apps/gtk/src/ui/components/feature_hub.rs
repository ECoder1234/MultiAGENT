use adw::prelude::*;

#[derive(Clone, Copy)]
enum FeatureState {
    Ready,
    Partial,
    Upstream,
}

impl FeatureState {
    fn label(self) -> &'static str {
        match self {
            FeatureState::Ready => "Ready",
            FeatureState::Partial => "Partial",
            FeatureState::Upstream => "Needs native support",
        }
    }

    fn css_class(self) -> &'static str {
        match self {
            FeatureState::Ready => "feature-status-ready",
            FeatureState::Partial => "feature-status-partial",
            FeatureState::Upstream => "feature-status-upstream",
        }
    }
}

fn feature_row(
    icon_name: &str,
    title: &str,
    body: &str,
    state: FeatureState,
    location: &str,
) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    row.add_css_class("feature-row");
    row.set_hexpand(true);
    row.set_halign(gtk::Align::Fill);

    let icon = gtk::Image::from_icon_name(icon_name);
    icon.set_pixel_size(16);
    icon.add_css_class("feature-row-icon");

    let text = gtk::Box::new(gtk::Orientation::Vertical, 4);
    text.set_hexpand(true);
    text.set_halign(gtk::Align::Fill);

    let top = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    top.set_hexpand(true);
    top.set_halign(gtk::Align::Fill);

    let title_label = gtk::Label::new(Some(title));
    title_label.set_xalign(0.0);
    title_label.set_hexpand(true);
    title_label.add_css_class("feature-row-title");
    top.append(&title_label);

    let badge = gtk::Label::new(Some(state.label()));
    badge.add_css_class("feature-status");
    badge.add_css_class(state.css_class());
    top.append(&badge);

    let body_label = gtk::Label::new(Some(body));
    body_label.set_xalign(0.0);
    body_label.set_wrap(true);
    body_label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    body_label.add_css_class("feature-row-body");

    let location_label = gtk::Label::new(Some(location));
    location_label.set_xalign(0.0);
    location_label.set_wrap(true);
    location_label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    location_label.add_css_class("feature-row-location");

    text.append(&top);
    text.append(&body_label);
    text.append(&location_label);

    row.append(&icon);
    row.append(&text);
    row
}

pub(crate) fn build_settings_page() -> gtk::ScrolledWindow {
    let page = gtk::Box::new(gtk::Orientation::Vertical, 12);
    page.set_hexpand(true);
    page.set_vexpand(true);

    let intro = gtk::Box::new(gtk::Orientation::Vertical, 8);
    intro.add_css_class("profile-settings-section");
    let title = gtk::Label::new(Some("Codex App Features"));
    title.set_xalign(0.0);
    title.add_css_class("profile-settings-title");
    let body = gtk::Label::new(Some(
        "This is the parity map for the desktop app: what is already usable, what is partially wired, and what depends on upstream Codex or platform-native support.",
    ));
    body.set_xalign(0.0);
    body.set_wrap(true);
    body.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    body.add_css_class("profile-muted-label");
    intro.append(&title);
    intro.append(&body);
    page.append(&intro);

    let list = gtk::Box::new(gtk::Orientation::Vertical, 8);
    list.add_css_class("profile-settings-section");
    list.append(&feature_row(
        "text-editor-symbolic",
        "Focused Threads",
        "Open each project thread as the full editor page.",
        FeatureState::Ready,
        "Select a thread, create a new one, or drag a thread into the workspace to focus it.",
    ));
    list.append(&feature_row(
        "git-symbolic",
        "Worktrees",
        "Keep parallel code changes isolated with built-in Git worktree support.",
        FeatureState::Ready,
        "Use the branch/worktree control in the composer, then merge from the worktree banner.",
    ));
    list.append(&feature_row(
        "waves-and-screen-symbolic",
        "Remote Connections",
        "Start, steer, approve, and review work from a connected host.",
        FeatureState::Ready,
        "Settings -> Remote supports Discord DMs and Telegram bot chats.",
    ));
    list.append(&feature_row(
        "screen-symbolic",
        "Computer Use",
        "Let Codex operate GUI apps, browser flows, and native app testing.",
        FeatureState::Upstream,
        "Linux app support needs a local computer-use bridge. macOS app control is not available on this Ubuntu build.",
    ));
    list.append(&feature_row(
        "commit-symbolic",
        "Review And Ship Changes",
        "Inspect diffs, stage files, commit, and push.",
        FeatureState::Ready,
        "Use the Review tab and file preview diff views.",
    ));
    list.append(&feature_row(
        "terminal-symbolic",
        "Terminal And Actions",
        "Run commands in each thread and launch repeatable project actions.",
        FeatureState::Ready,
        "Thread command events render in chat; repeatable commands live in the Actions tab and top-right actions menu.",
    ));
    list.append(&feature_row(
        "web-browser-symbolic",
        "In-App Browser",
        "Open local files, localhost apps, and web URLs without leaving the workspace.",
        FeatureState::Ready,
        "Use the Browser tab. Settings -> Browser still manages the Chrome bridge.",
    ));
    list.append(&feature_row(
        "web-browser-symbolic",
        "Chrome Extension",
        "Use Chrome for signed-in browser tasks while managing website approvals.",
        FeatureState::Partial,
        "Settings -> Browser enables Codex plugin config and checks the native host manifest.",
    ));
    list.append(&feature_row(
        "image-x-generic-symbolic",
        "Image Generation",
        "Generate or edit images in a thread while working on code and assets.",
        FeatureState::Partial,
        "Image attachments and image events are supported. Dedicated generate/edit controls still need runtime support.",
    ));
    list.append(&feature_row(
        "alarm-symbolic",
        "Automations",
        "Schedule recurring tasks, or wake up the same thread for ongoing checks.",
        FeatureState::Partial,
        "Actions can be saved and reused. Scheduled thread wakeups still need a scheduler service.",
    ));
    list.append(&feature_row(
        "3d-box-symbolic",
        "Skills",
        "Reuse instructions and workflows across the app, CLI, and IDE Extension.",
        FeatureState::Ready,
        "Use Settings -> Skills & MCP.",
    ));
    list.append(&feature_row(
        "text-x-generic-symbolic",
        "Sidebar And Artifacts",
        "Follow plans, sources, task summaries, and generated file previews.",
        FeatureState::Ready,
        "Use the sidebar, chat activity cards, file previews, and restore preview.",
    ));
    list.append(&feature_row(
        "3d-box-symbolic",
        "Plugins",
        "Connect apps, skills, and MCP servers to extend what Codex can do.",
        FeatureState::Ready,
        "Use Settings -> Skills & MCP and Settings -> Browser.",
    ));
    list.append(&feature_row(
        "applications-engineering-symbolic",
        "IDE Extension Sync",
        "Share Auto Context and active threads across app and IDE sessions.",
        FeatureState::Upstream,
        "The app reads shared Codex config; active-thread sync needs an IDE bridge protocol.",
    ));
    page.append(&list);

    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .min_content_height(220)
        .propagate_natural_height(false)
        .child(&page)
        .build();
    scroll.set_has_frame(false);
    scroll.set_hexpand(true);
    scroll.set_vexpand(true);
    scroll
}
