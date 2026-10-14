//! Help ▸ Check for updates (issue #28): ask for the latest release, and install it in one click
//! where the desktop app knows how.
//!
//! The desktop app supplies how to ask ([`PdfCraftApp::update_source`]) and how to install
//! ([`PdfCraftApp::update_installer`]), so this crate has no network code; without a source (the
//! web build, tests) the command opens the releases page. A Microsoft Store copy is updated by the
//! Store ([`PdfCraftApp::updates_from_store`]): NadsPDF neither checks nor installs anything then.
//!
//! Unless the user turns it off in the dialog, NadsPDF asks once a day at start, silently, and
//! shows the dialog only when a newer version is out.

use std::sync::Arc;

use egui::{Align, Layout};

use crate::{PdfCraftApp, theme, widgets};

/// Where every NadsPDF release is listed.
pub const RELEASES_PAGE: &str = "https://github.com/nadsous/nadspdf/releases";

/// Seconds between two automatic checks.
const AUTO_CHECK_EVERY: u64 = 24 * 60 * 60;

/// The latest published release.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Release {
    /// Its version tag, such as `v0.2.0`.
    pub version: String,
    /// Its page on [`RELEASES_PAGE`], where the downloads are.
    pub url: String,
    /// The installer for this computer, when the release has one NadsPDF can install itself.
    pub installer: Option<Installer>,
}

/// A release's installer: where to download it and the digest it must have.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Installer {
    /// The file name, such as `nadspdf-0.7.2-windows-x64.msi`.
    pub name: String,
    /// Its download address under [`RELEASES_PAGE`].
    pub url: String,
    /// Its SHA-256, lowercase hex, as GitHub published it.
    pub sha256: String,
}

/// Asks for the latest release (blocking; it runs on its own thread).
pub type UpdateSource = Arc<dyn Fn() -> Result<Release, String> + Send + Sync>;

/// Downloads an installer, checks its digest and starts it so that it runs once NadsPDF has
/// quit and opens the new version afterwards (blocking; it runs on its own thread).
pub type UpdateInstaller = Arc<dyn Fn(&Installer) -> Result<(), String> + Send + Sync>;

/// Whether release `latest` (a tag such as `v0.2.0`) is newer than version `current` (`0.1.1`).
/// Pre-release and build suffixes are ignored; a version that doesn't parse is never newer.
pub fn is_newer(latest: &str, current: &str) -> bool {
    matches!((parse(latest), parse(current)), (Some(l), Some(c)) if l > c)
}

fn parse(v: &str) -> Option<(u64, u64, u64)> {
    let v = v.trim().trim_start_matches(['v', 'V']);
    let core = v.split(['-', '+']).next()?;
    let mut parts = core.split('.');
    let mut next = |required: bool| match parts.next() {
        Some(p) => p.parse::<u64>().ok(),
        None if required => None,
        None => Some(0),
    };
    let version = (next(true)?, next(false)?, next(false)?);
    parts.next().is_none().then_some(version)
}

/// Where a check is.
#[derive(Default)]
pub(crate) enum Check {
    #[default]
    Idle,
    #[cfg(not(target_arch = "wasm32"))]
    Running(std::sync::mpsc::Receiver<Result<Release, String>>),
    Done(Result<Release, String>),
}

/// Where an installation is.
#[derive(Default)]
pub(crate) enum Install {
    #[default]
    Idle,
    #[cfg(not(target_arch = "wasm32"))]
    Running(std::sync::mpsc::Receiver<Result<(), String>>),
    Failed(String),
}

pub(crate) struct Updates {
    pub(crate) check: Check,
    pub(crate) install: Install,
    /// The Updates dialog is showing.
    pub(crate) open: bool,
    /// The running check was started at launch: show the dialog only if it finds a new version.
    pub(crate) quiet: bool,
    /// The launch check has been considered (once per run).
    pub(crate) started: bool,
    /// Ask once a day at start (saved).
    pub(crate) auto: bool,
    /// When the last automatic check ran, in Unix seconds (saved).
    pub(crate) checked_at: u64,
}

impl Default for Updates {
    fn default() -> Self {
        Self { check: Check::Idle, install: Install::Idle, open: false, quiet: false, started: false, auto: true, checked_at: 0 }
    }
}

impl Updates {
    pub(crate) fn persist(&self) -> serde_json::Value {
        serde_json::json!({ "auto": self.auto, "checked_at": self.checked_at })
    }

    pub(crate) fn restore(&mut self, v: &serde_json::Value) {
        if let Some(on) = v["auto"].as_bool() {
            self.auto = on;
        }
        if let Some(at) = v["checked_at"].as_u64() {
            self.checked_at = at;
        }
    }
}

