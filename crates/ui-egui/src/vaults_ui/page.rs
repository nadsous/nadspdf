//! A vault's own page, in place of Home while it is open: its monster and what it holds at the
//! top; a search box, status and tag filters, the sort and the view; its binders down the side
//! (drop a file on one to file it there, or on another vault to move it); and its files as
//! cards with a picture of their first page, or as a list. Every file has a status, tags, a
//! note and a binder, set from its menu. Files stay where they are on the computer.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use egui::{Align2, Color32, CornerRadius, Rect, Sense, Stroke, vec2};

use super::{Act, DragFile, VaultDialog, count_label, text_w};
use crate::PdfCraftApp;
use crate::theme::{self, Tokens};
use crate::vaults::{self, FileMeta, Status, Vault};
use crate::{icons, widgets};

/// A file card, and the picture of its first page on it.
const CARD_W: f32 = 184.0;
const CARD_H: f32 = 262.0;
const THUMB_H: f32 = 168.0;
/// The binder column's width.
const SIDE_W: f32 = 214.0;
/// First-page pictures made at once, and the largest file read for one.
const THUMB_WORKERS: usize = 2;
const MAX_THUMB_FILE: u64 = 200 * 1024 * 1024;

/// How the files are ordered.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Sort {
    /// Most recently added first.
    #[default]
    Added,
    Name,
    /// Most recently changed on disk first.
    Modified,
    /// Largest first.
    Size,
}

/// Which binder's files show.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Binder {
    #[default]
    All,
    /// Files in no binder.
    Loose,
    One(u64),
}

#[derive(Debug)]
enum Thumb {
    Pending,
    Ready(egui::ColorImage),
    Failed,
}

/// First-page pictures: made on worker threads, kept as textures for the session.
#[derive(Default)]
pub struct Thumbs {
    made: Arc<Mutex<HashMap<String, Thumb>>>,
    textures: HashMap<String, egui::TextureHandle>,
}

impl std::fmt::Debug for Thumbs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Thumbs").field("textures", &self.textures.len()).finish_non_exhaustive()
    }
}

/// What the page shows: filters, sort, view; reset when another vault opens.
#[derive(Debug, Default)]
pub struct PageState {
    for_vault: Option<u64>,
    pub query: String,
    pub status: Option<Status>,
    pub tag: Option<String>,
    pub binder: Binder,
    pub sort: Sort,
    /// The list instead of the cards.
    pub list: bool,
    thumbs: Thumbs,
}

pub(super) fn status_label(s: Status) -> &'static str {
    match s {
        Status::None => tl!("No status"),
        Status::Todo => tl!("To do"),
        Status::Doing => tl!("In progress"),
        Status::Done => tl!("Done"),
    }
}

fn status_color(s: Status) -> Color32 {
    match s {
        Status::None => Color32::from_rgb(0x94, 0xA3, 0xB8),
        Status::Todo => Color32::from_rgb(0xD9, 0x77, 0x06),
        Status::Doing => Color32::from_rgb(0x25, 0x63, 0xEB),
        Status::Done => Color32::from_rgb(0x16, 0xA3, 0x4A),
    }
}

fn status_icon(s: Status) -> &'static str {
    match s {
        Status::None => "circle-dashed",
        Status::Todo => "circle-dot",
        Status::Doing => "clock",
        Status::Done => "circle-check",
    }
}

/// A small coloured label: a status or a tag.
fn chip(ui: &egui::Ui, pos: egui::Pos2, text: &str, color: Color32, t: &Tokens, max_w: f32) -> f32 {
    let font = theme::medium(11.0);
    let w = (text_w(ui, text, &font) + 14.0).min(max_w);
    if w < 24.0 {
        return 0.0;
    }
    let rect = Rect::from_min_size(pos, vec2(w, 20.0));
    ui.painter().rect_filled(rect, CornerRadius::same(10), color.gamma_multiply(0.16));
    widgets::row_text(ui, rect.left_center() + vec2(7.0, 0.0), text.to_string(), font, color, w - 12.0);
    let _ = t;
    w
}

impl PdfCraftApp {
    /// The picture of `path`'s first page, once made; starts making it if not asked yet.
    fn vault_thumb(&mut self, ctx: &egui::Context, path: &str) -> Option<egui::TextureHandle> {
        let thumbs = &mut self.vaults.page.thumbs;
        if let Some(tex) = thumbs.textures.get(path) {
            return Some(tex.clone());
        }
        let mut made = thumbs.made.lock().unwrap_or_else(PoisonError::into_inner);
        match made.get(path) {
            Some(Thumb::Ready(_)) => {
                let Some(Thumb::Ready(img)) = made.insert(path.to_string(), Thumb::Failed) else { return None };
                drop(made);
                let tex = ctx.load_texture(format!("vault-thumb/{path}"), img, egui::TextureOptions::LINEAR);
                thumbs.textures.insert(path.to_string(), tex.clone());
                return Some(tex);
            }
            Some(Thumb::Pending | Thumb::Failed) => return None,
            None => {}
        }
        let busy = made.values().filter(|m| matches!(m, Thumb::Pending)).count();
        if busy >= THUMB_WORKERS || !path.to_lowercase().ends_with(".pdf") {
            if !path.to_lowercase().ends_with(".pdf") {
                made.insert(path.to_string(), Thumb::Failed);
            }
            return None;
        }
        made.insert(path.to_string(), Thumb::Pending);
        drop(made);
        let (shared, owned, waker) = (thumbs.made.clone(), path.to_string(), ctx.clone());
        let work = move || {
            let thumb = first_page(&owned).map_or(Thumb::Failed, Thumb::Ready);
            shared.lock().unwrap_or_else(PoisonError::into_inner).insert(owned, thumb);
            waker.request_repaint();
        };
        if std::thread::Builder::new().name("nadspdf-vault-thumb".into()).spawn(work).is_err() {
            thumbs.made.lock().unwrap_or_else(PoisonError::into_inner).insert(path.to_string(), Thumb::Failed);
        }
        None
    }
}

