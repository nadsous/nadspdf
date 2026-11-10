//! Vaults on the Home view, in the All tools panel, the command palette and their dialogs (the
//! model is in [`crate::vaults`]). Files stay where they are: nothing here moves, copies or
//! deletes a user's file, only the vault's list of paths.

// The browser has no file paths to keep: vaults are desktop-only, and their code idles there.
#![cfg_attr(target_arch = "wasm32", allow(dead_code))]

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use egui::{Align2, Color32, CornerRadius, Rect, Sense, Stroke, vec2};

use crate::PdfCraftApp;
use crate::theme::{self, Tokens};
use crate::vaults::{self, Added, FileStat, Status, VaultError, Vaults};
use crate::{icons, widgets};

mod page;

/// "Open all" opens at most this many files at once.
pub const MAX_OPEN_ALL: usize = 30;
/// While a vault shows, its files are looked up again at most this often (seconds).
const RESCAN_SECS: f64 = 5.0;

/// A vault chip being dragged (to reorder vaults).
#[derive(Clone, Copy, Debug)]
struct DragVault(u64);

/// A vault's file being dragged (to move it to another vault).
#[derive(Clone, Debug)]
struct DragFile {
    from: u64,
    path: String,
}

/// An open vault dialog.
#[derive(Clone, Debug, PartialEq)]
pub enum VaultDialog {
    /// Create (`id` None) or edit a vault; `then_add` joins a new vault once it is created.
    Edit { id: Option<u64>, name: String, description: String, color: [u8; 3], icon: &'static str, seed: u64, then_add: Vec<String> },
    /// Confirm deleting a vault.
    Delete(u64),
    /// Choose the vault a document joins.
    AddDocument(String),
    /// A file's tags, and the tag being typed.
    Tags { id: u64, path: String, draft: String },
    /// A file's note.
    Note { id: u64, path: String, text: String },
    /// Name a new binder (`binder` None) or rename one.
    Binder { id: u64, binder: Option<u64>, name: String },
}

type Scan = Arc<Mutex<Option<Vec<(String, FileStat)>>>>;

/// The vaults with what the Home view needs to show them.
#[derive(Debug, Default)]
pub struct VaultState {
    pub data: Vaults,
    /// The vault whose files Home lists.
    pub selected: Option<u64>,
    pub dialog: Option<VaultDialog>,
    /// What each listed file was like when last looked up.
    stats: HashMap<String, FileStat>,
    scan: Option<Scan>,
    last_scan: Option<f64>,
    stale: bool,
    /// Where each vault chip was drawn, and the open vault's file list, in the last frame that
    /// showed them (`pass`): files dropped from the system land there.
    targets: Vec<(u64, Rect)>,
    targets_pass: u64,
    /// The open vault's page: filters, sort, view and first-page pictures.
    pub page: page::PageState,
}

impl VaultState {
    /// What `path` was like when last looked up; `None` until a lookup finishes.
    pub fn stat(&self, path: &str) -> Option<FileStat> {
        self.stats.get(path).copied()
    }

    /// Look the shown vault's files up again on the next frame.
    pub fn refresh(&mut self) {
        self.stale = true;
    }

    /// Restore vaults from settings ([`Vaults::restore`]).
    pub(crate) fn restore(&mut self, value: &serde_json::Value) {
        self.data.restore(value);
        self.selected = None;
        self.dialog = None;
        self.stats.clear();
        self.stale = true;
    }
}

fn color32(c: [u8; 3]) -> Color32 {
    Color32::from_rgb(c[0], c[1], c[2])
}

/// "{n} files" in the interface language.
fn count_label(n: usize) -> String {
    crate::i18n::trn(crate::i18n::current(), n as u64, "{n} file", "{n} files")
}

impl PdfCraftApp {
    /// New vault…: open the dialog that names a new vault.
    pub fn new_vault_dialog(&mut self, then_add: Vec<String>) {
        let n = self.vaults.data.list.len();
        let color = vaults::COLORS.get(n % vaults::COLORS.len()).copied().unwrap_or(vaults::COLORS[0]);
        let seed = crate::vault_monster::new_seed(n as u64);
        self.vaults.dialog =
            Some(VaultDialog::Edit { id: None, name: String::new(), description: String::new(), color, icon: "folder", seed, then_add });
    }

