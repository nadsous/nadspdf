//! NadsPDF: software updates are NadsPDF's own (its Help menu and Preferences), so the page
//! layout interface checks nothing itself. These keep the names the rest of it uses.

use serde_json::Value;

use crate::DesignApp;

#[derive(Default)]
pub(crate) struct Runtime {
    /// The web build's Help ▸ Check for Updates window (never opened in NadsPDF).
    #[cfg(target_arch = "wasm32")]
    pub(crate) manual_window_open: bool,
}

impl DesignApp {
    /// No check at start: NadsPDF checks for its own updates.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn set_startup_update_check(&mut self, _on: bool) {}
}

/// Help ▸ Check for Updates: NadsPDF's own Help menu does it.
pub(crate) fn check(_app: &mut DesignApp) -> Result<Value, String> {
    Err("NadsPDF checks for updates from its own Help menu.".into())
}

pub(crate) fn tick(_app: &mut DesignApp, _ctx: &egui::Context) {}

pub(crate) fn show(_app: &mut DesignApp, _ctx: &egui::Context) {}