/// A picture of the first page of the PDF at `path`, about the size of a card's picture (twice
/// that, for sharp screens). `None` for a file that can't be read or drawn (too large, locked
/// with a password, damaged).
fn first_page(path: &str) -> Option<egui::ColorImage> {
    let len = std::fs::metadata(path).ok()?.len();
    if len == 0 || len > MAX_THUMB_FILE {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    let mut r = pdfcraft_render::PageRenderer::new(Arc::new(bytes), pdfcraft_render::RenderConfig::default());
    if r.page_count() == 0 {
        return None;
    }
    // A tiny render first, for the page's shape, so a poster-sized page is never drawn large.
    let probe_scale = 0.05_f32;
    let probe = r.render(pdfcraft_render::RenderRequest { page: 0, scale: probe_scale, preview: true, ..Default::default() });
    if probe.error.is_some() || probe.width == 0 || probe.height == 0 {
        return None;
    }
    let (pw, ph) = (probe.width as f32 / probe_scale, probe.height as f32 / probe_scale);
    let scale = ((CARD_W * 2.0) / pw).min((THUMB_H * 2.0) / ph).clamp(0.005, 2.0);
    let out = r.render(pdfcraft_render::RenderRequest { page: 0, scale, preview: true, ..Default::default() });
    let size = [usize::try_from(out.width).ok()?, usize::try_from(out.height).ok()?];
    if out.error.is_some() || size[0] == 0 || size[1] == 0 || out.rgba.len() != size[0].checked_mul(size[1])?.checked_mul(4)? {
        return None;
    }
    Some(egui::ColorImage::from_rgba_premultiplied(size, &out.rgba))
}

/// The files `v` shows with the page's filters, in its order.
fn shown_files(app: &PdfCraftApp, v: &Vault) -> Vec<(String, FileMeta)> {
    let p = &app.vaults.page;
    let query = p.query.trim().to_lowercase();
    let mut files: Vec<(String, FileMeta)> = v
        .files
        .iter()
        .map(|f| (f.clone(), v.meta_of(f)))
        .filter(|(_, m)| match p.binder {
            Binder::All => true,
            Binder::Loose => m.folder.is_none(),
            Binder::One(id) => m.folder == Some(id),
        })
        .filter(|(_, m)| p.status.is_none_or(|s| m.status == s))
        .filter(|(_, m)| p.tag.as_ref().is_none_or(|t| m.tags.iter().any(|x| x.to_lowercase() == t.to_lowercase())))
        .filter(|(f, m)| {
            query.is_empty()
                || vaults::file_name(f).to_lowercase().contains(&query)
                || m.note.to_lowercase().contains(&query)
                || m.tags.iter().any(|t| t.to_lowercase().contains(&query))
        })
        .collect();
    let stat = |f: &str| app.vaults.stat(f);
    match p.sort {
        Sort::Added => files.sort_by_key(|(_, m)| std::cmp::Reverse(m.added)),
        Sort::Name => files.sort_by_key(|(f, _)| vaults::file_name(f).to_lowercase()),
        Sort::Modified => files.sort_by(|a, b| stat(&b.0).and_then(|s| s.modified).cmp(&stat(&a.0).and_then(|s| s.modified))),
        Sort::Size => files.sort_by(|a, b| stat(&b.0).map_or(0, |s| s.size).cmp(&stat(&a.0).map_or(0, |s| s.size))),
    }
    files
}

/// Draw the open vault's page. Returns what the user did, applied by the caller.
pub(super) fn show(app: &mut PdfCraftApp, ui: &mut egui::Ui, v: &Vault, act: &mut Option<Act>, targets: &mut Vec<(u64, Rect)>) {
    let t = Tokens::get(ui.ctx());
    if app.vaults.page.for_vault != Some(v.id) {
        let thumbs = std::mem::take(&mut app.vaults.page.thumbs);
        app.vaults.page = PageState { for_vault: Some(v.id), thumbs, ..PageState::default() };
    }
    let files_hovering = ui.input(|i| !i.raw.hovered_files.is_empty());
    let pointer = ui.input(|i| i.pointer.latest_pos());
    let page_top = ui.cursor().min;

    // Back to all vaults.
    if widgets::ghost_button(ui, "chevron-left", tl!("All vaults")).clicked() {
        *act = Some(Act::Select(v.id));
    }
    ui.add_space(8.0);
    header(app, ui, &t, v, act);
    ui.add_space(14.0);
    toolbar(app, ui, &t, v);
    ui.add_space(12.0);

    let shown = shown_files(app, v);
    let others: Vec<(u64, String, [u8; 3], u64)> =
        app.vaults.data.list.iter().filter(|o| o.id != v.id).map(|o| (o.id, o.name.clone(), o.color, o.seed)).collect();
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(SIDE_W);
            sidebar(app, ui, &t, v, &others, act, targets);
        });
        ui.add_space(18.0);
        ui.vertical(|ui| {
            ui.set_width(ui.available_width());
            if v.files.is_empty() {
                empty(ui, &t, tl!("No files yet. Drop files here to add them."));
            } else if shown.is_empty() {
                empty(ui, &t, tl!("No file matches. Change the search or the filters."));
            } else if app.vaults.page.list {
                for (f, m) in &shown {
                    row(app, ui, &t, v, f, m, &others, act);
                }
            } else {
                let columns = ((ui.available_width() + 14.0) / (CARD_W + 14.0)).floor().max(1.0) as usize;
                for chunk in shown.chunks(columns) {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing = vec2(14.0, 0.0);
                        for (f, m) in chunk {
                            card(app, ui, &t, v, f, m, &others, act);
                        }
                    });
                    ui.add_space(14.0);
                }
            }
        });
    });
    // Files dropped anywhere on the page join this vault.
    let page = Rect::from_min_max(page_top, ui.min_rect().max);
    targets.push((v.id, page));
    if files_hovering && pointer.is_some_and(|p| page.contains(p)) {
        ui.painter().rect_stroke(page.expand(6.0), CornerRadius::same(14), Stroke::new(1.5, super::color32(v.color)), egui::StrokeKind::Inside);
    }
}

