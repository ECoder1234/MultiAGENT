use adw::prelude::*;
use gtk::gio;
use multiagent::chrome_bridge;
use multiagent_gtk::{APP_ICON, actions, app_id, app_name, ui};

fn main() {
    let args = std::env::args().collect::<Vec<_>>();
    if args.iter().any(|arg| arg == "--chrome-native-host") {
        if let Err(err) = chrome_bridge::run_native_host_stdio() {
            eprintln!("chrome native host failed: {err}");
            std::process::exit(1);
        }
        return;
    }
    if args.iter().any(|arg| arg == "--install-chrome-bridge") {
        match chrome_bridge::enable_linux_bridge() {
            Ok(status) => {
                println!(
                    "Chrome bridge updated. Manifest ready: {}. Host: {}",
                    status.native_manifest_live,
                    status
                        .host_candidate
                        .as_ref()
                        .map(|path| path.display().to_string())
                        .unwrap_or_else(|| "not found".to_string())
                );
            }
            Err(err) => {
                eprintln!("Could not enable Chrome bridge: {err}");
                std::process::exit(1);
            }
        }
        return;
    }

    gtk::glib::set_program_name(Some(app_id()));
    gtk::glib::set_application_name(app_name());
    let app_args = args
        .into_iter()
        .filter(|arg| arg != "--opencode" && arg != "--codex" && arg != "--install-chrome-bridge")
        .collect::<Vec<_>>();
    let app = adw::Application::builder().application_id(app_id()).build();

    app.connect_startup(|_| {
        sourceview5::init();

        let resources_bytes = include_bytes!("../../../resources.gresource");
        let resource_data = gtk::glib::Bytes::from_static(resources_bytes);
        let resource = gio::Resource::from_data(&resource_data).expect("Failed to load resources");
        gio::resources_register(&resource);

        if let Some(display) = gtk::gdk::Display::default() {
            let icon_theme = gtk::IconTheme::for_display(&display);
            icon_theme.add_resource_path("/com/multiagent/icons");
        }
        gtk::Window::set_default_icon_name(APP_ICON);

        ui::install_css();
    });

    app.connect_activate(ui::build_ui);
    app.connect_shutdown(|_| {
        actions::shutdown_all_running_actions();
    });

    app.run_with_args(&app_args);
}