    /// Create a vault and show it on Home; `None`, after telling the user, when it can't be.
    pub fn create_vault(&mut self, name: &str, color: [u8; 3], icon: &str) -> Option<u64> {
        match self.vaults.data.create(name, color, icon) {
            Ok(id) => {
                self.vaults.selected = Some(id);
                self.vaults.stale = true;
                Some(id)
            }
            Err(e) => {
                self.vault_error(e);
                None
            }
        }
    }

    fn vault_error(&mut self, e: VaultError) {
        match e {
            VaultError::EmptyName => self.notify_tr("Give the vault a name"),
            VaultError::TooManyVaults => {
                self.notify_fmt("You can have up to {n} vaults. Delete one first.", &[("n", &vaults::MAX_VAULTS.to_string())])
            }
            VaultError::NoSuchVault => self.notify_tr("That vault no longer exists"),
        }
    }

    /// Add `paths` to the vault `id` and say what happened.
    pub fn add_to_vault(&mut self, id: u64, paths: &[String]) -> Added {
        let name = self.vaults.data.get(id).map(|v| v.name.clone()).unwrap_or_default();
        match self.vaults.data.add_files(id, paths) {
            Ok(a) => {
                self.vaults.stale = true;
                if a.added > 0 {
                    self.notify_fmt("Added {files} to {vault}", &[("files", &count_label(a.added)), ("vault", &name)]);
                } else if a.duplicates > 0 {
                    self.notify_fmt("Already in {vault}", &[("vault", &name)]);
                } else if a.over_limit > 0 {
                    self.notify_fmt("A vault holds up to {n} files", &[("n", &vaults::MAX_FILES.to_string())]);
                } else if a.rejected > 0 {
                    self.notify_tr("Vaults hold PDFs and files NadsPDF can convert, on this computer");
                }
                a
            }
            Err(e) => {
                self.vault_error(e);
                Added::default()
            }
        }
    }

    /// Show the vault `id` on Home (from the palette or the All tools panel).
    pub fn show_vault(&mut self, id: u64) {
        if self.vaults.data.get(id).is_none() {
            return;
        }
        self.vaults.selected = Some(id);
        self.vaults.stale = true;
        self.active = None;
        self.combine_tab.focused = false;
    }

    /// Open every file of the vault `id` that is there, as tabs (at most [`MAX_OPEN_ALL`]).
    pub fn open_vault(&mut self, id: u64) {
        let Some(v) = self.vaults.data.get(id) else { return };
        let files: Vec<String> = v.files.iter().filter(|f| vaults::stat(f).exists).cloned().collect();
        if files.is_empty() {
            self.notify_tr("No files to open in this vault");
            return;
        }
        if files.len() > MAX_OPEN_ALL {
            self.notify_fmt("Opening the first {n} files", &[("n", &MAX_OPEN_ALL.to_string())]);
        }
        for f in files.iter().take(MAX_OPEN_ALL) {
            self.open_recent(f);
        }
    }

    /// Delete the vault `id`. Its files are not touched.
    pub fn delete_vault(&mut self, id: u64) {
        if let Some(v) = self.vaults.data.delete(id) {
            if self.vaults.selected == Some(id) {
                self.vaults.selected = None;
            }
            self.notify_fmt("Deleted the vault {vault}. Its files were not touched.", &[("vault", &v.name)]);
        }
    }

    /// Move `path` from one vault to another and say so.
    pub fn move_to_vault(&mut self, from: u64, to: u64, path: &str) -> bool {
        let moved = self.vaults.data.move_file(from, to, path);
        if moved {
            let name = self.vaults.data.get(to).map(|v| v.name.clone()).unwrap_or_default();
            self.notify_fmt("Moved {file} to {vault}", &[("file", &vaults::file_name(path)), ("vault", &name)]);
            self.vaults.stale = true;
        }
        moved
    }