fn empty(ui: &mut egui::Ui, t: &Tokens, text: &str) {
    ui.add_space(40.0);
    ui.vertical_centered(|ui| {
        icons::paint(ui, Rect::from_center_size(ui.cursor().center_top() + vec2(0.0, 18.0), vec2(30.0, 30.0)), "folder-open", 28.0, t.text_faint);
        ui.add_space(42.0);
        ui.label(egui::RichText::new(text).color(t.text_muted));
    });
}

/// The vault's monster, name, description and what it holds, with its actions.
fn header(app: &PdfCraftApp, ui: &mut egui::Ui, t: &Tokens, v: &Vault, act: &mut Option<Act>) {
    let color = super::color32(v.color);
    egui::Frame::NONE
        .fill(t.card)
        .stroke(Stroke::new(1.0, color.gamma_multiply(0.45)))
        .corner_radius(CornerRadius::same(16))
        .inner_margin(egui::Margin::same(18))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(vec2(96.0, 96.0), Sense::hover());
                crate::vault_monster::paint(ui.painter(), r, v.seed, v.color, 18);
                ui.add_space(10.0);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 4.0;
                    ui.label(egui::RichText::new(std::borrow::Cow::<str>::Borrowed(&v.name)).font(theme::semibold(22.0)));
                    if !v.description.is_empty() {
                        ui.label(egui::RichText::new(std::borrow::Cow::<str>::Borrowed(&v.description)).color(t.text_muted));
                    }
                    // What it holds: files, size on disk, and how far along they are.
                    let size: u64 = v.files.iter().filter_map(|f| app.vaults.stat(f)).map(|s| s.size).sum();
                    let mut parts = vec![count_label(v.files.len())];
                    if size > 0 {
                        parts.push(crate::panels::human_size(usize::try_from(size).unwrap_or(usize::MAX)));
                    }
                    ui.label(egui::RichText::new(parts.join("  ·  ")).color(t.text_faint).font(theme::regular(12.5)));
                    ui.horizontal(|ui| {
                        for s in [Status::Todo, Status::Doing, Status::Done] {
                            let n = v.files.iter().filter(|f| v.meta_of(f).status == s).count();
                            if n == 0 {
                                continue;
                            }
                            let label = format!("{} {n}", status_label(s));
                            let (r, _) = ui.allocate_exact_size(vec2(text_w(ui, &label, &theme::medium(11.0)) + 16.0, 20.0), Sense::hover());
                            chip(ui, r.min, &label, status_color(s), t, 400.0);
                        }
                    });
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                    if widgets::ghost_button(ui, "trash-2", tl!("Delete")).on_hover_text(tl!("Delete vault…")).clicked() {
                        *act = Some(Act::Delete(v.id));
                    }
                    if widgets::ghost_button(ui, "pencil", tl!("Edit vault")).clicked() {
                        *act = Some(Act::Edit(v.id));
                    }
                    if !v.files.is_empty() && widgets::ghost_button(ui, "files", tl!("Open all")).clicked() {
                        *act = Some(Act::OpenAll(v.id));
                    }
                    if widgets::icon_pill(ui, "file-plus", tl!("Add files…"), true).clicked() {
                        *act = Some(Act::AddFiles(v.id));
                    }
                });
            });
        });
}

