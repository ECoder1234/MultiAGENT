use crate::services::app::chat::AppDb;

pub(crate) const SETTING_MULTIVIEW_ENABLED: &str = "multiview_enabled";
pub(crate) const SETTING_PANE_LAYOUT_V1: &str = "pane_layout_v1";

pub(crate) fn single_thread_layout_json() -> &'static str {
    r#"{"version":1,"focusedPaneId":1,"panes":[]}"#
}

pub(crate) fn force_single_thread_mode(db: &AppDb) {
    let _ = db.set_setting(SETTING_MULTIVIEW_ENABLED, "0");
    let _ = db.set_setting(SETTING_PANE_LAYOUT_V1, single_thread_layout_json());
}
