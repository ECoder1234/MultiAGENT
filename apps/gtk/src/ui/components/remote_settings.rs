use crate::services::app::chat::{AppDb, RemoteTelegramAccountRecord};
use crate::services::app::remote;
use gtk::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::TryRecvError;
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

fn account_summary(account: &RemoteTelegramAccountRecord) -> String {
    let provider = remote::remote_provider_label(&account.provider);
    let username = account
        .telegram_username
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .map(|value| format!("@{}", value.trim()))
        .unwrap_or_else(|| account.telegram_user_id.clone());
    format!(
        "Connected as {} over {}\nToken: {}",
        username,
        provider,
        remote::mask_bot_token(&account.bot_token)
    )
}

fn build_auth_popup(
    parent: &gtk::Window,
    provider: &str,
    code: &str,
) -> (gtk::Window, gtk::Label, gtk::Button) {
    let provider_label = remote::remote_provider_label(provider);
    let popup = gtk::Window::builder()
        .title(&format!("Authenticate {provider_label}"))
        .default_width(420)
        .default_height(250)
        .modal(true)
        .transient_for(parent)
        .build();
    popup.add_css_class("settings-window");

    let root = gtk::Box::new(gtk::Orientation::Vertical, 12);
    root.set_margin_start(16);
    root.set_margin_end(16);
    root.set_margin_top(16);
    root.set_margin_bottom(16);

    let title = gtk::Label::new(Some(&format!("Send This Code In {provider_label}")));
    title.set_xalign(0.0);
    title.add_css_class("profile-section-title");
    root.append(&title);

    let instructions = gtk::Label::new(Some(
        if remote::normalize_remote_provider(provider) == remote::REMOTE_PROVIDER_TELEGRAM {
            "Open a chat with your Telegram bot, send this exact 6-digit code, and keep the chat open until authentication completes."
        } else {
            "Open the DM from your Discord bot and reply with this exact 6-digit code. Authentication will complete automatically when the bot can read it."
        },
    ));
    instructions.set_xalign(0.0);
    instructions.set_wrap(true);
    instructions.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    instructions.add_css_class("dim-label");
    root.append(&instructions);

    let code_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let code_label = gtk::Label::new(Some(code));
    code_label.add_css_class("remote-auth-code");
    code_label.set_xalign(0.0);
    code_box.append(&code_label);
    let copy_button = gtk::Button::with_label("Copy");
    copy_button.add_css_class("app-flat-button");
    code_box.append(&copy_button);
    root.append(&code_box);

    let status_label = gtk::Label::new(Some(&format!("Waiting for {provider_label} message...")));
    status_label.set_xalign(0.0);
    status_label.set_wrap(true);
    status_label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    status_label.add_css_class("dim-label");
    root.append(&status_label);

    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    actions.set_halign(gtk::Align::End);
    let cancel_button = gtk::Button::with_label("Cancel");
    actions.append(&cancel_button);
    root.append(&actions);

    {
        let code = code.to_string();
        copy_button.connect_clicked(move |_| {
            if let Some(display) = gtk::gdk::Display::default() {
                display.clipboard().set_text(&code);
            }
        });
    }

    popup.set_child(Some(&root));
    (popup, status_label, cancel_button)
}

fn unix_now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn set_auth_code_guard(db: &AppDb, code: &str) {
    let expires_at = unix_now_secs() + 240;
    let _ = db.set_setting(
        remote::SETTING_REMOTE_TELEGRAM_AUTH_EXPECTED_CODE,
        code.trim(),
    );
    let _ = db.set_setting(
        remote::SETTING_REMOTE_TELEGRAM_AUTH_EXPIRES_AT,
        &expires_at.to_string(),
    );
}

fn clear_auth_code_guard(db: &AppDb) {
    let _ = db.set_setting(remote::SETTING_REMOTE_TELEGRAM_AUTH_EXPECTED_CODE, "");
    let _ = db.set_setting(remote::SETTING_REMOTE_TELEGRAM_AUTH_EXPIRES_AT, "0");
}

fn selected_provider(dropdown: &gtk::DropDown) -> &'static str {
    if dropdown.selected() == 1 {
        remote::REMOTE_PROVIDER_TELEGRAM
    } else {
        remote::REMOTE_PROVIDER_DISCORD
    }
}

fn refresh_provider_fields(
    provider: &str,
    token_entry: &gtk::PasswordEntry,
    user_label: &gtk::Label,
    user_entry: &gtk::Entry,
) {
    if remote::normalize_remote_provider(provider) == remote::REMOTE_PROVIDER_TELEGRAM {
        token_entry.set_placeholder_text(Some("Telegram bot token"));
        user_label.set_text("User ID");
        user_entry.set_placeholder_text(Some("Optional Telegram user ID"));
    } else {
        token_entry.set_placeholder_text(Some("Discord bot token"));
        user_label.set_text("User ID");
        user_entry.set_placeholder_text(Some("Your Discord user ID"));
    }
}