/// A filter's pill: filled in the accent when on. (Unlike a dialog's default button, it never
/// takes the keyboard focus, which the search box needs.)
fn filter_pill(ui: &mut egui::Ui, t: &Tokens, label: &str, on: bool) -> egui::Response {
    let font = theme::medium(12.5);
    let w = text_w(ui, label, &font);
    let (rect, resp) = ui.allocate_exact_size(vec2(w + 26.0, 28.0), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, on, label));
    let press = theme::Press::track(ui, &resp);
    let (fill, stroke, color) = if on {
        (t.accent_soft, Stroke::new(1.0, t.accent.gamma_multiply(0.6)), t.accent_text)
    } else {
        (press.fill(t.card, t.hover, t.pressed), Stroke::new(1.0, t.border), t.text)
    };
    ui.painter().rect(rect, CornerRadius::same(14), fill, stroke, egui::StrokeKind::Inside);
    ui.painter().text(rect.center() + press.offset(), Align2::CENTER_CENTER, label, font, color);
    theme::hand(resp)
}

/// Search, status and tag filters, sort and view.
fn toolbar(app: &mut PdfCraftApp, ui: &mut egui::Ui, t: &Tokens, v: &Vault) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
        let label = tl!("Search this vault");
        // A search icon that also names the field for screen readers.
        let (icon_rect, icon) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
        icon.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, label));
        icons::paint(ui, icon_rect, "search", 16.0, t.icon);
        ui.add(widgets::line(&mut app.vaults.page.query).hint_text(label).desired_width(240.0)).labelled_by(icon.id);
        ui.add_space(6.0);
        let page = &mut app.vaults.page;
        if filter_pill(ui, t, tl!("All"), page.status.is_none()).clicked() {
            page.status = None;
        }
        for s in [Status::Todo, Status::Doing, Status::Done] {
            if filter_pill(ui, t, status_label(s), page.status == Some(s)).clicked() {
                page.status = if page.status == Some(s) { None } else { Some(s) };
            }
        }
        let tags = v.tags();
        if !tags.is_empty() {
            let current = page.tag.clone().unwrap_or_else(|| tl!("Tags").to_string());
            let resp = widgets::icon_pill(ui, "tag", &current, page.tag.is_some());
            egui::Popup::menu(&resp).show(|ui| {
                if ui.button(tl!("All tags")).clicked() {
                    page.tag = None;
                }
                for tag in &tags {
                    if ui.button(std::borrow::Cow::<str>::Borrowed(tag)).clicked() {
                        page.tag = Some(tag.clone());
                    }
                }
            });
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let (icon, label) = if page.list { ("layout-grid", tl!("Cards")) } else { ("rows-3", tl!("List")) };
            if widgets::ghost_button(ui, icon, label).clicked() {
                page.list = !page.list;
            }
            let sort = match page.sort {
                Sort::Added => tl!("Recently added"),
                Sort::Name => tl!("Name"),
                Sort::Modified => tl!("Recently changed"),
                Sort::Size => tl!("Largest"),
            };
            let resp = widgets::ghost_button(ui, "arrow-up-down", sort);
            egui::Popup::menu(&resp).show(|ui| {
                for (s, l) in [
                    (Sort::Added, tl!("Recently added")),
                    (Sort::Name, tl!("Name")),
                    (Sort::Modified, tl!("Recently changed")),
                    (Sort::Size, tl!("Largest")),
                ] {
                    if ui.button(l).clicked() {
                        page.sort = s;
                    }
                }
            });
            let _ = t;
        });
    });
}

