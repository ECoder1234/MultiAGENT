use adw::prelude::*;
use multiagent::chrome_bridge;

fn status_line(title: &str, value: &str, ok: bool) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    row.add_css_class("profile-settings-row");
    row.set_hexpand(true);

    let icon = gtk::Image::from_icon_name(if ok {
        "emblem-ok-symbolic"
    } else {
        "dialog-warning-symbolic"
    });
    icon.set_pixel_size(16);

    let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
    text.set_hexpand(true);
    let title_label = gtk::Label::new(Some(title));
    title_label.set_xalign(0.0);
    title_label.add_css_class("profile-section-title");
    let value_label = gtk::Label::new(Some(value));
    value_label.set_xalign(0.0);
    value_label.set_wrap(true);
    value_label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    value_label.add_css_class("profile-muted-label");
    text.append(&title_label);
    text.append(&value_label);

    row.append(&icon);
    row.append(&text);
    row
}

fn path_list(paths: &[std::path::PathBuf]) -> String {
    if paths.is_empty() {
        return "Not found".to_string();
    }
    paths
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

fn native_manifest_value(status: &chrome_bridge::ChromeBridgeStatus) -> String {
    if status.native_manifest_host_paths.is_empty() {
        return path_list(&status.native_manifest_paths);
    }
    format!(
        "{}\nTarget:\n{}",
        path_list(&status.native_manifest_paths),
        path_list(&status.native_manifest_host_paths)
    )
}

fn refresh_status(list: &gtk::Box, summary: &gtk::Label) {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }

    let status = chrome_bridge::status();
    let ready = status.extension_installed
        && status.native_manifest_live
        && status.codex_plugins_enabled
        && status.codex_plugins_installed
        && status.host_candidate.is_some();

    summary.set_text(if ready {
        "Chrome native messaging is wired. Restart Chrome or reload the extension, then start a new Codex thread."
    } else {
        "This page can install the Linux native-host wrapper that the Codex Chrome extension expects."
    });

    list.append(&status_line(
        "Official Chrome extension",
        &path_list(&status.extension_paths),
        status.extension_installed,
    ));
    list.append(&status_line(
        "Codex plugin config",
        &format!(
            "{}\nChrome and browser-use config: {}\nPlugin files:\n{}",
            status.codex_config_path.display(),
            if status.codex_plugins_enabled {
                "enabled"
            } else {
                "not enabled"
            },
            path_list(&status.codex_plugin_paths)
        ),
        status.codex_plugins_enabled && status.codex_plugins_installed,
    ));
    list.append(&status_line(
        "Native messaging manifest",
        &native_manifest_value(&status),
        status.native_manifest_live,
    ));
    list.append(&status_line(
        "Native host",
        status
            .host_candidate
            .as_ref()
            .map(|path| path.display().to_string())
            .as_deref()
            .unwrap_or(
                "Not installed yet. Click Enable Linux bridge to create the app native-host wrapper.",
            ),
        status.host_candidate.is_some(),
    ));
    if !status.notes.is_empty() {
        list.append(&status_line("Notes", &status.notes.join("\n"), ready));
    }
}

pub(crate) fn build_settings_page() -> gtk::Box {
    let page = gtk::Box::new(gtk::Orientation::Vertical, 12);
    page.set_hexpand(true);
    page.set_vexpand(true);

    let intro = gtk::Box::new(gtk::Orientation::Vertical, 8);
    intro.add_css_class("profile-settings-section");

    let title = gtk::Label::new(Some("Chrome Bridge"));
    title.set_xalign(0.0);
    title.add_css_class("profile-settings-title");

    let summary = gtk::Label::new(None);
    summary.set_xalign(0.0);
    summary.set_wrap(true);
    summary.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    summary.add_css_class("profile-muted-label");

    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let enable_button = gtk::Button::with_label("Enable Linux bridge");
    enable_button.add_css_class("suggested-action");
    let refresh_button = gtk::Button::with_label("Refresh");
    actions.append(&enable_button);
    actions.append(&refresh_button);

    intro.append(&title);
    intro.append(&summary);
    intro.append(&actions);

    let status_list = gtk::Box::new(gtk::Orientation::Vertical, 8);
    status_list.add_css_class("profile-settings-section");

    refresh_status(&status_list, &summary);

    {
        let status_list = status_list.clone();
        let summary = summary.clone();
        refresh_button.connect_clicked(move |_| {
            refresh_status(&status_list, &summary);
        });
    }

    {
        let status_list = status_list.clone();
        let summary = summary.clone();
        enable_button.connect_clicked(move |_| {
            match chrome_bridge::enable_linux_bridge() {
                Ok(_) => summary.set_text(
                    "Bridge settings were updated. Restart Chrome or reload the extension, then start a new Codex thread.",
                ),
                Err(err) => summary.set_text(&err),
            }
            refresh_status(&status_list, &summary);
        });
    }

    page.append(&intro);
    page.append(&status_list);
    page
}
