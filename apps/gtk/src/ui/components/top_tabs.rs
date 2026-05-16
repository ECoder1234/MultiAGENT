use adw::prelude::*;

pub fn build_top_tabs(
    stack: &adw::ViewStack,
    browser_split_toggle: Option<&gtk::ToggleButton>,
) -> gtk::Box {
    let tabs = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    tabs.add_css_class("top-tabs");
    tabs.set_halign(gtk::Align::Center);
    tabs.set_valign(gtk::Align::Center);

    let chat = tab_button("chat-new-symbolic", "Chat");
    chat.add_css_class("top-tab-chat");
    let browser = tab_button("web-browser-symbolic", "Browser");
    browser.add_css_class("top-tab-browser");
    let git = tab_button("git-symbolic", "Review");
    git.add_css_class("top-tab-git");
    let actions = tab_button("terminal-symbolic", "Actions");
    actions.add_css_class("top-tab-actions");
    let buttons = vec![chat.clone(), browser.clone(), git.clone(), actions.clone()];
    set_active_tab(&buttons, 0);

    {
        let stack = stack.clone();
        let buttons = buttons.clone();
        chat.connect_clicked(move |_| {
            stack.set_visible_child_name("chat");
            set_active_tab(&buttons, 0);
        });
    }

    if let Some(browser_split_toggle) = browser_split_toggle {
        let toggle = browser_split_toggle.clone();
        browser.connect_clicked(move |_| {
            toggle.set_active(!toggle.is_active());
        });
        {
            let browser = browser.clone();
            browser_split_toggle.connect_toggled(move |toggle| {
                if toggle.is_active() {
                    browser.add_css_class("top-tab-split-open");
                } else {
                    browser.remove_css_class("top-tab-split-open");
                }
            });
        }
    } else {
        let stack = stack.clone();
        let buttons = buttons.clone();
        browser.connect_clicked(move |_| {
            stack.set_visible_child_name("browser");
            set_active_tab(&buttons, 1);
        });
    }

    {
        let stack = stack.clone();
        let buttons = buttons.clone();
        git.connect_clicked(move |_| {
            stack.set_visible_child_name("git");
            set_active_tab(&buttons, 2);
        });
    }

    {
        let stack = stack.clone();
        let buttons = buttons.clone();
        actions.connect_clicked(move |_| {
            stack.set_visible_child_name("actions");
            set_active_tab(&buttons, 3);
        });
    }

    tabs.append(&chat);
    tabs.append(&tab_separator());
    tabs.append(&browser);
    tabs.append(&tab_separator());
    tabs.append(&git);
    tabs.append(&tab_separator());
    tabs.append(&actions);
    tabs
}

fn set_active_tab(buttons: &[gtk::Button], active_idx: usize) {
    for (idx, button) in buttons.iter().enumerate() {
        if idx == active_idx {
            button.add_css_class("top-tab-active");
        } else {
            button.remove_css_class("top-tab-active");
        }
    }
}

fn tab_button(icon: &str, label: &str) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class("app-flat-button");
    button.add_css_class("top-tab");
    button.set_valign(gtk::Align::Center);

    let content = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    content.set_valign(gtk::Align::Center);
    let image = gtk::Image::from_icon_name(icon);
    image.set_pixel_size(12);
    image.set_valign(gtk::Align::Center);
    content.append(&image);

    let text = gtk::Label::new(Some(label));
    text.set_valign(gtk::Align::Center);
    content.append(&text);

    button.set_child(Some(&content));
    button
}

fn tab_separator() -> gtk::Label {
    let separator = gtk::Label::new(Some("|"));
    separator.add_css_class("tab-separator");
    separator.set_valign(gtk::Align::Center);
    separator
}