/// The binders, then the other vaults: click to show, drop a file to file or move it.
fn sidebar(
    app: &mut PdfCraftApp,
    ui: &mut egui::Ui,
    t: &Tokens,
    v: &Vault,
    others: &[(u64, String, [u8; 3], u64)],
    act: &mut Option<Act>,
    targets: &mut Vec<(u64, Rect)>,
) {
    widgets::section_title(ui, tl!("Binders"));
    let loose = v.files.iter().filter(|f| v.meta_of(f).folder.is_none()).count();
    let mut rows: Vec<(Binder, String, usize, &'static str)> = vec![
        (Binder::All, tl!("All files").to_string(), v.files.len(), "files"),
        (Binder::Loose, tl!("Not in a binder").to_string(), loose, "file-text"),
    ];
    for b in &v.folders {
        let n = v.files.iter().filter(|f| v.meta_of(f).folder == Some(b.id)).count();
        rows.push((Binder::One(b.id), b.name.clone(), n, "folder"));
    }
    for (binder, name, n, icon) in rows {
        let selected = app.vaults.page.binder == binder;
        let (rect, resp) = ui.allocate_exact_size(vec2(SIDE_W, 34.0), Sense::click());
        resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, selected, &name));
        // A file dropped on a binder is filed there (on All files, it stays as it is).
        let dropping = resp.dnd_hover_payload::<DragFile>().is_some_and(|d| d.from == v.id) && binder != Binder::All;
        let fill = if selected || dropping {
            super::color32(v.color).gamma_multiply(0.16)
        } else if resp.hovered() {
            t.hover
        } else {
            Color32::TRANSPARENT
        };
        ui.painter().rect_filled(rect, CornerRadius::same(8), fill);
        icons::paint(
            ui,
            Rect::from_min_size(rect.min + vec2(8.0, 8.0), vec2(18.0, 18.0)),
            icon,
            16.0,
            if selected { super::color32(v.color) } else { t.icon },
        );
        let c = ui.painter().text(rect.right_center() - vec2(10.0, 0.0), Align2::RIGHT_CENTER, n.to_string(), theme::regular(11.5), t.text_faint);
        widgets::row_text(ui, rect.left_center() + vec2(34.0, 0.0), name.clone(), theme::medium(13.0), t.text, c.left() - rect.left() - 42.0);
        if let Some(d) = resp.dnd_release_payload::<DragFile>()
            && d.from == v.id
        {
            match binder {
                Binder::Loose => *act = Some(Act::SetFolder(v.id, d.path.clone(), None)),
                Binder::One(b) => *act = Some(Act::SetFolder(v.id, d.path.clone(), Some(b))),
                Binder::All => {}
            }
        }
        let resp = theme::hand(resp);
        if resp.clicked() {
            app.vaults.page.binder = binder;
        }
        if let Binder::One(b) = binder {
            resp.context_menu(|ui| {
                if ui.button(tl!("Rename binder…")).clicked() {
                    *act = Some(Act::BinderDialog(v.id, Some(b)));
                }
                if ui.button(tl!("Delete binder")).clicked() {
                    *act = Some(Act::DeleteBinder(v.id, b));
                }
            });
        }
    }
    if widgets::ghost_button(ui, "folder-plus", tl!("New binder…")).clicked() {
        *act = Some(Act::BinderDialog(v.id, None));
    }
    if others.is_empty() {
        return;
    }
    ui.add_space(14.0);
    widgets::section_title(ui, tl!("Other vaults"));
    for (id, name, color, seed) in others {
        let (rect, resp) = ui.allocate_exact_size(vec2(SIDE_W, 36.0), Sense::click());
        targets.push((*id, rect));
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, name));
        let dropping = resp.dnd_hover_payload::<DragFile>().is_some_and(|d| d.from != *id);
        ui.painter().rect_filled(
            rect,
            CornerRadius::same(8),
            if dropping {
                super::color32(*color).gamma_multiply(0.18)
            } else if resp.hovered() {
                t.hover
            } else {
                Color32::TRANSPARENT
            },
        );
        crate::vault_monster::paint(ui.painter(), Rect::from_min_size(rect.min + vec2(5.0, 5.0), vec2(26.0, 26.0)), *seed, *color, 6);
        widgets::row_text(ui, rect.left_center() + vec2(38.0, 0.0), name.clone(), theme::regular(13.0), t.text, SIDE_W - 46.0);
        if let Some(d) = resp.dnd_release_payload::<DragFile>() {
            *act = Some(Act::Move(d.from, *id, d.path.clone()));
        }
        if theme::hand(resp).on_hover_text(tl!("Open this vault. Drop a file here to move it there.")).clicked() {
            *act = Some(Act::Show(*id));
        }
    }
}

/// A file's menu: open, status, binder, tags, note, move, remove.
fn file_menu(ui: &mut egui::Ui, v: &Vault, f: &str, m: &FileMeta, missing: bool, others: &[(u64, String, [u8; 3], u64)], act: &mut Option<Act>) {
    if !missing && ui.button(tl!("Open")).clicked() {
        *act = Some(Act::Open(f.to_string()));
    }
    if missing && ui.button(tl!("Locate…")).clicked() {
        *act = Some(Act::Locate(v.id, f.to_string()));
    }
    ui.menu_button(tl!("Status"), |ui| {
        for s in Status::ALL {
            if ui.add(egui::Button::selectable(m.status == s, status_label(s))).clicked() {
                *act = Some(Act::SetStatus(v.id, f.to_string(), s));
            }
        }
    });
    ui.menu_button(tl!("Binder"), |ui| {
        if ui.add(egui::Button::selectable(m.folder.is_none(), tl!("Not in a binder"))).clicked() {
            *act = Some(Act::SetFolder(v.id, f.to_string(), None));
        }
        for b in &v.folders {
            if ui.add(egui::Button::selectable(m.folder == Some(b.id), std::borrow::Cow::<str>::Borrowed(&b.name))).clicked() {
                *act = Some(Act::SetFolder(v.id, f.to_string(), Some(b.id)));
            }
        }
        if ui.button(tl!("New binder…")).clicked() {
            *act = Some(Act::BinderDialog(v.id, None));
        }
    });
    if ui.button(tl!("Tags…")).clicked() {
        *act = Some(Act::Tags(v.id, f.to_string()));
    }
    if ui.button(tl!("Note…")).clicked() {
        *act = Some(Act::Note(v.id, f.to_string()));
    }
    if !others.is_empty() {
        ui.menu_button(tl!("Move to vault"), |ui| {
            for (id, name, _, _) in others {
                if ui.button(std::borrow::Cow::<str>::Borrowed(name)).clicked() {
                    *act = Some(Act::Move(v.id, *id, f.to_string()));
                }
            }
        });
    }
    ui.separator();
    if ui.button(tl!("Remove from vault")).clicked() {
        *act = Some(Act::Remove(v.id, f.to_string()));
    }
}