    /// Add files to a vault…: choose PDFs (and files NadsPDF converts) for the vault `id`.
    pub fn add_files_to_vault_dialog(&mut self, id: u64) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let dialog = rfd::AsyncFileDialog::new().set_title(tl!("Add files…").to_string());
            let dialog = Self::with_open_filters(dialog);
            self.ask(crate::pickers::Ask::Files(dialog), None, move |app, paths| {
                let paths: Vec<String> = paths.iter().map(|p| p.to_string_lossy().into_owned()).collect();
                app.add_to_vault(id, &paths);
            });
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = id;
            self.notify_tr("Vaults need the desktop app");
        }
    }

    /// Locate…: point a vault's missing file at where it is now.
    fn locate_vault_file(&mut self, id: u64, old: String) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let mut dialog = rfd::AsyncFileDialog::new().set_title(crate::i18n::fmt(tl!("Locate {file}"), &[("file", &vaults::file_name(&old))]));
            if let Some(dir) = std::path::Path::new(&old).parent().filter(|d| d.is_dir()) {
                dialog = dialog.set_directory(dir);
            }
            self.ask_one(crate::pickers::Ask::File(dialog), None, move |app, new| {
                if app.vaults.data.relocate(id, &old, &new.to_string_lossy()) {
                    app.vaults.stale = true;
                }
            });
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (id, old);
        }
    }

    /// Add document to vault…: the active document joins a vault the user picks.
    pub fn add_current_to_vault(&mut self) {
        let path = self.active_ids().and_then(|(_, id)| self.session.get(id)).and_then(|d| d.path.clone());
        match path {
            Some(path) if self.vaults.data.list.is_empty() => self.new_vault_dialog(vec![path]),
            Some(path) => self.vaults.dialog = Some(VaultDialog::AddDocument(path)),
            None => self.notify_tr("Save this document to a file first, then add it to a vault"),
        }
    }

    /// Files dropped from the system on a vault chip (or the open vault's file list) join that
    /// vault instead of opening.
    pub(crate) fn drop_on_vault(&mut self, dropped: Vec<egui::DroppedFileHandle>, ctx: &egui::Context) -> Vec<egui::DroppedFileHandle> {
        if dropped.is_empty() || self.active.is_some() || self.combine_showing() {
            return dropped;
        }
        let Some(id) = self.vault_under_pointer(ctx) else { return dropped };
        let paths: Vec<String> = dropped.iter().filter(|f| f.path().is_absolute()).map(|f| f.path().to_string_lossy().into_owned()).collect();
        if paths.is_empty() {
            self.notify_tr("Vaults hold PDFs and files NadsPDF can convert, on this computer");
        } else {
            self.add_to_vault(id, &paths);
        }
        Vec::new()
    }

    /// The vault whose chip (or file list) the pointer is over, from the last frame that drew them.
    fn vault_under_pointer(&self, ctx: &egui::Context) -> Option<u64> {
        // Only targets drawn in the last pass or two count: Home may have closed since.
        if ctx.cumulative_pass_nr().saturating_sub(self.vaults.targets_pass) > 2 {
            return None;
        }
        let pos = ctx.input(|i| i.pointer.latest_pos())?;
        self.vaults.targets.iter().rev().find(|(_, r)| r.contains(pos)).map(|(id, _)| *id)
    }

    /// Look up the shown vault's files again if due (called while Home shows; `now` is egui time),
    /// on a worker thread so an offline drive never blocks the frame (inline in tests).
    pub fn refresh_vault_stats(&mut self, now: f64) {
        self.poll_vault_stats();
        if self.vaults.scan.is_some() {
            return;
        }
        let Some(files) = self.vaults.selected.and_then(|id| self.vaults.data.get(id)).map(|v| v.files.clone()) else { return };
        let due = self.vaults.stale || self.vaults.last_scan.is_none_or(|last| now < last || now - last >= RESCAN_SECS);
        if !due {
            return;
        }
        self.vaults.stale = false;
        self.vaults.last_scan = Some(now);
        let slot: Scan = Arc::new(Mutex::new(None));
        let out = slot.clone();
        let ctx = self.ctx.clone();
        let work = move || {
            let stats: Vec<(String, FileStat)> = files.into_iter().map(|f| (f.clone(), vaults::stat(&f))).collect();
            *out.lock().unwrap_or_else(PoisonError::into_inner) = Some(stats);
            if let Some(ctx) = ctx {
                ctx.request_repaint();
            }
        };
        if self.run_inline || self.ctx.is_none() {
            work();
        } else if std::thread::Builder::new().name("pdfcraft-vaults".into()).spawn(work).is_err() {
            return;
        }
        self.vaults.scan = Some(slot);
        self.poll_vault_stats();
    }

    fn poll_vault_stats(&mut self) {
        let Some(slot) = &self.vaults.scan else { return };
        let done = slot.lock().unwrap_or_else(PoisonError::into_inner).take();
        match done {
            Some(stats) => {
                self.vaults.scan = None;
                self.vaults.stats = stats.into_iter().collect();
            }
            None if Arc::strong_count(slot) == 1 => self.vaults.scan = None,
            None => {}
        }
    }
}

