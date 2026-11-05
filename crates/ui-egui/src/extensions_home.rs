//! NadsPDF: the extensions on Home. First the built-in ones (Vision Bubble), then one card per
//! installed extension, with its picture (the manifest's `icon`, or a pixel pattern of its own),
//! what it does and its on/off switch, and a way in for new ones: import an extension folder, or
//! open the extensions folder.
//!
//! Importing copies a checked extension into the folder ([`pdfcraft_extensions::import`]); it
//! still runs only once the user turns it on and accepts what it will be able to do.

use egui::{CornerRadius, Rect, Sense, Stroke, vec2};
use pdfcraft_extensions::{Entry, Installed};

use crate::theme::{self, Tokens};
use crate::{PdfCraftApp, icons, widgets};

/// Side of an extension's picture on its card.
const PICTURE: f32 = 44.0;

/// A pixel pattern for an extension without a picture: a 6 × 6 symmetric motif in the nads reds,
/// the same for the same id, so each extension is recognisable.
pub(crate) fn pixel_badge(ui: &egui::Ui, rect: Rect, seed: &str, t: &Tokens) {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in seed.bytes() {
        h = (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3);
    }
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(10), t.accent_soft);
    let cell = rect.width() / 8.0;
    let shades = [t.accent, t.accent_text, t.accent.gamma_multiply(0.6)];
    for row in 0..6u64 {
        for col in 0..3u64 {
            let bit = (h >> (row * 3 + col)) & 1;
            if bit == 0 {
                continue;
            }
            let shade = shades.get(((h >> (row + 20)) % 3) as usize).copied().unwrap_or(t.accent);
            for c in [col, 5 - col] {
                let min = rect.min + vec2(cell * (1.0 + c as f32), cell * (1.0 + row as f32));
                painter.rect_filled(Rect::from_min_size(min, vec2(cell, cell)), CornerRadius::ZERO, shade);
            }
        }
    }
}

/// An extension's picture, or its pixel pattern.
pub(crate) fn picture(ui: &mut egui::Ui, ext: &Installed, t: &Tokens) {
    let (rect, _) = ui.allocate_exact_size(vec2(PICTURE, PICTURE), Sense::hover());
    match &ext.icon {
        Some(png) => {
            // Keyed by the fingerprint too: an updated extension may bring a new picture.
            let key = format!("extension/{}-{}", ext.manifest.id, ext.fingerprint);
            match crate::raster_icon::texture(ui.ctx(), &key, || Some(png.clone())) {
                Some(tex) => {
                    egui::Image::new(&tex).corner_radius(CornerRadius::same(10)).paint_at(ui, rect);
                }
                None => pixel_badge(ui, rect, &ext.manifest.id, t),
            }
        }
        None => pixel_badge(ui, rect, &ext.manifest.id, t),
    }
}

/// The Extensions section of Home (desktop; extensions are off on the web).
pub(crate) fn section(app: &mut PdfCraftApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let installable = app.extensions.enabled();
    let entries = if installable { app.extensions.entries().to_vec() } else { Vec::new() };
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(tl!("Extensions")).font(theme::semibold(15.0)));
        ui.label(egui::RichText::new((entries.len() + 1).to_string()).color(t.text_faint));
        if installable {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if widgets::ghost_button(ui, "folder-open", tl!("Open the extensions folder")).clicked() {
                    app.open_extensions_folder();
                }
                if widgets::ghost_button(ui, "plus", tl!("Import an extension…")).clicked() {
                    app.import_extension_dialog();
                }
            });
        }
    });
    ui.add_space(4.0);
    ui.label(
        egui::RichText::new(tl!("Programs that add to NadsPDF. An extension runs only once you turn it on, and only does what it asked for."))
            .color(t.text_muted),
    );
    ui.add_space(10.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(12.0, 12.0);
        vision_card(app, ui, &t);
        for entry in &entries {
            card(app, ui, &t, entry);
        }
    });
    if installable && entries.is_empty() {
        ui.add_space(12.0);
        empty_state(ui, &t);
    }
}

/// Vision Bubble's picture: a speech bubble holding an eye, in nads pixels.
fn vision_picture(ui: &mut egui::Ui, t: &Tokens, on: bool) {
    const ROWS: [&str; 9] =
        ["..#######..", ".#.......#.", "#...###...#", "#..#.#.#..#", "#.#..#..#.#", "#..#.#.#..#", "#...###...#", ".#.......#.", "..###.###.."];
    let (rect, _) = ui.allocate_exact_size(vec2(PICTURE, PICTURE), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(10), if on { t.accent_soft } else { t.hover });
    let cell = (rect.width() - 8.0) / 11.0;
    let color = if on { t.accent } else { t.text_faint };
    for (row, line) in ROWS.iter().enumerate() {
        for (col, ch) in line.chars().enumerate() {
            if ch != '#' {
                continue;
            }
            let min = rect.min + vec2(4.0 + cell * col as f32, 4.0 + cell * (row as f32 + 1.0));
            painter.rect_filled(Rect::from_min_size(min, vec2(cell, cell)), CornerRadius::ZERO, color);
        }
    }
}