/// A file as a card: the picture of its first page, its name, status, binder and tags.
#[expect(clippy::too_many_arguments, reason = "one card's inputs, all borrowed")]
fn card(
    app: &mut PdfCraftApp,
    ui: &mut egui::Ui,
    t: &Tokens,
    v: &Vault,
    f: &str,
    m: &FileMeta,
    others: &[(u64, String, [u8; 3], u64)],
    act: &mut Option<Act>,
) {
    let stat = app.vaults.stat(f);
    let missing = stat.is_some_and(|s| !s.exists);
    let name = vaults::file_name(f);
    let (rect, resp) = ui.allocate_exact_size(vec2(CARD_W, CARD_H), Sense::click_and_drag());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, !missing, &name));
    if !missing {
        resp.dnd_set_drag_payload(DragFile { from: v.id, path: f.to_string() });
    }
    let press = theme::Press::track(ui, &resp);
    if !missing {
        crate::home::card_lift(ui, t, rect, press);
    }
    let stroke = if press.hover > 0.05 { Stroke::new(1.0, super::color32(v.color).gamma_multiply(0.7)) } else { Stroke::new(1.0, t.border) };
    ui.painter().rect(rect, CornerRadius::same(12), press.fill(t.card, t.hover, t.pressed), stroke, egui::StrokeKind::Inside);
    let body = rect.translate(press.offset());
    // The picture: the first page on a soft backdrop, or the file's kind.
    let pic = Rect::from_min_size(body.min + vec2(8.0, 8.0), vec2(CARD_W - 16.0, THUMB_H));
    ui.painter().rect_filled(pic, CornerRadius::same(8), t.pasteboard);
    let tex = if missing { None } else { app.vault_thumb(ui.ctx(), f) };
    match tex {
        Some(tex) => {
            let size = tex.size_vec2();
            let scale = ((pic.width() - 16.0) / size.x.max(1.0)).min((pic.height() - 16.0) / size.y.max(1.0));
            let page = Rect::from_center_size(pic.center(), size * scale);
            ui.painter().add(egui::Shadow { offset: [0, 2], blur: 8, spread: 0, color: t.page_shadow }.as_shape(page, CornerRadius::same(2)));
            egui::Image::new(&tex).paint_at(ui, page);
        }
        None => {
            let icon = if missing { "triangle-alert" } else { "file-text" };
            icons::paint(
                ui,
                Rect::from_center_size(pic.center(), vec2(40.0, 40.0)),
                icon,
                38.0,
                if missing { t.accent_text } else { super::color32(v.color) },
            );
        }
    }
    if let Some(s) = (m.status != Status::None).then_some(m.status) {
        let badge = Rect::from_min_size(pic.right_top() + vec2(-30.0, 6.0), vec2(24.0, 24.0));
        ui.painter().circle_filled(badge.center(), 12.0, t.card);
        icons::paint(ui, badge.shrink(3.0), status_icon(s), 18.0, status_color(s));
    }
    if !m.note.is_empty() {
        let n = Rect::from_min_size(pic.left_top() + vec2(6.0, 6.0), vec2(24.0, 24.0));
        ui.painter().circle_filled(n.center(), 12.0, t.card);
        icons::paint(ui, n.shrink(4.0), "sticky-note", 16.0, t.icon);
    }
    // Name, then binder or where it is, then tags.
    let x = body.left() + 12.0;
    let w = CARD_W - 24.0;
    let mut job = egui::text::LayoutJob::simple(name.clone(), theme::medium(13.0), if missing { t.text_faint } else { t.text }, w);
    job.wrap.max_rows = 2;
    let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
    let name_h = galley.size().y;
    ui.painter().galley(egui::pos2(x, pic.bottom() + 8.0), galley, t.text);
    let mut y = pic.bottom() + 10.0 + name_h;
    let place = m.folder.and_then(|b| v.folder(b)).map_or_else(|| crate::folders_ui::location(f), |b| b.name.clone());
    widgets::row_text(ui, egui::pos2(x, y + 7.0), place, theme::regular(11.0), t.text_faint, w);
    y += 18.0;
    if missing {
        ui.painter().text(egui::pos2(x, y + 10.0), Align2::LEFT_CENTER, tl!("Missing"), theme::medium(12.0), t.accent_text);
        let font = theme::medium(12.0);
        let lw = text_w(ui, tl!("Locate…"), &font) + 18.0;
        let r = Rect::from_min_size(egui::pos2(body.right() - 10.0 - lw, y), vec2(lw, 22.0));
        if super::row_button(ui, t, r, ui.scope_id().with(("vault-card-locate", f)), tl!("Locate…"), true).clicked() {
            *act = Some(Act::Locate(v.id, f.to_string()));
        }
    } else {
        let mut cx = x;
        for tag in m.tags.iter().take(3) {
            let used = chip(ui, egui::pos2(cx, y), tag, super::color32(v.color), t, x + w - cx);
            if used == 0.0 {
                break;
            }
            cx += used + 4.0;
        }
    }
    let resp = if missing {
        resp.on_hover_text(tl!("This file has moved or been deleted. Locate it, or remove it from the vault."))
    } else {
        let hover = if m.note.is_empty() { f.to_string() } else { format!("{f}\n\n{}", m.note) };
        theme::hand(resp).on_hover_text(hover)
    };
    if resp.clicked() && !missing {
        *act = Some(Act::Open(f.to_string()));
    }
    resp.context_menu(|ui| file_menu(ui, v, f, m, missing, others, act));
}