/// What the user did in the vault section, applied after drawing.
enum Act {
    /// Open the vault's page, or close it when it is the one open.
    Select(u64),
    /// Open the vault's page.
    Show(u64),
    SetStatus(u64, String, Status),
    SetFolder(u64, String, Option<u64>),
    Tags(u64, String),
    Note(u64, String),
    /// Name a new binder, or rename one.
    BinderDialog(u64, Option<u64>),
    DeleteBinder(u64, u64),
    New,
    Edit(u64),
    Delete(u64),
    OpenAll(u64),
    AddFiles(u64),
    Shift(u64, isize),
    Reorder(u64, usize),
    Open(String),
    Remove(u64, String),
    Locate(u64, String),
    Move(u64, u64, String),
}

/// A vault's tile: its pixel monster (`vault_monster`) on a soft tile of its colour.
fn tile(ui: &egui::Ui, rect: Rect, color: [u8; 3], seed: u64, radius: u8) {
    crate::vault_monster::paint(ui.painter(), rect, seed, color, radius);
}

/// A small text button painted inside a row.
fn row_button(ui: &mut egui::Ui, t: &Tokens, rect: Rect, id: egui::Id, label: &str, strong: bool) -> egui::Response {
    let resp = ui.interact(rect, id, Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
    let fill = if resp.hovered() { t.pressed } else { t.hover };
    ui.painter().rect(rect, CornerRadius::same(8), fill, Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, label, theme::medium(12.0), if strong { t.accent_text } else { t.text });
    theme::hand(resp)
}

fn text_w(ui: &egui::Ui, text: &str, font: &egui::FontId) -> f32 {
    ui.fonts_mut(|f| f.layout_no_wrap(text.to_owned(), font.clone(), Color32::WHITE).size().x)
}

/// The Home view's Vaults section.
pub(crate) fn section(app: &mut PdfCraftApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    app.refresh_vault_stats(ui.input(|i| i.time));
    if app.vaults.selected.is_some_and(|id| app.vaults.data.get(id).is_none()) {
        app.vaults.selected = None;
    }
    let pass = ui.ctx().cumulative_pass_nr();
    let mut targets = Vec::new();
    let files_hovering = ui.input(|i| !i.raw.hovered_files.is_empty());
    let pointer = ui.input(|i| i.pointer.latest_pos());
    let mut act: Option<Act> = None;

    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(tl!("Vaults")).font(theme::semibold(17.0)));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // With vaults, the New vault card after them does this.
            if app.vaults.data.list.is_empty() && widgets::ghost_button(ui, "plus", tl!("New vault")).clicked() {
                act = Some(Act::New);
            }
        });
    });
    ui.add_space(4.0);
    if app.vaults.data.list.is_empty() {
        ui.label(
            egui::RichText::new(tl!(
                "Group PDFs from anywhere on your computer into named vaults with a colour and an icon. Files stay where they are."
            ))
            .color(t.text_muted),
        );
        // Empty: the header's New vault button is enough, so Home stays compact.
        app.vaults.targets.clear();
        if matches!(act, Some(Act::New)) {
            app.new_vault_dialog(Vec::new());
        }
        return;
    }

    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(12.0, 12.0);
        let count = app.vaults.data.list.len();
        for (index, v) in app.vaults.data.list.iter().enumerate() {
            let (rect, resp) = ui.allocate_exact_size(vec2(248.0, 96.0), Sense::click_and_drag());
            targets.push((v.id, rect));
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &v.name));
            resp.dnd_set_drag_payload(DragVault(v.id));
            let press = theme::Press::track(ui, &resp);
            let drop_hover = resp.dnd_hover_payload::<DragFile>().is_some_and(|d| d.from != v.id)
                || resp.dnd_hover_payload::<DragVault>().is_some_and(|d| d.0 != v.id)
                || (files_hovering && pointer.is_some_and(|p| rect.contains(p)));
            crate::home::card_lift(ui, &t, rect, press);
            let fill = press.fill(t.card, t.hover, t.pressed);
            let stroke = if drop_hover {
                Stroke::new(1.5, color32(v.color))
            } else if press.hover > 0.05 {
                Stroke::new(1.0, color32(v.color).gamma_multiply(0.6))
            } else {
                Stroke::new(1.0, t.border)
            };
            ui.painter().rect(rect, CornerRadius::same(14), fill, stroke, egui::StrokeKind::Inside);
            let body = rect.translate(press.offset());
            tile(ui, Rect::from_min_size(body.min + vec2(12.0, 12.0), vec2(72.0, 72.0)), v.color, v.seed, 14);
            let w = body.width() - 106.0;
            widgets::row_text(ui, body.min + vec2(96.0, 26.0), std::borrow::Cow::<str>::Borrowed(&v.name), theme::semibold(15.0), t.text, w);
            widgets::row_text(ui, body.min + vec2(96.0, 47.0), count_label(v.files.len()), theme::regular(12.0), t.text_muted, w);
            // How far along: a thin bar of done, in progress and to do.
            let (todo, doing, done) = v.files.iter().fold((0usize, 0usize, 0usize), |(a, b, c), f| match v.meta_of(f).status {
                Status::Todo => (a + 1, b, c),
                Status::Doing => (a, b + 1, c),
                Status::Done => (a, b, c + 1),
                Status::None => (a, b, c),
            });
            let bar = Rect::from_min_size(body.min + vec2(96.0, 66.0), vec2(w.min(130.0), 6.0));
            ui.painter().rect_filled(bar, CornerRadius::same(3), t.hover);
            let total = v.files.len().max(1) as f32;
            let mut x = bar.left();
            for (n, c) in [
                (done, Color32::from_rgb(0x16, 0xA3, 0x4A)),
                (doing, Color32::from_rgb(0x25, 0x63, 0xEB)),
                (todo, Color32::from_rgb(0xD9, 0x77, 0x06)),
            ] {
                let width = bar.width() * n as f32 / total;
                if width > 0.0 {
                    ui.painter().rect_filled(Rect::from_min_size(egui::pos2(x, bar.top()), vec2(width, bar.height())), CornerRadius::same(3), c);
                    x += width;
                }
            }
            if let Some(d) = resp.dnd_release_payload::<DragFile>() {
                act = Some(Act::Move(d.from, v.id, d.path.clone()));
            } else if let Some(d) = resp.dnd_release_payload::<DragVault>() {
                act = Some(Act::Reorder(d.0, index));
            }
            let resp = theme::hand(resp).on_hover_text(tl!("Click to open it. Drag to reorder, or drop files here."));
            if resp.clicked() {
                act = Some(Act::Show(v.id));
            }
            resp.context_menu(|ui| {
                if ui.button(tl!("Open all")).clicked() {
                    act = Some(Act::OpenAll(v.id));
                }
                if ui.button(tl!("Add files…")).clicked() {
                    act = Some(Act::AddFiles(v.id));
                }
                if ui.button(tl!("Edit vault…")).clicked() {
                    act = Some(Act::Edit(v.id));
                }
                ui.separator();
                if ui.add_enabled(index > 0, egui::Button::new(tl!("Move left"))).clicked() {
                    act = Some(Act::Shift(v.id, -1));
                }
                if ui.add_enabled(index + 1 < count, egui::Button::new(tl!("Move right"))).clicked() {
                    act = Some(Act::Shift(v.id, 1));
                }
                ui.separator();
                if ui.button(tl!("Delete vault…")).clicked() {
                    act = Some(Act::Delete(v.id));
                }
            });
        }
        // New vault card.
        let (rect, resp) = ui.allocate_exact_size(vec2(170.0, 96.0), Sense::click());
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, tl!("New vault…")));
        let press = theme::Press::track(ui, &resp);
        ui.painter().rect(
            rect,
            CornerRadius::same(14),
            press.fill(t.pasteboard, t.hover, t.pressed),
            Stroke::new(1.0, t.border),
            egui::StrokeKind::Inside,
        );
        let body = rect.translate(press.offset());
        icons::paint(ui, Rect::from_center_size(body.center() - vec2(0.0, 12.0), vec2(22.0, 22.0)), "plus", 20.0, t.accent);
        ui.painter().text(body.center() + vec2(0.0, 16.0), Align2::CENTER_CENTER, tl!("New vault"), theme::medium(13.0), t.text);
        if theme::hand(resp).clicked() {
            act = Some(Act::New);
        }
    });

    app.vaults.targets = targets;
    app.vaults.targets_pass = pass;

    if let Some(act) = act {
        apply(app, act);
    }
}