/// The built-in Vision Bubble: what it does, what it sends, and its switch.
fn vision_card(app: &mut PdfCraftApp, ui: &mut egui::Ui, t: &Tokens) {
    egui::Frame::NONE
        .fill(t.card)
        .stroke(Stroke::new(1.0, if app.link_vision { t.accent.gamma_multiply(0.55) } else { t.border }))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(egui::Margin::same(12))
        .show(ui, |ui| {
            ui.set_width(340.0);
            ui.horizontal_top(|ui| {
                vision_picture(ui, t, app.link_vision);
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(tl!("Vision Bubble")).font(theme::semibold(13.5)));
                        ui.label(egui::RichText::new(tl!("Built in")).small().color(t.text_faint));
                    });
                    ui.label(
                        egui::RichText::new(tl!(
                            "Point at a link to see the website's icon and a preview of the site: its title, description and picture."
                        ))
                        .color(t.text_muted),
                    );
                    ui.label(
                        egui::RichText::new(tl!("Reads the home page of each site you point at, which tells that site.")).small().color(t.text_faint),
                    );
                    ui.add_space(4.0);
                    let (label, on) = if app.link_vision { (tl!("Turn off"), false) } else { (tl!("Turn on"), true) };
                    if widgets::pill_button(ui, label, on).clicked() {
                        app.set_link_vision(on);
                    }
                });
            });
        });
}

/// No extension yet: what one is, so the import button makes sense.
fn empty_state(ui: &mut egui::Ui, t: &Tokens) {
    egui::Frame::NONE
        .fill(t.card)
        .stroke(Stroke::new(1.0, t.border))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(egui::Margin::same(16))
        .show(ui, |ui| {
            ui.set_width(ui.available_width().min(620.0));
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(vec2(PICTURE, PICTURE), Sense::hover());
                pixel_badge(ui, rect, "nadspdf", t);
                ui.vertical(|ui| {
                    ui.label(egui::RichText::new(tl!("No extension installed")).font(theme::semibold(13.5)));
                    ui.label(
                        egui::RichText::new(tl!("An extension is a folder with a manifest.json and its program. Import one to add it here."))
                            .color(t.text_muted),
                    );
                });
            });
        });
}

/// One extension's card: its picture beside its name, description, state and switch.
fn card(app: &mut PdfCraftApp, ui: &mut egui::Ui, t: &Tokens, entry: &Entry) {
    egui::Frame::NONE
        .fill(t.card)
        .stroke(Stroke::new(1.0, t.border))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(egui::Margin::same(12))
        .show(ui, |ui| {
            ui.set_width(340.0);
            ui.horizontal_top(|ui| match entry {
                Entry::Ok(ext) => {
                    picture(ui, ext, t);
                    ui.vertical(|ui| crate::extensions_ui::extension_row(ui, app, t, ext));
                }
                Entry::Broken { dir, error } => {
                    let (rect, _) = ui.allocate_exact_size(vec2(PICTURE, PICTURE), Sense::hover());
                    ui.painter().rect_filled(rect, CornerRadius::same(10), t.hover);
                    icons::paint(ui, rect.shrink(12.0), "circle-help", 18.0, t.text_faint);
                    ui.vertical(|ui| {
                        let folder = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                        ui.label(egui::RichText::new(folder).strong());
                        ui.label(
                            egui::RichText::new(crate::i18n::fmt(tl!("This extension can't be used: {e}"), &[("e", error)]))
                                .small()
                                .color(t.text_muted),
                        );
                    });
                }
            });
        });
}

impl PdfCraftApp {
    /// Home ▸ Extensions ▸ Import: pick an extension folder, check it and copy it in.
    pub(crate) fn import_extension_dialog(&mut self) {
        let dialog = rfd::AsyncFileDialog::new().set_title(tl!("Import an extension"));
        self.ask_one(crate::pickers::Ask::Folder(dialog), None, |app, dir| app.import_extension(&dir));
    }

    /// Copy the extension in `dir` into the extensions folder and read the folder again.
    pub fn import_extension(&mut self, dir: &std::path::Path) {
        let Some(folder) = self.extensions.folder().map(std::path::Path::to_path_buf) else {
            self.notify_tr("Extensions are not available in this copy of NadsPDF.");
            return;
        };
        match pdfcraft_extensions::import(dir, &folder) {
            Ok(ext) => {
                self.notify_fmt("{name} is installed. Turn it on to use it.", &[("name", &ext.manifest.name)]);
                crate::extensions_ui::rescan(self);
            }
            Err(e) => self.notify_fmt("Couldn't import this extension: {e}", &[("e", &e)]),
        }
    }

    /// Show the extensions folder in the file manager (created first if needed).
    pub(crate) fn open_extensions_folder(&mut self) {
        let Some(folder) = self.extensions.folder().map(std::path::Path::to_path_buf) else { return };
        if let Err(e) = std::fs::create_dir_all(&folder) {
            self.notify_fmt("Couldn't open the extensions folder: {e}", &[("e", &e.to_string())]);
            return;
        }
        if let Err(e) = crate::tab_context::reveal(&folder) {
            self.notify_fmt("Couldn't open the extensions folder: {e}", &[("e", &e)]);
        }
    }
}