/// A file as a list row: icon, name and place, status, tags, date and size.
#[expect(clippy::too_many_arguments, reason = "one row's inputs, all borrowed")]
fn row(
    app: &mut PdfCraftApp,
    ui: &mut egui::Ui,
    t: &Tokens,
    v: &Vault,
    f: &str,
    m: &FileMeta,
    others: &[(u64, String, [u8; 3], u64)],
    act: &mut Option<Act>,
) {
    let stat = app.vaults.stat(f);
    let missing = stat.is_some_and(|s| !s.exists);
    let name = vaults::file_name(f);
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 48.0), Sense::click_and_drag());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, !missing, &name));
    if !missing {
        resp.dnd_set_drag_payload(DragFile { from: v.id, path: f.to_string() });
    }
    let hovered = ui.rect_contains_pointer(rect);
    if hovered {
        ui.painter().rect_filled(rect, CornerRadius::same(8), t.hover);
    }
    icons::paint(
        ui,
        Rect::from_min_size(rect.min + vec2(10.0, 12.0), vec2(24.0, 24.0)),
        if missing { "triangle-alert" } else { status_icon(m.status) },
        20.0,
        if missing {
            t.accent_text
        } else if m.status == Status::None {
            super::color32(v.color)
        } else {
            status_color(m.status)
        },
    );
    let mut right = rect.right() - 10.0;
    if missing {
        let font = theme::medium(12.0);
        for (label, strong, locate) in [(tl!("Remove"), false, false), (tl!("Locate…"), true, true)] {
            let w = text_w(ui, label, &font) + 20.0;
            let r = Rect::from_min_max(egui::pos2(right - w, rect.center().y - 13.0), egui::pos2(right, rect.center().y + 13.0));
            if super::row_button(ui, t, r, ui.scope_id().with(("vault-file", label, f)), label, strong).clicked() {
                *act = Some(if locate { Act::Locate(v.id, f.to_string()) } else { Act::Remove(v.id, f.to_string()) });
            }
            right = r.left() - 8.0;
        }
        right =
            ui.painter().text(egui::pos2(right, rect.center().y), Align2::RIGHT_CENTER, tl!("Missing"), theme::medium(12.0), t.accent_text).left();
    } else if let Some(s) = stat {
        let size = crate::panels::human_size(usize::try_from(s.size).unwrap_or(usize::MAX));
        #[cfg(not(target_arch = "wasm32"))]
        let when = s.modified.map(crate::combine_ui::ago).unwrap_or_default();
        #[cfg(target_arch = "wasm32")]
        let when = String::new();
        let detail = if when.is_empty() { size } else { format!("{when}  ·  {size}") };
        right = ui.painter().text(egui::pos2(right, rect.center().y), Align2::RIGHT_CENTER, detail, theme::regular(12.0), t.text_muted).left() - 12.0;
        for tag in m.tags.iter().take(3).rev() {
            let w = text_w(ui, tag, &theme::medium(11.0)) + 14.0;
            if right - w < rect.left() + 260.0 {
                break;
            }
            chip(ui, egui::pos2(right - w, rect.center().y - 10.0), tag, super::color32(v.color), t, w);
            right -= w + 4.0;
        }
    }
    let width = right - 16.0 - (rect.left() + 46.0);
    widgets::row_text(ui, rect.min + vec2(46.0, 16.0), name.clone(), theme::medium(13.5), if missing { t.text_faint } else { t.text }, width);
    let place = m.folder.and_then(|b| v.folder(b)).map_or_else(|| crate::folders_ui::location(f), |b| b.name.clone());
    widgets::row_text(ui, rect.min + vec2(46.0, 33.0), place, theme::regular(11.0), t.text_faint, width);
    let resp = if missing {
        resp.on_hover_text(tl!("This file has moved or been deleted. Locate it, or remove it from the vault."))
    } else {
        theme::hand(resp).on_hover_text(f)
    };
    if resp.clicked() && !missing {
        *act = Some(Act::Open(f.to_string()));
    }
    resp.context_menu(|ui| file_menu(ui, v, f, m, missing, others, act));
}