/// The open vault's page, in place of Home (desktop).
pub(crate) fn page_view(app: &mut PdfCraftApp, ui: &mut egui::Ui) {
    app.refresh_vault_stats(ui.input(|i| i.time));
    let Some(v) = app.vaults.selected.and_then(|id| app.vaults.data.get(id)).cloned() else {
        app.vaults.selected = None;
        return;
    };
    let mut act = None;
    let mut targets = Vec::new();
    page::show(app, ui, &v, &mut act, &mut targets);
    app.vaults.targets = targets;
    app.vaults.targets_pass = ui.ctx().cumulative_pass_nr();
    if let Some(act) = act {
        apply(app, act);
    }
}

/// Carry out what the user did in a vault view.
fn apply(app: &mut PdfCraftApp, act: Act) {
    match act {
        Act::Select(id) => {
            app.vaults.selected = if app.vaults.selected == Some(id) { None } else { Some(id) };
            app.vaults.stale = true;
        }
        Act::Show(id) => app.show_vault(id),
        Act::SetStatus(id, path, status) => {
            app.vaults.data.set_status(id, &path, status);
        }
        Act::SetFolder(id, path, folder) => {
            app.vaults.data.set_folder(id, &path, folder);
        }
        Act::Tags(id, path) => app.vaults.dialog = Some(VaultDialog::Tags { id, path, draft: String::new() }),
        Act::Note(id, path) => {
            let text = app.vaults.data.get(id).map(|v| v.meta_of(&path).note).unwrap_or_default();
            app.vaults.dialog = Some(VaultDialog::Note { id, path, text });
        }
        Act::BinderDialog(id, binder) => {
            let name = binder.and_then(|b| app.vaults.data.get(id).and_then(|v| v.folder(b)).map(|f| f.name.clone())).unwrap_or_default();
            app.vaults.dialog = Some(VaultDialog::Binder { id, binder, name });
        }
        Act::DeleteBinder(id, binder) => {
            app.vaults.data.delete_folder(id, binder);
            app.vaults.page.binder = page::Binder::All;
        }
        Act::New => app.new_vault_dialog(Vec::new()),
        Act::Edit(id) => {
            if let Some(v) = app.vaults.data.get(id) {
                app.vaults.dialog = Some(VaultDialog::Edit {
                    id: Some(id),
                    name: v.name.clone(),
                    description: v.description.clone(),
                    color: v.color,
                    icon: v.icon,
                    seed: v.seed,
                    then_add: Vec::new(),
                });
            }
        }
        Act::Delete(id) => app.vaults.dialog = Some(VaultDialog::Delete(id)),
        Act::OpenAll(id) => app.open_vault(id),
        Act::AddFiles(id) => app.add_files_to_vault_dialog(id),
        Act::Shift(id, by) => app.vaults.data.shift(id, by),
        Act::Reorder(id, to) => app.vaults.data.move_to(id, to),
        Act::Open(path) => app.open_recent(&path),
        Act::Remove(id, path) => {
            app.vaults.data.remove_file(id, &path);
        }
        Act::Locate(id, path) => app.locate_vault_file(id, path),
        Act::Move(from, to, path) => {
            app.move_to_vault(from, to, &path);
        }
    }
}