/// The current Unix time in seconds (0 when the clock gives nothing usable).
#[cfg(not(target_arch = "wasm32"))]
fn now_unix() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Whether an automatic check is due at `now`, the last one having run at `last`. A clock set
/// back (now before last) counts as due, so a wrong date can't silence checks for good.
fn due(last: u64, now: u64) -> bool {
    now.checked_sub(last).is_none_or(|elapsed| elapsed >= AUTO_CHECK_EVERY)
}

impl PdfCraftApp {
    /// Help ▸ Check for updates: ask for the latest release and show the outcome.
    pub fn check_for_updates(&mut self) {
        if self.updates_from_store {
            self.updates.open = true;
            return;
        }
        self.start_check(false);
    }

    fn start_check(&mut self, quiet: bool) {
        let Some(source) = self.update_source.clone() else {
            if !quiet {
                self.open_url(RELEASES_PAGE);
            }
            return;
        };
        #[cfg(not(target_arch = "wasm32"))]
        {
            if !quiet {
                self.updates.open = true;
            }
            if matches!(self.updates.check, Check::Running(_)) {
                // A visible request takes over a quiet one still running.
                self.updates.quiet &= quiet;
                return;
            }
            self.updates.quiet = quiet;
            let (tx, rx) = std::sync::mpsc::channel();
            let ctx = self.ctx.clone();
            std::thread::spawn(move || {
                // The receiver may be gone (the app quit): nothing to report to then.
                let _ = tx.send(source());
                if let Some(ctx) = ctx {
                    ctx.request_repaint();
                }
            });
            self.updates.check = Check::Running(rx);
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = source;
            if !quiet {
                self.open_url(RELEASES_PAGE);
            }
        }
    }

    /// The once-a-day check at start, when it's on and due.
    fn maybe_check_at_start(&mut self) {
        if self.updates.started {
            return;
        }
        self.updates.started = true;
        #[cfg(not(target_arch = "wasm32"))]
        {
            let now = now_unix();
            if self.updates.auto && !self.updates_from_store && self.update_source.is_some() && due(self.updates.checked_at, now) {
                self.updates.checked_at = now;
                self.start_check(true);
            }
        }
    }

    /// Download and start the installer of the release found, then quit so it can run.
    fn install_update(&mut self, installer: Installer) {
        let Some(install) = self.update_installer.clone() else {
            return;
        };
        #[cfg(not(target_arch = "wasm32"))]
        {
            if matches!(self.updates.install, Install::Running(_)) {
                return;
            }
            let (tx, rx) = std::sync::mpsc::channel();
            let ctx = self.ctx.clone();
            std::thread::spawn(move || {
                let _ = tx.send(install(&installer));
                if let Some(ctx) = ctx {
                    ctx.request_repaint();
                }
            });
            self.updates.install = Install::Running(rx);
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (install, installer);
        }
    }