/// The dialogs a file or binder opens: tags, note, binder name.
pub(super) fn dialog(app: &mut PdfCraftApp, ctx: &egui::Context, dialog: VaultDialog) {
    let t = Tokens::get(ctx);
    match dialog {
        VaultDialog::Tags { id, path, mut draft } => {
            let Some(v) = app.vaults.data.get(id).cloned() else {
                app.vaults.dialog = None;
                return;
            };
            let m = v.meta_of(&path);
            let mut close = false;
            let mut add: Option<String> = None;
            let mut remove: Option<String> = None;
            let resp = widgets::show_modal(ctx, "vault-tags", |ui| {
                ui.set_width(380.0);
                ui.label(egui::RichText::new(tl!("Tags")).font(theme::semibold(16.0)));
                ui.label(egui::RichText::new(vaults::file_name(&path)).color(t.text_muted));
                ui.add_space(10.0);
                ui.horizontal_wrapped(|ui| {
                    for tag in &m.tags {
                        if widgets::icon_pill(ui, "x", tag, false).on_hover_text(tl!("Remove this tag")).clicked() {
                            remove = Some(tag.clone());
                        }
                    }
                });
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    let label = tl!("New tag");
                    let field = ui.add(widgets::line(&mut draft).hint_text(label).desired_width(220.0).char_limit(vaults::MAX_TAG_CHARS));
                    field.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, label));
                    let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    if (widgets::pill_button(ui, tl!("Add"), true).clicked() || enter) && !draft.trim().is_empty() {
                        add = Some(std::mem::take(&mut draft));
                    }
                });
                // The vault's other tags, one click away.
                let unused: Vec<String> = v.tags().into_iter().filter(|x| !m.tags.iter().any(|y| y.to_lowercase() == x.to_lowercase())).collect();
                if !unused.is_empty() {
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new(tl!("In this vault")).small().color(t.text_faint));
                    ui.horizontal_wrapped(|ui| {
                        for tag in unused {
                            if widgets::icon_pill(ui, "tag", &tag, false).clicked() {
                                add = Some(tag);
                            }
                        }
                    });
                }
                ui.add_space(12.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if widgets::pill_button(ui, tl!("Done"), true).clicked() {
                        close = true;
                    }
                });
            });
            if let Some(tag) = add {
                app.vaults.data.add_tag(id, &path, &tag);
            }
            if let Some(tag) = remove {
                app.vaults.data.remove_tag(id, &path, &tag);
            }
            app.vaults.dialog = if close || resp.should_close() { None } else { Some(VaultDialog::Tags { id, path, draft }) };
        }
        VaultDialog::Note { id, path, mut text } => {
            let mut done: Option<bool> = None;
            let resp = widgets::show_modal(ctx, "vault-note", |ui| {
                ui.set_width(420.0);
                ui.label(egui::RichText::new(tl!("Note")).font(theme::semibold(16.0)));
                ui.label(egui::RichText::new(vaults::file_name(&path)).color(t.text_muted));
                ui.add_space(10.0);
                let label = tl!("Note");
                let field = ui.add(
                    egui::TextEdit::multiline(&mut text)
                        .hint_text(tl!("What to remember about this file"))
                        .desired_width(f32::INFINITY)
                        .desired_rows(6)
                        .char_limit(vaults::MAX_NOTE_CHARS),
                );
                field.widget_info(|| egui::WidgetInfo::labeled(egui::Role::MultilineTextInput, true, label));
                ui.add_space(12.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if widgets::pill_button(ui, tl!("Save"), true).clicked() {
                        done = Some(true);
                    }
                    if widgets::pill_button(ui, tl!("Cancel"), false).clicked() {
                        done = Some(false);
                    }
                });
            });
            if resp.should_close() && done.is_none() {
                done = Some(false);
            }
            match done {
                Some(true) => {
                    app.vaults.data.set_note(id, &path, &text);
                    app.vaults.dialog = None;
                }
                Some(false) => app.vaults.dialog = None,
                None => app.vaults.dialog = Some(VaultDialog::Note { id, path, text }),
            }
        }
        VaultDialog::Binder { id, binder, mut name } => {
            let mut done: Option<bool> = None;
            let resp = widgets::show_modal(ctx, "vault-binder", |ui| {
                ui.set_width(360.0);
                ui.label(egui::RichText::new(if binder.is_some() { tl!("Rename binder") } else { tl!("New binder") }).font(theme::semibold(16.0)));
                ui.add_space(10.0);
                let label = tl!("Binder name");
                let field = ui.add(widgets::line(&mut name).hint_text(label).desired_width(f32::INFINITY).char_limit(vaults::MAX_NAME_CHARS));
                field.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, label));
                if ui.memory(|m| m.focused().is_none()) {
                    field.request_focus();
                }
                if field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    done = Some(true);
                }
                ui.add_space(12.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if widgets::pill_button(ui, if binder.is_some() { tl!("Save") } else { tl!("Create binder") }, true).clicked() {
                        done = Some(true);
                    }
                    if widgets::pill_button(ui, tl!("Cancel"), false).clicked() {
                        done = Some(false);
                    }
                });
            });
            if resp.should_close() && done.is_none() {
                done = Some(false);
            }
            match done {
                Some(true) => {
                    let ok = match binder {
                        Some(b) => app.vaults.data.rename_folder(id, b, &name),
                        None => match app.vaults.data.add_folder(id, &name) {
                            Some(b) => {
                                app.vaults.page.binder = Binder::One(b);
                                true
                            }
                            None => false,
                        },
                    };
                    if ok {
                        app.vaults.dialog = None;
                    } else {
                        app.notify_tr("Give the binder a name (a vault holds up to 50 binders)");
                        app.vaults.dialog = Some(VaultDialog::Binder { id, binder, name });
                    }
                }
                Some(false) => app.vaults.dialog = None,
                None => app.vaults.dialog = Some(VaultDialog::Binder { id, binder, name }),
            }
        }
        other => app.vaults.dialog = Some(other),
    }
}