/// Vaults in the All tools panel, so they are one click away while documents are open.
pub(crate) fn tools_panel_section(app: &mut PdfCraftApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    widgets::section_title(ui, "Vaults");
    let mut show = None;
    let mut new = false;
    for v in &app.vaults.data.list {
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 32.0), Sense::click());
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &v.name));
        let press = theme::Press::track(ui, &resp);
        press.wash(ui, rect, 6, false);
        let body = rect.translate(press.offset());
        tile(ui, Rect::from_min_size(body.min + vec2(4.0, 5.0), vec2(22.0, 22.0)), v.color, v.seed, 6);
        let count = v.files.len().to_string();
        let c = ui.painter().text(body.right_center() - vec2(8.0, 0.0), Align2::RIGHT_CENTER, count, theme::regular(11.5), t.text_faint);
        widgets::row_text(
            ui,
            body.left_center() + vec2(36.0, 0.0),
            std::borrow::Cow::<str>::Borrowed(&v.name),
            theme::regular(13.5),
            t.text,
            c.left() - body.left() - 44.0,
        );
        if theme::hand(resp).on_hover_text(tl!("Show this vault on Home")).clicked() {
            show = Some(v.id);
        }
    }
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, tl!("New vault…")));
    let press = theme::Press::track(ui, &resp);
    press.wash(ui, rect, 6, false);
    icons::paint(ui, Rect::from_min_size(rect.min + vec2(6.0, 6.0), vec2(18.0, 18.0)), "plus", 16.0, t.accent_text);
    widgets::row_text(ui, rect.left_center() + vec2(36.0, 0.0), tl!("New vault…"), theme::regular(13.0), t.accent_text, rect.width() - 40.0);
    if theme::hand(resp).clicked() {
        new = true;
    }
    if let Some(id) = show {
        app.show_vault(id);
    }
    if new {
        app.new_vault_dialog(Vec::new());
    }
}

