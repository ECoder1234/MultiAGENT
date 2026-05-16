use adw::prelude::*;
use std::process::Command;

fn command_output(args: &[&str]) -> Option<String> {
    let output = Command::new("copilot").args(args).output().ok()?;
    let text = if output.stdout.is_empty() {
        String::from_utf8_lossy(&output.stderr).to_string()
    } else {
        String::from_utf8_lossy(&output.stdout).to_string()
    };
    Some(text.trim().to_string())
}

pub(crate) fn build_settings_page() -> gtk::Box {
    let page = gtk::Box::new(gtk::Orientation::Vertical, 12);
    page.set_hexpand(true);
    page.set_vexpand(true);

    let section = gtk::Box::new(gtk::Orientation::Vertical, 8);
    section.add_css_class("profile-settings-section");

    let title = gtk::Label::new(Some("GitHub Copilot CLI"));
    title.set_xalign(0.0);
    title.add_css_class("profile-settings-title");

    let installed = command_output(&["--version"]);
    let status = gtk::Label::new(Some(match installed.as_deref() {
        Some(version) if !version.is_empty() => version,
        _ => "copilot command was not found in PATH",
    }));
    status.set_xalign(0.0);
    status.set_wrap(true);
    status.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    status.add_css_class("profile-muted-label");

    let hint = gtk::Label::new(Some(
        "This app can detect and prepare Copilot CLI. Full in-app Copilot chat needs an ACP adapter; for now use copilot login, copilot, or copilot -p from a terminal.",
    ));
    hint.set_xalign(0.0);
    hint.set_wrap(true);
    hint.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    hint.add_css_class("profile-muted-label");

    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let login_button = gtk::Button::with_label("Run copilot login");
    let docs_button = gtk::Button::with_label("Open docs");
    actions.append(&login_button);
    actions.append(&docs_button);

    {
        let status = status.clone();
        login_button.connect_clicked(move |_| {
            let _ = Command::new("sh")
                .arg("-lc")
                .arg("x-terminal-emulator -e 'copilot login; read -p \"Press Enter to close\"' || gnome-terminal -- bash -lc 'copilot login; read -p \"Press Enter to close\"' || copilot login")
                .spawn();
            status.set_text("Started copilot login in a terminal when available.");
        });
    }

    docs_button.connect_clicked(move |_| {
        let _ = gtk::gio::AppInfo::launch_default_for_uri(
            "https://docs.github.com/copilot/how-tos/copilot-cli",
            None::<&gtk::gio::AppLaunchContext>,
        );
    });

    section.append(&title);
    section.append(&status);
    section.append(&hint);
    section.append(&actions);
    page.append(&section);
    page
}