    /// Start the launch check, and pick up a finished check or download (each frame).
    pub(crate) fn poll_updates(&mut self) {
        self.maybe_check_at_start();
        #[cfg(not(target_arch = "wasm32"))]
        {
            if let Check::Running(rx) = &self.updates.check {
                let result = match rx.try_recv() {
                    Ok(r) => Some(r),
                    Err(std::sync::mpsc::TryRecvError::Empty) => None,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => Some(Err("the update check stopped unexpectedly".into())),
                };
                if let Some(result) = result {
                    if self.updates.quiet {
                        // A launch check speaks up only for a new version.
                        let newer = matches!(&result, Ok(r) if is_newer(&r.version, env!("CARGO_PKG_VERSION")));
                        self.updates.open |= newer;
                        self.updates.quiet = false;
                    }
                    self.updates.check = Check::Done(result);
                }
            }
            if let Install::Running(rx) = &self.updates.install {
                let result = match rx.try_recv() {
                    Ok(r) => Some(r),
                    Err(std::sync::mpsc::TryRecvError::Empty) => None,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => Some(Err("the download stopped unexpectedly".into())),
                };
                match result {
                    None => {}
                    Some(Ok(())) => {
                        // The installer waits for NadsPDF to quit; unsaved work is asked about as
                        // for any other quit.
                        self.updates.install = Install::Idle;
                        self.updates.open = false;
                        if let Some(ctx) = &self.ctx {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    }
                    Some(Err(e)) => self.updates.install = Install::Failed(e),
                }
            }
        }
    }
}

/// What the dialog's buttons do.
enum Choice {
    Close,
    Download(String),
    Install(Installer),
}

/// The Updates dialog.
pub(crate) fn dialog(app: &mut PdfCraftApp, ctx: &egui::Context) {
    // A quiet check shows nothing.
    if !app.updates.open || app.updates.quiet {
        return;
    }
    let t = theme::Tokens::get(ctx);
    let current = env!("CARGO_PKG_VERSION");
    let can_install = app.update_installer.is_some();
    let from_store = app.updates_from_store;
    let mut choice: Option<Choice> = None;
    let modal = crate::widgets::show_modal(ctx, "updates", |ui| {
        ui.set_width(440.0);
        ui.horizontal(|ui| {
            ui.add(crate::icons::image("cloud", 22.0, t.accent));
            ui.label(egui::RichText::new(tl!("Check for updates")).font(theme::semibold(16.0)));
        });
        ui.add_space(8.0);
        let mut offer: Option<Release> = None;
        let installing = !matches!(app.updates.install, Install::Idle | Install::Failed(_));
        if from_store {
            ui.label(crate::i18n::fmt(tl!("You have version {c}."), &[("c", current)]));
            ui.label(
                egui::RichText::new(tl!("This copy of NadsPDF comes from the Microsoft Store, which installs new versions by itself."))
                    .color(t.text_muted),
            );
        } else {
            match &app.updates.check {
                Check::Idle => {
                    ui.label(tl!("No check has run yet."));
                }
                #[cfg(not(target_arch = "wasm32"))]
                Check::Running(_) => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(tl!("Checking for a newer version…"));
                    });
                }
                Check::Done(Ok(r)) if is_newer(&r.version, current) => {
                    let version = r.version.trim_start_matches(['v', 'V']);
                    ui.label(egui::RichText::new(crate::i18n::fmt(tl!("NadsPDF {v} is available."), &[("v", version)])).strong());
                    let hint = if can_install && r.installer.is_some() {
                        tl!(
                            "You have version {c}. NadsPDF will close, install the new version in place of this one and open again. Your settings are kept."
                        )
                    } else {
                        tl!("You have version {c}. Download the new version from its release page.")
                    };
                    ui.label(egui::RichText::new(crate::i18n::fmt(hint, &[("c", current)])).color(t.text_muted));
                    offer = Some(r.clone());
                }
                Check::Done(Ok(_)) => {
                    ui.label(crate::i18n::fmt(tl!("NadsPDF {c} is up to date."), &[("c", current)]));
                }
                Check::Done(Err(e)) => {
                    ui.label(crate::i18n::fmt(tl!("Couldn't check for updates: {e}"), &[("e", &e.to_string())]));
                    ui.label(
                        egui::RichText::new(crate::i18n::fmt(
                            tl!("You have version {c}. All releases are listed at {page}."),
                            &[("c", current), ("page", RELEASES_PAGE)],
                        ))
                        .color(t.text_muted),
                    );
                }
            }
            if installing {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(tl!("Downloading the update…"));
                });
            }
            if let Install::Failed(e) = &app.updates.install {
                ui.add_space(6.0);
                ui.label(egui::RichText::new(crate::i18n::fmt(tl!("Couldn't install the update: {e}"), &[("e", e)])).color(t.accent));
            }
            ui.add_space(10.0);
            ui.add(widgets::checkbox(&mut app.updates.auto, tl!("Check for updates when NadsPDF starts (once a day)")));
        }
        ui.add_space(12.0);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| match offer {
            Some(r) if can_install && !installing => {
                let failed = matches!(app.updates.install, Install::Failed(_));
                match r.installer {
                    Some(installer) if !failed => {
                        if widgets::pill_button(ui, tl!("Update"), true).clicked() {
                            choice = Some(Choice::Install(installer));
                        }
                    }
                    _ => {
                        if widgets::pill_button(ui, tl!("Download"), true).clicked() {
                            choice = Some(Choice::Download(r.url.clone()));
                        }
                    }
                }
                if widgets::pill_button(ui, tl!("Later"), false).clicked() {
                    choice = Some(Choice::Close);
                }
            }
            Some(r) if !installing => {
                if widgets::pill_button(ui, tl!("Download"), true).clicked() {
                    choice = Some(Choice::Download(r.url));
                }
                if widgets::pill_button(ui, tl!("Later"), false).clicked() {
                    choice = Some(Choice::Close);
                }
            }
            _ => {
                if widgets::pill_button(ui, tl!("Close"), true).clicked() {
                    choice = Some(Choice::Close);
                }
            }
        });
    });
    if modal.should_close() {
        choice = Some(Choice::Close);
    }
    match choice {
        None => {}
        Some(Choice::Close) => {
            app.updates.open = false;
            if matches!(app.updates.install, Install::Failed(_)) {
                app.updates.install = Install::Idle;
            }
        }
        Some(Choice::Download(url)) => {
            app.updates.open = false;
            app.open_url(&url);
        }
        Some(Choice::Install(installer)) => app.install_update(installer),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_check_is_due_once_a_day_and_after_the_clock_goes_back() {
        assert!(due(0, 1_800_000_000));
        assert!(!due(1_800_000_000, 1_800_000_000 + AUTO_CHECK_EVERY - 1));
        assert!(due(1_800_000_000, 1_800_000_000 + AUTO_CHECK_EVERY));
        assert!(due(1_800_000_000, 1_700_000_000), "a clock set back");
        assert!(!due(u64::MAX - 1, u64::MAX));
    }
}
