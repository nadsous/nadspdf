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
use crate::vaults::{self, Added, FileStat, VaultError, Vaults};
use crate::{icons, widgets};

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
    Edit { id: Option<u64>, name: String, color: [u8; 3], icon: &'static str, then_add: Vec<String> },
    /// Confirm deleting a vault.
    Delete(u64),
    /// Choose the vault a document joins.
    AddDocument(String),
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

/// The vault's colour at about 20 % over the card, behind its icon.
fn tint(c: [u8; 3]) -> Color32 {
    Color32::from_rgba_unmultiplied(c[0], c[1], c[2], 52)
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
        self.vaults.dialog = Some(VaultDialog::Edit { id: None, name: String::new(), color, icon: "folder", then_add });
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
            let dialog = rfd::AsyncFileDialog::new()
                .set_title(tl!("Add files…").to_string())
                .add_filter("PDF", &["pdf"])
                .add_filter(tl!("Images, text and documents (converted to PDF)"), &crate::create_ui::CONVERTIBLE);
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
    Select(u64),
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

/// A vault's tile: its colour at about 20 % with the icon in full colour.
fn tile(ui: &egui::Ui, rect: Rect, color: [u8; 3], icon: &str, radius: u8) {
    ui.painter().rect_filled(rect, CornerRadius::same(radius), tint(color));
    let size = (rect.width() * 0.52).round();
    icons::paint(ui, rect, icon, size, color32(color));
}

/// A small text button painted inside a row.
fn row_button(ui: &mut egui::Ui, t: &Tokens, rect: Rect, id: egui::Id, label: &str, strong: bool) -> egui::Response {
    let resp = ui.interact(rect, id, Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
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
        ui.spacing_mut().item_spacing = vec2(10.0, 10.0);
        let count = app.vaults.data.list.len();
        for (index, v) in app.vaults.data.list.iter().enumerate() {
            let selected = app.vaults.selected == Some(v.id);
            let (rect, resp) = ui.allocate_exact_size(vec2(200.0, 60.0), Sense::click_and_drag());
            targets.push((v.id, rect));
            resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, selected, &v.name));
            resp.dnd_set_drag_payload(DragVault(v.id));
            let press = theme::Press::track(ui, &resp);
            let drop_hover = resp.dnd_hover_payload::<DragFile>().is_some_and(|d| d.from != v.id)
                || resp.dnd_hover_payload::<DragVault>().is_some_and(|d| d.0 != v.id)
                || (files_hovering && pointer.is_some_and(|p| rect.contains(p)));
            let fill = press.fill(t.card, t.hover, t.pressed);
            let stroke = if selected || drop_hover {
                Stroke::new(1.5, color32(v.color))
            } else if press.hover > 0.05 {
                Stroke::new(1.0, color32(v.color).gamma_multiply(0.6))
            } else {
                Stroke::new(1.0, t.border)
            };
            ui.painter().rect(rect, CornerRadius::same(12), fill, stroke, egui::StrokeKind::Inside);
            let body = rect.translate(press.offset());
            tile(ui, Rect::from_min_size(body.min + vec2(12.0, 12.0), vec2(36.0, 36.0)), v.color, v.icon, 9);
            let w = body.width() - 70.0;
            widgets::row_text(ui, body.min + vec2(58.0, 22.0), crate::bidi::visual(&v.name), theme::semibold(13.5), t.text, w);
            widgets::row_text(ui, body.min + vec2(58.0, 40.0), count_label(v.files.len()), theme::regular(11.5), t.text_muted, w);
            if let Some(d) = resp.dnd_release_payload::<DragFile>() {
                act = Some(Act::Move(d.from, v.id, d.path.clone()));
            } else if let Some(d) = resp.dnd_release_payload::<DragVault>() {
                act = Some(Act::Reorder(d.0, index));
            }
            let resp = theme::hand(resp).on_hover_text(tl!("Click to show its files. Drag to reorder, or drop files here."));
            if resp.clicked() {
                act = Some(Act::Select(v.id));
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
        let (rect, resp) = ui.allocate_exact_size(vec2(150.0, 60.0), Sense::click());
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, tl!("New vault…")));
        let press = theme::Press::track(ui, &resp);
        ui.painter().rect(
            rect,
            CornerRadius::same(12),
            press.fill(t.pasteboard, t.hover, t.pressed),
            Stroke::new(1.0, t.border),
            egui::StrokeKind::Inside,
        );
        let body = rect.translate(press.offset());
        icons::paint(ui, Rect::from_center_size(body.left_center() + vec2(26.0, 0.0), vec2(20.0, 20.0)), "plus", 18.0, t.accent);
        widgets::row_text(ui, body.left_center() + vec2(44.0, 0.0), tl!("New vault"), theme::medium(13.0), t.text, body.width() - 52.0);
        if theme::hand(resp).clicked() {
            act = Some(Act::New);
        }
    });

    if let Some(v) = app.vaults.selected.and_then(|id| app.vaults.data.get(id)).cloned() {
        ui.add_space(12.0);
        let others: Vec<(u64, String, [u8; 3], &'static str)> =
            app.vaults.data.list.iter().filter(|o| o.id != v.id).map(|o| (o.id, o.name.clone(), o.color, o.icon)).collect();
        let frame = egui::Frame::NONE
            .fill(t.card)
            .stroke(Stroke::new(1.0, t.border))
            .corner_radius(CornerRadius::same(12))
            .inner_margin(egui::Margin::same(16))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(vec2(40.0, 40.0), Sense::hover());
                    tile(ui, r, v.color, v.icon, 10);
                    ui.add_space(4.0);
                    ui.vertical(|ui| {
                        ui.label(egui::RichText::new(crate::bidi::visual(&v.name)).font(theme::semibold(16.0)));
                        ui.label(egui::RichText::new(count_label(v.files.len())).color(t.text_muted).font(theme::regular(12.0)));
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if widgets::ghost_button(ui, "trash-2", tl!("Delete")).on_hover_text(tl!("Delete vault…")).clicked() {
                            act = Some(Act::Delete(v.id));
                        }
                        if widgets::ghost_button(ui, "pencil", tl!("Edit vault")).clicked() {
                            act = Some(Act::Edit(v.id));
                        }
                        if !v.files.is_empty() && widgets::ghost_button(ui, "files", tl!("Open all")).clicked() {
                            act = Some(Act::OpenAll(v.id));
                        }
                        if widgets::ghost_button(ui, "file-plus", tl!("Add files…")).clicked() {
                            act = Some(Act::AddFiles(v.id));
                        }
                    });
                });
                ui.add_space(8.0);
                if v.files.is_empty() {
                    ui.add_space(6.0);
                    ui.label(egui::RichText::new(tl!("No files yet. Drop files here to add them.")).color(t.text_muted));
                    ui.add_space(6.0);
                }
                for f in &v.files {
                    let stat = app.vaults.stat(f);
                    let missing = stat.is_some_and(|s| !s.exists);
                    let name = vaults::file_name(f);
                    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 46.0), Sense::click_and_drag());
                    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, !missing, &name));
                    if !missing {
                        resp.dnd_set_drag_payload(DragFile { from: v.id, path: f.clone() });
                    }
                    let hovered = ui.rect_contains_pointer(rect);
                    if hovered {
                        ui.painter().rect_filled(rect, CornerRadius::same(8), t.hover);
                    }
                    let icon_color = if missing { t.text_faint } else { color32(v.color) };
                    icons::paint(ui, Rect::from_min_size(rect.min + vec2(10.0, 11.0), vec2(24.0, 24.0)), "file-text", 21.0, icon_color);
                    // The right side: missing-file actions, or the date and size and a remove button.
                    let mut right = rect.right() - 10.0;
                    if missing {
                        let font = theme::medium(12.0);
                        for (label, strong, is_locate) in [(tl!("Remove"), false, false), (tl!("Locate…"), true, true)] {
                            let w = text_w(ui, label, &font) + 20.0;
                            let r = Rect::from_min_max(egui::pos2(right - w, rect.center().y - 13.0), egui::pos2(right, rect.center().y + 13.0));
                            let id = ui.id().with(("vault-file", label, f));
                            if row_button(ui, &t, r, id, label, strong).clicked() {
                                act = Some(if is_locate { Act::Locate(v.id, f.clone()) } else { Act::Remove(v.id, f.clone()) });
                            }
                            right = r.left() - 8.0;
                        }
                        let tag = ui.painter().text(
                            egui::pos2(right, rect.center().y),
                            Align2::RIGHT_CENTER,
                            tl!("Missing"),
                            theme::medium(12.0),
                            t.accent_text,
                        );
                        right = tag.left();
                    } else {
                        if hovered {
                            let x_rect = Rect::from_center_size(egui::pos2(right - 14.0, rect.center().y), vec2(28.0, 28.0));
                            let x = ui.interact(x_rect, ui.id().with(("vault-remove", f)), Sense::click());
                            let label = crate::i18n::fmt(tl!("Remove {name} from this vault"), &[("name", &name)]);
                            x.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
                            if x.hovered() {
                                ui.painter().rect_filled(x_rect, CornerRadius::same(6), t.pressed);
                            }
                            icons::paint(ui, x_rect.shrink(6.0), "x", 16.0, t.icon);
                            if x.on_hover_text(tl!("Remove from vault")).clicked() {
                                act = Some(Act::Remove(v.id, f.clone()));
                            }
                        }
                        right -= 38.0;
                        if let Some(s) = stat {
                            let size = crate::panels::human_size(usize::try_from(s.size).unwrap_or(usize::MAX));
                            let when = s.modified.map(crate::combine_ui::ago).unwrap_or_default();
                            let detail = if when.is_empty() { size } else { format!("{when}  ·  {size}") };
                            let d = ui.painter().text(
                                egui::pos2(right, rect.center().y),
                                Align2::RIGHT_CENTER,
                                detail,
                                theme::regular(12.0),
                                t.text_muted,
                            );
                            right = d.left();
                        }
                    }
                    let width = right - 16.0 - (rect.left() + 46.0);
                    let name_color = if missing { t.text_faint } else { t.text };
                    widgets::row_text(ui, rect.min + vec2(46.0, 15.0), crate::bidi::visual(&name), theme::medium(13.5), name_color, width);
                    widgets::row_text(
                        ui,
                        rect.min + vec2(46.0, 32.0),
                        crate::bidi::visual(&crate::folders_ui::location(f)),
                        theme::regular(11.0),
                        t.text_faint,
                        width,
                    );
                    let resp = if missing {
                        resp.on_hover_text(tl!("This file has moved or been deleted. Locate it, or remove it from the vault."))
                    } else {
                        theme::hand(resp).on_hover_text(f)
                    };
                    if resp.clicked() && !missing {
                        act = Some(Act::Open(f.clone()));
                    }
                    resp.context_menu(|ui| {
                        if !missing && ui.button(tl!("Open")).clicked() {
                            act = Some(Act::Open(f.clone()));
                        }
                        if missing && ui.button(tl!("Locate…")).clicked() {
                            act = Some(Act::Locate(v.id, f.clone()));
                        }
                        if !others.is_empty() {
                            ui.menu_button(tl!("Move to vault"), |ui| {
                                for (id, name, _, _) in &others {
                                    if ui.button(crate::bidi::visual(name)).clicked() {
                                        act = Some(Act::Move(v.id, *id, f.clone()));
                                    }
                                }
                            });
                        }
                        if ui.button(tl!("Remove from vault")).clicked() {
                            act = Some(Act::Remove(v.id, f.clone()));
                        }
                    });
                }
            });
        targets.insert(0, (v.id, frame.response.rect));
        if files_hovering && pointer.is_some_and(|p| frame.response.rect.contains(p)) {
            ui.painter().rect_stroke(frame.response.rect, CornerRadius::same(12), Stroke::new(1.5, color32(v.color)), egui::StrokeKind::Inside);
        }
    }
    app.vaults.targets = targets;
    app.vaults.targets_pass = pass;

    let Some(act) = act else { return };
    match act {
        Act::Select(id) => {
            app.vaults.selected = if app.vaults.selected == Some(id) { None } else { Some(id) };
            app.vaults.stale = true;
        }
        Act::New => app.new_vault_dialog(Vec::new()),
        Act::Edit(id) => {
            if let Some(v) = app.vaults.data.get(id) {
                app.vaults.dialog =
                    Some(VaultDialog::Edit { id: Some(id), name: v.name.clone(), color: v.color, icon: v.icon, then_add: Vec::new() });
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
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &v.name));
        let press = theme::Press::track(ui, &resp);
        press.wash(ui, rect, 6, false);
        let body = rect.translate(press.offset());
        tile(ui, Rect::from_min_size(body.min + vec2(4.0, 5.0), vec2(22.0, 22.0)), v.color, v.icon, 6);
        let count = v.files.len().to_string();
        let c = ui.painter().text(body.right_center() - vec2(8.0, 0.0), Align2::RIGHT_CENTER, count, theme::regular(11.5), t.text_faint);
        widgets::row_text(
            ui,
            body.left_center() + vec2(36.0, 0.0),
            crate::bidi::visual(&v.name),
            theme::regular(13.5),
            t.text,
            c.left() - body.left() - 44.0,
        );
        if theme::hand(resp).on_hover_text(tl!("Show this vault on Home")).clicked() {
            show = Some(v.id);
        }
    }
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, tl!("New vault…")));
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
    match dialog {
        VaultDialog::Edit { id, mut name, mut color, mut icon, then_add } => {
            let mut done: Option<bool> = None;
            let title = if id.is_some() { tl!("Edit vault") } else { tl!("New vault") };
            let resp = widgets::show_modal(ctx, "vault-edit", |ui| {
                ui.set_width(380.0);
                ui.label(egui::RichText::new(title).font(theme::semibold(16.0)));
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(vec2(44.0, 44.0), Sense::hover());
                    tile(ui, r, color, icon, 11);
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
                ui.add_space(12.0);
                ui.label(egui::RichText::new(tl!("Colour")).color(t.text_muted));
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
                    for c in vaults::COLORS {
                        let (r, resp) = ui.allocate_exact_size(vec2(26.0, 26.0), Sense::click());
                        let hex = vaults::color_hex(c);
                        resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::RadioButton, true, c == color, &hex));
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
                ui.label(egui::RichText::new(tl!("Icon")).color(t.text_muted));
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
                    for i in vaults::ICONS {
                        let (r, resp) = ui.allocate_exact_size(vec2(36.0, 36.0), Sense::click());
                        resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::RadioButton, true, i == icon, i));
                        let fill = if i == icon {
                            tint(color)
                        } else if resp.hovered() {
                            t.hover
                        } else {
                            Color32::TRANSPARENT
                        };
                        ui.painter().rect_filled(r, CornerRadius::same(8), fill);
                        icons::paint(ui, r, i, 18.0, if i == icon { color32(color) } else { t.icon });
                        if theme::hand(resp).clicked() {
                            icon = i;
                        }
                    }
                });
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
                        Some(id) => match app.vaults.data.update(id, &name, color, icon) {
                            Ok(()) => true,
                            Err(e) => {
                                app.vault_error(e);
                                false
                            }
                        },
                        None => match app.create_vault(&name, color, icon) {
                            Some(new) => {
                                if !then_add.is_empty() {
                                    app.add_to_vault(new, &then_add);
                                }
                                true
                            }
                            None => false,
                        },
                    };
                    app.vaults.dialog = if ok { None } else { Some(VaultDialog::Edit { id, name, color, icon, then_add }) };
                }
                Some(false) => app.vaults.dialog = None,
                None => app.vaults.dialog = Some(VaultDialog::Edit { id, name, color, icon, then_add }),
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
                ui.label(egui::RichText::new(crate::bidi::visual(&vaults::file_name(&path))).color(t.text_muted));
                ui.add_space(10.0);
                egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                    for v in &app.vaults.data.list {
                        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 38.0), Sense::click());
                        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &v.name));
                        let press = theme::Press::track(ui, &resp);
                        press.wash(ui, rect, 8, false);
                        tile(ui, Rect::from_min_size(rect.min + vec2(6.0, 6.0), vec2(26.0, 26.0)), v.color, v.icon, 7);
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
                            crate::bidi::visual(&v.name),
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
    }
}