pub(crate) fn build_settings_page(dialog: &gtk::Window, db: Rc<AppDb>) -> gtk::Box {
    let root = gtk::Box::new(gtk::Orientation::Vertical, 10);
    root.set_margin_start(12);
    root.set_margin_end(12);
    root.set_margin_top(12);
    root.set_margin_bottom(12);

    let intro = gtk::Label::new(Some(
        "Remote mode forwards assistant updates outside the app. Connect Discord or Telegram, then use the bottom-bar Remote icon to turn it on.",
    ));
    intro.set_xalign(0.0);
    intro.set_wrap(true);
    intro.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    intro.add_css_class("dim-label");
    root.append(&intro);

    let telegram_section = gtk::Box::new(gtk::Orientation::Vertical, 8);
    telegram_section.add_css_class("profile-settings-section");
    let telegram_title = gtk::Label::new(Some("Remote Connection"));
    telegram_title.set_xalign(0.0);
    telegram_title.add_css_class("profile-section-title");
    telegram_section.append(&telegram_title);

    let provider_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let provider_label = gtk::Label::new(Some("Provider"));
    provider_label.set_xalign(0.0);
    provider_label.set_width_chars(12);
    let provider_dropdown = gtk::DropDown::from_strings(&["Discord", "Telegram"]);
    provider_dropdown.set_selected(0);
    provider_dropdown.set_hexpand(true);
    provider_row.append(&provider_label);
    provider_row.append(&provider_dropdown);
    telegram_section.append(&provider_row);

    let token_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let token_label = gtk::Label::new(Some("Bot token"));
    token_label.set_xalign(0.0);
    token_label.set_width_chars(12);
    let token_entry = gtk::PasswordEntry::new();
    token_entry.set_hexpand(true);
    token_entry.set_show_peek_icon(true);
    token_entry.set_placeholder_text(Some("Discord bot token"));
    token_row.append(&token_label);
    token_row.append(&token_entry);
    telegram_section.append(&token_row);

    let user_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let user_label = gtk::Label::new(Some("User ID"));
    user_label.set_xalign(0.0);
    user_label.set_width_chars(12);
    let user_entry = gtk::Entry::new();
    user_entry.set_hexpand(true);
    user_entry.set_placeholder_text(Some("Your Discord user ID"));
    user_row.append(&user_label);
    user_row.append(&user_entry);
    telegram_section.append(&user_row);

    let telegram_actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    telegram_actions.set_halign(gtk::Align::Start);
    let authenticate_button = gtk::Button::with_label("Authenticate");
    authenticate_button.add_css_class("suggested-action");
    let unlink_button = gtk::Button::with_label("Unlink");
    unlink_button.add_css_class("destructive-action");
    telegram_actions.append(&authenticate_button);
    telegram_actions.append(&unlink_button);
    telegram_section.append(&telegram_actions);

    let linked_label = gtk::Label::new(None);
    linked_label.set_xalign(0.0);
    linked_label.set_wrap(true);
    linked_label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    linked_label.add_css_class("dim-label");
    telegram_section.append(&linked_label);

    let auth_status = gtk::Label::new(None);
    auth_status.set_xalign(0.0);
    auth_status.set_wrap(true);
    auth_status.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    auth_status.add_css_class("dim-label");
    telegram_section.append(&auth_status);
    root.append(&telegram_section);

    let active_auth_cancel: Rc<RefCell<Option<Arc<AtomicBool>>>> = Rc::new(RefCell::new(None));

    refresh_provider_fields(
        selected_provider(&provider_dropdown),
        &token_entry,
        &user_label,
        &user_entry,
    );
    {
        let token_entry = token_entry.clone();
        let user_label = user_label.clone();
        let user_entry = user_entry.clone();
        provider_dropdown.connect_selected_notify(move |dropdown| {
            refresh_provider_fields(
                selected_provider(dropdown),
                &token_entry,
                &user_label,
                &user_entry,
            );
        });
    }

    let refresh_ui: Rc<dyn Fn()> = {
        let db = db.clone();
        let provider_dropdown = provider_dropdown.clone();
        let token_entry = token_entry.clone();
        let user_label = user_label.clone();
        let user_entry = user_entry.clone();
        let linked_label = linked_label.clone();
        let unlink_button = unlink_button.clone();
        Rc::new(move || {
            let account = db.remote_telegram_active_account().ok().flatten();
            if let Some(account) = account {
                provider_dropdown.set_selected(
                    if remote::normalize_remote_provider(&account.provider)
                        == remote::REMOTE_PROVIDER_TELEGRAM
                    {
                        1
                    } else {
                        0
                    },
                );
                refresh_provider_fields(&account.provider, &token_entry, &user_label, &user_entry);
                token_entry.set_text(&account.bot_token);
                user_entry.set_text(&account.telegram_user_id);
                linked_label.set_text(&account_summary(&account));
                unlink_button.set_sensitive(true);
            } else {
                user_entry.set_text("");
                linked_label.set_text("No linked remote connection.");
                unlink_button.set_sensitive(false);
            }
        })
    };
    (refresh_ui)();

    {
        let db = db.clone();
        let refresh_ui = refresh_ui.clone();
        let auth_status = auth_status.clone();
        unlink_button.connect_clicked(move |_| {
            let current = db.remote_telegram_active_account().ok().flatten();
            if let Some(account) = current {
                let _ = db.delete_remote_telegram_account(account.id);
            }
            let _ = db.set_remote_mode_enabled(false);
            let _ = db.set_setting(remote::SETTING_REMOTE_TELEGRAM_POLLING_ENABLED, "0");
            remote::stop_background_worker();
            auth_status.set_text("Remote connection unlinked.");
            (refresh_ui)();
        });
    }

    {
        let db = db.clone();
        let dialog = dialog.clone();
        let provider_dropdown = provider_dropdown.clone();
        let token_entry = token_entry.clone();
        let user_entry = user_entry.clone();
        let auth_status = auth_status.clone();
        let refresh_ui = refresh_ui.clone();
        let unlink_button = unlink_button.clone();
        let active_auth_cancel = active_auth_cancel.clone();
        authenticate_button.connect_clicked(move |button| {
            auth_status.set_text("");
            let provider = selected_provider(&provider_dropdown).to_string();
            let provider_label = remote::remote_provider_label(&provider);
            let token = token_entry.text().trim().to_string();
            if token.is_empty() {
                auth_status.set_text(&format!("Enter a {provider_label} bot token first."));
                return;
            }
            let user_id = user_entry.text().trim().to_string();
            if remote::normalize_remote_provider(&provider) == remote::REMOTE_PROVIDER_DISCORD
                && user_id.is_empty()
            {
                auth_status.set_text("Enter your Discord user ID first.");
                return;
            }
            if let Some(existing_cancel) = active_auth_cancel.borrow().as_ref() {
                existing_cancel.store(true, Ordering::Relaxed);
            }

            button.set_sensitive(false);
            unlink_button.set_sensitive(false);
            let code = remote::generate_auth_code();
            set_auth_code_guard(db.as_ref(), &code);
            let (popup, popup_status, popup_cancel) = build_auth_popup(&dialog, &provider, &code);
            popup.present();

            let (rx, cancel_flag) = remote::start_remote_auth_poll(
                provider.clone(),
                token.clone(),
                user_id.clone(),
                code,
                Duration::from_secs(180),
            );
            active_auth_cancel.replace(Some(cancel_flag.clone()));

            {
                let popup = popup.clone();
                let active_auth_cancel = active_auth_cancel.clone();
                let db = db.clone();
                popup_cancel.connect_clicked(move |_| {
                    if let Some(cancel) = active_auth_cancel.borrow().as_ref() {
                        cancel.store(true, Ordering::Relaxed);
                    }
                    clear_auth_code_guard(db.as_ref());
                    popup.close();
                });
            }

            let db = db.clone();
            let auth_status = auth_status.clone();
            let button = button.clone();
            let unlink_button = unlink_button.clone();
            let refresh_ui = refresh_ui.clone();
            let active_auth_cancel = active_auth_cancel.clone();
            gtk::glib::timeout_add_local(Duration::from_millis(120), move || {
                if !popup.is_visible() {
                    clear_auth_code_guard(db.as_ref());
                    button.set_sensitive(true);
                    (refresh_ui)();
                    return gtk::glib::ControlFlow::Break;
                }

                match rx.try_recv() {
                    Ok(Ok(found)) => {
                        clear_auth_code_guard(db.as_ref());
                        match db.upsert_remote_connection(
                            &provider,
                            &token,
                            &found.user_id,
                            &found.chat_id,
                            found.username.as_deref(),
                        ) {
                            Ok(_) => {
                                let _ = db.set_setting(
                                    remote::SETTING_REMOTE_TELEGRAM_POLLING_ENABLED,
                                    "1",
                                );
                                remote::start_background_worker();
                                auth_status.set_text(&format!(
                                    "{provider_label} remote connection authenticated."
                                ));
                                popup.close();
                            }
                            Err(err) => {
                                let message =
                                    format!("Authentication matched, but failed to save: {err}");
                                popup_status.set_text(&message);
                                auth_status.set_text(&message);
                            }
                        }
                        button.set_sensitive(true);
                        unlink_button.set_sensitive(true);
                        active_auth_cancel.replace(None);
                        (refresh_ui)();
                        gtk::glib::ControlFlow::Break
                    }
                    Ok(Err(err)) => {
                        clear_auth_code_guard(db.as_ref());
                        popup_status.set_text(&err);
                        auth_status.set_text(&err);
                        button.set_sensitive(true);
                        unlink_button.set_sensitive(true);
                        active_auth_cancel.replace(None);
                        (refresh_ui)();
                        gtk::glib::ControlFlow::Break
                    }
                    Err(TryRecvError::Empty) => gtk::glib::ControlFlow::Continue,
                    Err(TryRecvError::Disconnected) => {
                        clear_auth_code_guard(db.as_ref());
                        let message = "Remote auth worker disconnected unexpectedly.".to_string();
                        popup_status.set_text(&message);
                        auth_status.set_text(&message);
                        button.set_sensitive(true);
                        unlink_button.set_sensitive(true);
                        active_auth_cancel.replace(None);
                        (refresh_ui)();
                        gtk::glib::ControlFlow::Break
                    }
                }
            });
        });
    }

    root
}