/// Command palette entries, one per vault matching `query` (`score` ranks a label).
pub(crate) fn palette_hits(app: &PdfCraftApp, mut score: impl FnMut(&str) -> Option<usize>) -> Vec<(usize, u64, String, &'static str)> {
    app.vaults
        .data
        .list
        .iter()
        .filter_map(|v| {
            let label = crate::i18n::fmt(tl!("Vault: {name}"), &[("name", &v.name)]);
            score(&label).or_else(|| score(&v.name)).map(|s| (s + 2, v.id, label, v.icon))
        })
        .collect()
}

/// The vault dialogs (New/Edit, Delete, Add document to vault).
pub(crate) fn dialogs(app: &mut PdfCraftApp, ctx: &egui::Context) {
    let Some(dialog) = app.vaults.dialog.clone() else { return };
    let t = Tokens::get(ctx);
    if matches!(dialog, VaultDialog::Tags { .. } | VaultDialog::Note { .. } | VaultDialog::Binder { .. }) {
        page::dialog(app, ctx, dialog);
        return;
    }
    match dialog {
        VaultDialog::Edit { id, mut name, mut description, mut color, icon, mut seed, then_add } => {
            let mut done: Option<bool> = None;
            let title = if id.is_some() { tl!("Edit vault") } else { tl!("New vault") };
            let resp = widgets::show_modal(ctx, "vault-edit", |ui| {
                ui.set_width(380.0);
                ui.label(egui::RichText::new(title).font(theme::semibold(16.0)));
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(vec2(56.0, 56.0), Sense::hover());
                    tile(ui, r, color, seed, 12);
                    ui.add_space(6.0);
                    ui.vertical(|ui| {
                        let label = ui.label(egui::RichText::new(tl!("Vault name")).color(t.text_muted).font(theme::regular(12.0)));
                        let field = ui
                            .add(
                                widgets::line(&mut name)
                                    .hint_text(tl!("Vault name"))
                                    .desired_width(ui.available_width())
                                    .char_limit(vaults::MAX_NAME_CHARS),
                            )
                            .labelled_by(label.id);
                        if field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            done = Some(true);
                        }
                        if ui.memory(|m| m.focused().is_none()) {
                            field.request_focus();
                        }
                    });
                });
                ui.add_space(10.0);
                let label = ui.label(egui::RichText::new(tl!("What it is for (optional)")).color(t.text_muted).font(theme::regular(12.0)));
                ui.add(
                    widgets::line(&mut description)
                        .hint_text(tl!("Quotes and invoices for the renovation"))
                        .desired_width(f32::INFINITY)
                        .char_limit(vaults::MAX_DESCRIPTION_CHARS),
                )
                .labelled_by(label.id);
                ui.add_space(12.0);
                ui.label(egui::RichText::new(tl!("Colour")).color(t.text_muted));
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
                    for c in vaults::COLORS {
                        let (r, resp) = ui.allocate_exact_size(vec2(26.0, 26.0), Sense::click());
                        let hex = vaults::color_hex(c);
                        resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::RadioButton, true, c == color, &hex));
                        if c == color {
                            ui.painter().circle_stroke(r.center(), 12.5, Stroke::new(2.0, t.text));
                        }
                        ui.painter().circle_filled(r.center(), 9.5, color32(c));
                        if theme::hand(resp).clicked() {
                            color = c;
                        }
                    }
                });
                ui.add_space(10.0);
                ui.label(egui::RichText::new(tl!("Its monster")).color(t.text_muted));
                ui.label(
                    egui::RichText::new(tl!("Every vault has a pixel monster of its own, as unique as a QR code, so you know it at a glance."))
                        .small()
                        .color(t.text_faint),
                );
                ui.add_space(4.0);
                if widgets::ghost_button(ui, "sparkles", tl!("Another monster")).clicked() {
                    seed = crate::vault_monster::new_seed(seed);
                }
                ui.add_space(14.0);
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let ok = if id.is_some() { tl!("Save") } else { tl!("Create vault") };
                        if widgets::pill_button(ui, ok, true).clicked() {
                            done = Some(true);
                        }
                        if widgets::pill_button(ui, tl!("Cancel"), false).clicked() {
                            done = Some(false);
                        }
                    });
                });
            });
            if resp.should_close() && done.is_none() {
                done = Some(false);
            }
            match done {
                Some(true) => {
                    let ok = match id {
                        Some(id) => match app
                            .vaults
                            .data
                            .update(id, &name, color, icon)
                            .and_then(|()| app.vaults.data.set_seed(id, seed))
                            .and_then(|()| app.vaults.data.set_description(id, &description))
                        {
                            Ok(()) => true,
                            Err(e) => {
                                app.vault_error(e);
                                false
                            }
                        },
                        None => match app.create_vault(&name, color, icon) {
                            Some(new) => {
                                let _ = app.vaults.data.set_seed(new, seed);
                                let _ = app.vaults.data.set_description(new, &description);
                                if !then_add.is_empty() {
                                    app.add_to_vault(new, &then_add);
                                }
                                true
                            }
                            None => false,
                        },
                    };
                    app.vaults.dialog = if ok { None } else { Some(VaultDialog::Edit { id, name, description, color, icon, seed, then_add }) };
                }
                Some(false) => app.vaults.dialog = None,
                None => app.vaults.dialog = Some(VaultDialog::Edit { id, name, description, color, icon, seed, then_add }),
            }
        }
        VaultDialog::Delete(id) => {
            let Some(v) = app.vaults.data.get(id).cloned() else {
                app.vaults.dialog = None;
                return;
            };
            let mut done: Option<bool> = None;
            let resp = widgets::show_modal(ctx, "vault-delete", |ui| {
                ui.set_width(360.0);
                ui.label(egui::RichText::new(crate::i18n::fmt(tl!("Delete the vault {vault}?"), &[("vault", &v.name)])).font(theme::semibold(16.0)));
                ui.add_space(8.0);
                ui.label(egui::RichText::new(tl!("Its files stay where they are on your computer. Only the vault is deleted.")).color(t.text_muted));
                ui.add_space(14.0);
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if widgets::pill_button(ui, tl!("Delete vault"), true).clicked() {
                            done = Some(true);
                        }
                        if widgets::pill_button(ui, tl!("Cancel"), false).clicked() {
                            done = Some(false);
                        }
                    });
                });
            });
            if resp.should_close() && done.is_none() {
                done = Some(false);
            }
            match done {
                Some(true) => {
                    app.vaults.dialog = None;
                    app.delete_vault(id);
                }
                Some(false) => app.vaults.dialog = None,
                None => {}
            }
        }
        VaultDialog::AddDocument(path) => {
            let mut chosen: Option<Option<u64>> = None;
            let mut cancel = false;
            let resp = widgets::show_modal(ctx, "vault-add-document", |ui| {
                ui.set_width(360.0);
                ui.label(egui::RichText::new(tl!("Add to vault")).font(theme::semibold(16.0)));
                ui.label(egui::RichText::new(std::borrow::Cow::<str>::Borrowed(&vaults::file_name(&path))).color(t.text_muted));
                ui.add_space(10.0);
                egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                    for v in &app.vaults.data.list {
                        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 38.0), Sense::click());
                        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &v.name));
                        let press = theme::Press::track(ui, &resp);
                        press.wash(ui, rect, 8, false);
                        tile(ui, Rect::from_min_size(rect.min + vec2(6.0, 6.0), vec2(26.0, 26.0)), v.color, v.seed, 7);
                        let has = v.files.iter().any(|f| vaults::same_path(f, &path));
                        let c = ui.painter().text(
                            rect.right_center() - vec2(10.0, 0.0),
                            Align2::RIGHT_CENTER,
                            if has { tl!("Already in it").to_string() } else { count_label(v.files.len()) },
                            theme::regular(11.5),
                            t.text_faint,
                        );
                        widgets::row_text(
                            ui,
                            rect.left_center() + vec2(42.0, 0.0),
                            std::borrow::Cow::<str>::Borrowed(&v.name),
                            theme::medium(13.5),
                            t.text,
                            c.left() - rect.left() - 52.0,
                        );
                        if theme::hand(resp).clicked() {
                            chosen = Some(Some(v.id));
                        }
                    }
                });
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if widgets::ghost_button(ui, "plus", tl!("New vault…")).clicked() {
                        chosen = Some(None);
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if widgets::pill_button(ui, tl!("Cancel"), false).clicked() {
                            cancel = true;
                        }
                    });
                });
            });
            if resp.should_close() || cancel {
                app.vaults.dialog = None;
            }
            match chosen {
                Some(Some(id)) => {
                    app.vaults.dialog = None;
                    app.add_to_vault(id, std::slice::from_ref(&path));
                }
                Some(None) => app.new_vault_dialog(vec![path]),
                None => {}
            }
        }
        VaultDialog::Tags { .. } | VaultDialog::Note { .. } | VaultDialog::Binder { .. } => {}
    }
}
