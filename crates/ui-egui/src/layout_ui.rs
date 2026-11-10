//! NadsPDF's Mise en page tab: page layout with text and image frames, exported to PDF.
//!
//! The document lives in [`pdfcraft_layout::Layout`]; this module draws its current page, lets
//! the pointer create, select, move and resize frames, edits the selected frame's text and font,
//! and exports the document as a PDF that opens in a NadsPDF tab. Like the Combine files tab,
//! it is a tab of its own that holds no PDF document.

use egui::{Align, Color32, CornerRadius, Layout, Pos2, Rect, Sense, Stroke, pos2, vec2};
use pdfcraft_layout::{ItemKind, Layout as Doc, Paper, Rect as PRect};

use crate::theme::{self, Tokens};
use crate::{PdfCraftApp, icons, widgets};

/// The tool the pointer uses on the page.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tool {
    /// Pick, move and resize frames.
    #[default]
    Select,
    /// Drag out a text frame.
    Text,
    /// Drag out a frame for the picked image.
    Image,
}

/// Which corner or side of the selected frame a resize drags.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Handle {
    TopLeft,
    Top,
    TopRight,
    Right,
    BottomRight,
    Bottom,
    BottomLeft,
    Left,
}

impl Handle {
    const ALL: [Handle; 8] =
        [Handle::TopLeft, Handle::Top, Handle::TopRight, Handle::Right, Handle::BottomRight, Handle::Bottom, Handle::BottomLeft, Handle::Left];

    /// Where the handle sits on `r` (screen).
    fn at(self, r: Rect) -> Pos2 {
        let (cx, cy) = (r.center().x, r.center().y);
        match self {
            Handle::TopLeft => r.left_top(),
            Handle::Top => pos2(cx, r.top()),
            Handle::TopRight => r.right_top(),
            Handle::Right => pos2(r.right(), cy),
            Handle::BottomRight => r.right_bottom(),
            Handle::Bottom => pos2(cx, r.bottom()),
            Handle::BottomLeft => r.left_bottom(),
            Handle::Left => pos2(r.left(), cy),
        }
    }

    /// `r` (page points) with this handle moved by (`dx`, `dy`).
    fn drag(self, r: PRect, dx: f64, dy: f64) -> PRect {
        let mut o = r;
        match self {
            Handle::TopLeft => {
                o.x0 += dx;
                o.y0 += dy;
            }
            Handle::Top => o.y0 += dy,
            Handle::TopRight => {
                o.x1 += dx;
                o.y0 += dy;
            }
            Handle::Right => o.x1 += dx,
            Handle::BottomRight => {
                o.x1 += dx;
                o.y1 += dy;
            }
            Handle::Bottom => o.y1 += dy,
            Handle::BottomLeft => {
                o.x0 += dx;
                o.y1 += dy;
            }
            Handle::Left => o.x0 += dx,
        }
        o
    }

    fn cursor(self) -> egui::CursorIcon {
        match self {
            Handle::TopLeft | Handle::BottomRight => egui::CursorIcon::ResizeNwSe,
            Handle::TopRight | Handle::BottomLeft => egui::CursorIcon::ResizeNeSw,
            Handle::Top | Handle::Bottom => egui::CursorIcon::ResizeVertical,
            Handle::Left | Handle::Right => egui::CursorIcon::ResizeHorizontal,
        }
    }
}

/// A drag on the page in progress, in page points.
#[derive(Clone, Copy, Debug)]
enum Drag {
    /// Drawing a new frame from `from`.
    Create { from: (f64, f64), to: (f64, f64) },
    /// Moving frame `id` by the pointer's travel since `from`.
    Move { id: u64, from: (f64, f64), to: (f64, f64) },
    /// Resizing frame `id` from `rect` by one handle.
    Resize { id: u64, handle: Handle, rect: PRect, from: (f64, f64), to: (f64, f64) },
}

/// The page as last drawn, kept until the document, page or scale changes.
struct PageTexture {
    key: (u64, usize, u32),
    texture: egui::TextureHandle,
}

/// The Mise en page tab.
#[derive(Default)]
pub struct LayoutTab {
    pub open: bool,
    pub focused: bool,
    doc: Option<Doc>,
    /// The page shown (0-based).
    pub page: usize,
    /// Screen points per page point; `None` fits the page in the window.
    zoom: Option<f32>,
    pub tool: Tool,
    pub selected: Option<u64>,
    drag: Option<Drag>,
    texture: Option<PageTexture>,
    /// The selected text frame's text being edited, and its frame.
    draft: Option<(u64, String)>,
    /// The image the Image tool places, and its name.
    image: Option<(Vec<u8>, String)>,
}

impl std::fmt::Debug for LayoutTab {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LayoutTab").field("open", &self.open).field("page", &self.page).field("tool", &self.tool).finish_non_exhaustive()
    }
}

impl LayoutTab {
    /// Whether a document is open in the tab.
    pub fn has_document(&self) -> bool {
        self.doc.is_some()
    }

    /// The document's page count (0 without one).
    pub fn page_count(&self) -> usize {
        self.doc.as_ref().map_or(0, Doc::page_count)
    }

    /// The frames on the page shown (tests, automation).
    pub fn items(&self) -> Vec<pdfcraft_layout::Item> {
        self.doc.as_ref().map_or_else(Vec::new, |d| d.items(self.page))
    }

    /// The document itself (tests, automation).
    pub fn document_mut(&mut self) -> Option<&mut Doc> {
        self.doc.as_mut()
    }

    fn set_doc(&mut self, doc: Doc) {
        *self = LayoutTab { open: true, focused: true, doc: Some(doc), ..LayoutTab::default() };
    }
}

impl PdfCraftApp {
    /// Whether the Mise en page tab is the one shown.
    pub fn layout_showing(&self) -> bool {
        self.active.is_none() && self.layout_tab.open && self.layout_tab.focused
    }

    /// Show the Mise en page tab, opening it (empty) if needed.
    pub fn open_layout_tab(&mut self) {
        self.layout_tab.open = true;
        self.layout_tab.focused = true;
        self.combine_tab.focused = false;
        self.active = None;
    }

    /// A mode was chosen while the Mise en page tab showed: the last PDF tab shows instead (Home
    /// without one); the Mise en page tab stays open.
    pub fn leave_layout_tab(&mut self) {
        if self.layout_showing() {
            self.layout_tab.focused = false;
            self.active = self.views.len().checked_sub(1);
        }
    }

    /// Start a new layout document on `paper` in the Mise en page tab.
    pub fn new_layout(&mut self, paper: Paper) {
        match Doc::new(paper, 1) {
            Ok(doc) => {
                self.layout_tab.set_doc(doc);
                self.open_layout_tab();
            }
            Err(e) => self.notify_error(e),
        }
    }

    /// Open the sample magazine in the Mise en page tab.
    pub fn open_layout_sample(&mut self) {
        match Doc::sample() {
            Ok(doc) => {
                self.layout_tab.set_doc(doc);
                self.open_layout_tab();
            }
            Err(e) => self.notify_error(e),
        }
    }

    /// Close the Mise en page tab and forget its document; the last PDF tab shows, if any.
    pub fn close_layout_tab(&mut self) {
        let showing = self.layout_showing();
        self.layout_tab = LayoutTab::default();
        if showing {
            self.active = self.views.len().checked_sub(1);
        }
    }

    /// Undo in the Mise en page document; `false` when there is nothing to undo.
    pub fn layout_undo(&mut self) -> bool {
        self.layout_tab.draft = None;
        self.layout_tab.doc.as_mut().is_some_and(Doc::undo)
    }

    pub fn layout_redo(&mut self) -> bool {
        self.layout_tab.draft = None;
        self.layout_tab.doc.as_mut().is_some_and(Doc::redo)
    }

    /// Export the document as a PDF and open it in a NadsPDF tab (unsaved, named `name`).
    pub fn export_layout(&mut self, name: &str) {
        self.commit_layout_text();
        let Some(doc) = self.layout_tab.doc.as_mut() else { return };
        match doc.export_pdf() {
            Ok(bytes) => {
                if let Err(e) = self.open_bytes(name, None, bytes) {
                    self.notify_error(e);
                }
            }
            Err(e) => self.notify_error(e),
        }
    }

    /// Write the text being edited into its frame (one undo step), if it changed.
    fn commit_layout_text(&mut self) {
        let Some((id, text)) = self.layout_tab.draft.take() else { return };
        let Some(doc) = self.layout_tab.doc.as_mut() else { return };
        if doc.text(id).as_deref().map(normalize_breaks) == Some(text.clone()) {
            return;
        }
        if let Err(e) = doc.set_text(id, &text) {
            self.notify_error(e);
        }
    }

    /// Pick an image for the Image tool; the next drag on the page places it.
    #[cfg(not(target_arch = "wasm32"))]
    fn pick_layout_image(&mut self) {
        let dialog = rfd::AsyncFileDialog::new().add_filter(tl!("Images"), &["png", "jpg", "jpeg", "gif", "bmp", "webp", "tif", "tiff"]);
        self.ask_one(crate::pickers::Ask::File(dialog), None, |app, path| match std::fs::read(&path) {
            Ok(bytes) => {
                let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                app.layout_tab.image = Some((bytes, name));
                app.layout_tab.tool = Tool::Image;
                app.notify_tr("Drag on the page to place the image");
            }
            Err(e) => app.notify_error(e),
        });
    }
}

/// Paragraph breaks as the text box writes them.
fn normalize_breaks(text: &str) -> String {
    text.replace("\r\n", "\n").replace(['\u{2029}', '\r'], "\n")
}

/// The families the font menu offers: NadsPDF's own, and the document faces when built in.
const FAMILIES: [&str; 4] = ["Inter", "Source Serif 4", "Source Sans 3", "JetBrains Mono"];

/// The Mise en page tab's page.
pub(crate) fn page(app: &mut PdfCraftApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    if app.layout_tab.doc.is_none() {
        empty_state(app, ui, &t);
        return;
    }
    toolbar(app, ui, &t);
    ui.add_space(6.0);
    let inspector_w = 280.0;
    let avail = ui.available_rect_before_wrap();
    let canvas = Rect::from_min_max(avail.min, pos2((avail.right() - inspector_w - 10.0).max(avail.left() + 100.0), avail.bottom()));
    let side = Rect::from_min_max(pos2(canvas.right() + 10.0, avail.top()), avail.max);
    ui.scope_builder(egui::UiBuilder::new().max_rect(canvas), |ui| page_view(app, ui, &t));
    ui.scope_builder(egui::UiBuilder::new().max_rect(side), |ui| inspector(app, ui, &t));
    ui.allocate_rect(avail, Sense::hover());
    keys(app, ui);
}

/// No document yet: start one, or open the sample.
fn empty_state(app: &mut PdfCraftApp, ui: &mut egui::Ui, t: &Tokens) {
    ui.add_space(40.0);
    ui.vertical_centered(|ui| {
        ui.set_max_width(520.0);
        egui::Frame::NONE
            .fill(t.card)
            .stroke(Stroke::new(1.0, t.border))
            .corner_radius(CornerRadius::same(14))
            .inner_margin(egui::Margin::same(28))
            .show(ui, |ui| {
                ui.vertical_centered(|ui| {
                    let (slot, _) = ui.allocate_exact_size(vec2(48.0, 48.0), Sense::hover());
                    icons::paint(ui, slot, "panels-top-left", 40.0, t.accent);
                    ui.add_space(10.0);
                    ui.label(egui::RichText::new(tl!("Page design")).font(theme::semibold(22.0)).color(t.text));
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new(tl!("Lay out pages with text and pictures, then export them as a PDF."))
                            .font(theme::regular(14.0))
                            .color(t.text_muted),
                    );
                    ui.add_space(18.0);
                    ui.horizontal(|ui| {
                        let w = ui.available_width();
                        ui.add_space((w - 380.0).max(0.0) / 2.0);
                        if widgets::pill_button(ui, tl!("New A4 document"), true).clicked() {
                            app.new_layout(Paper::A4);
                        }
                        if widgets::pill_button(ui, tl!("New Letter document"), false).clicked() {
                            app.new_layout(Paper::Letter);
                        }
                        if widgets::pill_button(ui, tl!("Sample magazine"), false).clicked() {
                            app.open_layout_sample();
                        }
                    });
                });
            });
    });
}

/// Tools, history, pages, zoom and Export.
fn toolbar(app: &mut PdfCraftApp, ui: &mut egui::Ui, t: &Tokens) {
    let pages = app.layout_tab.page_count();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        widgets::pill_group(ui, |ui| {
            for (tool, icon, label) in
                [(Tool::Select, "mouse-pointer-2", "Select"), (Tool::Text, "type", "Text frame"), (Tool::Image, "image", "Image")]
            {
                if widgets::mode_pill(ui, icon, tl!(label), app.layout_tab.tool == tool).clicked() {
                    app.commit_layout_text();
                    if tool == Tool::Image {
                        #[cfg(not(target_arch = "wasm32"))]
                        app.pick_layout_image();
                    } else {
                        app.layout_tab.tool = tool;
                    }
                }
            }
        });
        widgets::pill_group(ui, |ui| {
            if icons::button(ui, "undo-2", widgets::PILL_HEIGHT, false, tl!("Undo")).clicked() {
                app.layout_undo();
            }
            if icons::button(ui, "redo-2", widgets::PILL_HEIGHT, false, tl!("Redo")).clicked() {
                app.layout_redo();
            }
        });
        widgets::pill_group(ui, |ui| {
            if icons::button(ui, "chevron-left", widgets::PILL_HEIGHT, false, tl!("Previous page")).clicked() && app.layout_tab.page > 0 {
                app.commit_layout_text();
                app.layout_tab.page -= 1;
                app.layout_tab.selected = None;
            }
            let label = crate::i18n::fmt(tl!("Page {n} of {count}"), &[("n", &(app.layout_tab.page + 1).to_string()), ("count", &pages.to_string())]);
            ui.label(egui::RichText::new(label).font(theme::medium(13.0)).color(t.text));
            if icons::button(ui, "chevron-right", widgets::PILL_HEIGHT, false, tl!("Next page")).clicked() && app.layout_tab.page + 1 < pages {
                app.commit_layout_text();
                app.layout_tab.page += 1;
                app.layout_tab.selected = None;
            }
            if icons::button(ui, "file-plus", widgets::PILL_HEIGHT, false, tl!("Add a page")).clicked() {
                app.commit_layout_text();
                let added = app.layout_tab.doc.as_mut().map(Doc::add_page);
                match added {
                    Some(Ok(page)) => {
                        app.layout_tab.page = page;
                        app.layout_tab.selected = None;
                    }
                    Some(Err(e)) => app.notify_error(e),
                    None => {}
                }
            }
        });
        widgets::pill_group(ui, |ui| {
            if icons::button(ui, "zoom-out", widgets::PILL_HEIGHT, false, tl!("Zoom out")).clicked() {
                app.layout_tab.zoom = Some((app.layout_tab.zoom.unwrap_or(1.0) / 1.25).max(0.1));
            }
            let fit = app.layout_tab.zoom.is_none();
            if widgets::pill_button(ui, tl!("Fit"), fit).clicked() {
                app.layout_tab.zoom = None;
            }
            if icons::button(ui, "zoom-in", widgets::PILL_HEIGHT, false, tl!("Zoom in")).clicked() {
                app.layout_tab.zoom = Some((app.layout_tab.zoom.unwrap_or(1.0) * 1.25).min(8.0));
            }
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if widgets::pill_button(ui, tl!("Export as PDF"), true).clicked() {
                app.export_layout(&format!("{}.pdf", tl!("Page design")));
            }
        });
    });
}

/// The page, its frames and the pointer's work on them.
fn page_view(app: &mut PdfCraftApp, ui: &mut egui::Ui, t: &Tokens) {
    let area = ui.max_rect();
    ui.painter().rect_filled(area, CornerRadius::same(12), t.pasteboard);
    let page = app.layout_tab.page;
    let Some((pw, ph)) = app.layout_tab.doc.as_ref().and_then(|d| d.page_size(page)) else { return };
    // Points on screen per page point: the zoom, or the page fitted with a margin.
    let fit = ((area.width() - 48.0) / pw as f32).min((area.height() - 48.0) / ph as f32).max(0.05);
    let zoom = app.layout_tab.zoom.unwrap_or(fit);
    let size = vec2(pw as f32 * zoom, ph as f32 * zoom);
    // Dragging draws or moves frames: the wheel and the scroll bars scroll.
    egui::ScrollArea::both()
        .auto_shrink([false, false])
        .scroll_source(egui::scroll_area::ScrollSource::SCROLL_BAR | egui::scroll_area::ScrollSource::MOUSE_WHEEL)
        .show(ui, |ui| {
            let room = vec2(size.x.max(area.width() - 24.0) + 24.0, size.y.max(area.height() - 24.0) + 24.0);
            let (outer, _) = ui.allocate_exact_size(room, Sense::hover());
            let rect = Rect::from_center_size(outer.center(), size);
            draw_page(app, ui, t, rect, zoom);
            interact(app, ui, t, outer, rect, zoom);
        });
}

/// The page raster, from the cache when nothing changed.
fn draw_page(app: &mut PdfCraftApp, ui: &mut egui::Ui, t: &Tokens, rect: Rect, zoom: f32) {
    let ppp = ui.ctx().pixels_per_point();
    let scale = f64::from(zoom * ppp);
    let tab = &mut app.layout_tab;
    let Some(doc) = tab.doc.as_mut() else { return };
    let key = (doc.revision(), tab.page, (scale * 1000.0).round() as u32);
    if tab.texture.as_ref().is_none_or(|p| p.key != key)
        && let Some(img) = doc.render_page(tab.page, scale)
    {
        let size = [img.width as usize, img.height as usize];
        let image = egui::ColorImage::from_rgba_premultiplied(size, &img.rgba);
        let texture = ui.ctx().load_texture("layout-page", image, egui::TextureOptions::LINEAR);
        tab.texture = Some(PageTexture { key, texture });
    }
    let shadow = rect.translate(vec2(0.0, 3.0)).expand(2.0);
    ui.painter().rect_filled(shadow, CornerRadius::same(4), t.page_shadow);
    match &tab.texture {
        Some(p) => {
            ui.painter().image(p.texture.id(), rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        }
        None => {
            ui.painter().rect_filled(rect, CornerRadius::ZERO, Color32::WHITE);
        }
    }
}

/// Screen rect of `r` (page points) on the page drawn at `page`.
fn to_screen(page: Rect, zoom: f32, r: PRect) -> Rect {
    Rect::from_min_max(
        pos2(page.left() + r.x0 as f32 * zoom, page.top() + r.y0 as f32 * zoom),
        pos2(page.left() + r.x1 as f32 * zoom, page.top() + r.y1 as f32 * zoom),
    )
}

/// Page point under screen point `p`.
fn to_page(page: Rect, zoom: f32, p: Pos2) -> (f64, f64) {
    (f64::from((p.x - page.left()) / zoom), f64::from((p.y - page.top()) / zoom))
}

/// The handle of the selected frame under `p`, if any.
fn handle_at(frame: Rect, p: Pos2) -> Option<Handle> {
    Handle::ALL.into_iter().find(|h| h.at(frame).distance(p) <= 7.0)
}

/// Create, select, move and resize frames with the pointer.
/// The whole area around the page takes the pointer, so a frame can start just off the page.
fn interact(app: &mut PdfCraftApp, ui: &mut egui::Ui, t: &Tokens, area: Rect, page_rect: Rect, zoom: f32) {
    let response = ui.interact(area, ui.scope_id().with("layout-page"), Sense::click_and_drag());
    let tab = &mut app.layout_tab;
    let page = tab.page;
    let Some(doc) = tab.doc.as_ref() else { return };
    let items = doc.items(page);
    let selected_rect = tab.selected.and_then(|id| items.iter().find(|i| i.id == id)).map(|i| to_screen(page_rect, zoom, i.rect));
    let pointer = response.hover_pos().or(response.interact_pointer_pos());

    // Hover: a faint outline on frames, and the cursor for what a press would do.
    if let Some(p) = response.hover_pos() {
        let (x, y) = to_page(page_rect, zoom, p);
        match tab.tool {
            Tool::Select => {
                if let Some(h) = selected_rect.and_then(|r| handle_at(r, p)) {
                    ui.ctx().set_cursor_icon(h.cursor());
                } else if let Some(it) = items.iter().rev().find(|i| i.rect.contains(x, y)) {
                    ui.painter().rect_stroke(
                        to_screen(page_rect, zoom, it.rect),
                        0.0,
                        Stroke::new(1.0, t.accent.gamma_multiply(0.6)),
                        egui::StrokeKind::Outside,
                    );
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Move);
                }
            }
            Tool::Text | Tool::Image => ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair),
        }
    }

    if response.drag_started()
        && let Some(p) = pointer
    {
        let at = to_page(page_rect, zoom, p);
        tab.drag = match tab.tool {
            Tool::Text | Tool::Image => Some(Drag::Create { from: at, to: at }),
            Tool::Select => match (selected_rect.and_then(|r| handle_at(r, p)), tab.selected) {
                (Some(handle), Some(id)) => items.iter().find(|i| i.id == id).map(|i| Drag::Resize { id, handle, rect: i.rect, from: at, to: at }),
                _ => items.iter().rev().find(|i| i.rect.contains(at.0, at.1)).map(|i| {
                    tab.selected = Some(i.id);
                    Drag::Move { id: i.id, from: at, to: at }
                }),
            },
        };
    }
    if response.dragged()
        && let Some(p) = pointer
    {
        let at = to_page(page_rect, zoom, p);
        match &mut tab.drag {
            Some(Drag::Create { to, .. } | Drag::Move { to, .. } | Drag::Resize { to, .. }) => *to = at,
            None => {}
        }
    }

    // What the drag would do, drawn over the page.
    match tab.drag {
        Some(Drag::Create { from, to }) => {
            let r = to_screen(page_rect, zoom, PRect::new(from.0, from.1, to.0, to.1).normalized());
            ui.painter().rect(r, 0.0, t.accent_soft.gamma_multiply(0.5), Stroke::new(1.5, t.accent), egui::StrokeKind::Inside);
        }
        Some(Drag::Move { id, from, to }) => {
            if let Some(it) = items.iter().find(|i| i.id == id) {
                let r = to_screen(page_rect, zoom, it.rect).translate(vec2(((to.0 - from.0) as f32) * zoom, ((to.1 - from.1) as f32) * zoom));
                ui.painter().rect_stroke(r, 0.0, Stroke::new(1.5, t.accent), egui::StrokeKind::Outside);
            }
        }
        Some(Drag::Resize { handle, rect, from, to, .. }) => {
            let r = to_screen(page_rect, zoom, handle.drag(rect, to.0 - from.0, to.1 - from.1).normalized());
            ui.painter().rect_stroke(r, 0.0, Stroke::new(1.5, t.accent), egui::StrokeKind::Outside);
        }
        None => {
            if let Some(r) = selected_rect {
                ui.painter().rect_stroke(r, 0.0, Stroke::new(1.5, t.accent), egui::StrokeKind::Outside);
                for h in Handle::ALL {
                    let c = Rect::from_center_size(h.at(r), vec2(8.0, 8.0));
                    ui.painter().rect(c, 1.0, Color32::WHITE, Stroke::new(1.5, t.accent), egui::StrokeKind::Inside);
                }
            }
        }
    }

    if response.drag_stopped() {
        let drag = tab.drag.take();
        finish_drag(app, drag);
    } else if response.clicked()
        && app.layout_tab.tool == Tool::Select
        && let Some(p) = pointer
    {
        let (x, y) = to_page(page_rect, zoom, p);
        let hit = app.layout_tab.doc.as_ref().and_then(|d| d.item_at(page, x, y)).map(|i| i.id);
        if hit != app.layout_tab.selected {
            app.commit_layout_text();
        }
        app.layout_tab.selected = hit;
    }
}

/// Apply a finished drag to the document.
fn finish_drag(app: &mut PdfCraftApp, drag: Option<Drag>) {
    let page = app.layout_tab.page;
    let result = match drag {
        Some(Drag::Create { from, to }) => {
            let r = PRect::new(from.0, from.1, to.0, to.1).normalized();
            // A click without a drag makes a frame of a usual size.
            let r = if r.width() < pdfcraft_layout::MIN_FRAME * 2.0 && r.height() < pdfcraft_layout::MIN_FRAME * 2.0 {
                PRect::new(from.0, from.1, from.0 + 200.0, from.1 + 100.0)
            } else {
                r
            };
            app.commit_layout_text();
            let tool = app.layout_tab.tool;
            let image = app.layout_tab.image.take();
            let Some(doc) = app.layout_tab.doc.as_mut() else { return };
            let made = match (tool, image) {
                (Tool::Image, Some((bytes, name))) => doc.add_image(page, r, &bytes, &name),
                _ => doc.add_text_frame(page, r, tl!("Type your text here")),
            };
            made.map(|id| {
                app.layout_tab.selected = Some(id);
                app.layout_tab.tool = Tool::Select;
            })
        }
        Some(Drag::Move { id, from, to }) => {
            let Some(doc) = app.layout_tab.doc.as_mut() else { return };
            doc.move_item(id, to.0 - from.0, to.1 - from.1)
        }
        Some(Drag::Resize { id, handle, rect, from, to }) => {
            let Some(doc) = app.layout_tab.doc.as_mut() else { return };
            doc.resize_item(page, id, handle.drag(rect, to.0 - from.0, to.1 - from.1).normalized())
        }
        None => Ok(()),
    };
    if let Err(e) = result {
        app.notify_error(e);
    }
}

/// The selected frame's text, font and position.
fn inspector(app: &mut PdfCraftApp, ui: &mut egui::Ui, t: &Tokens) {
    egui::Frame::NONE
        .fill(t.card)
        .stroke(Stroke::new(1.0, t.border))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(egui::Margin::same(14))
        .show(ui, |ui| {
            ui.set_min_size(ui.available_size());
            let page = app.layout_tab.page;
            let item = app.layout_tab.selected.and_then(|id| app.layout_tab.doc.as_ref()?.item(page, id));
            let Some(item) = item else {
                app.layout_tab.selected = None;
                widgets::section_title(ui, tl!("Page design"));
                ui.label(
                    egui::RichText::new(tl!("Pick a tool above and drag on the page to add a frame. Click a frame to change it."))
                        .font(theme::regular(13.0))
                        .color(t.text_muted),
                );
                return;
            };
            let kind = match item.kind {
                ItemKind::Text => tl!("Text frame"),
                ItemKind::Image => tl!("Image"),
                ItemKind::Shape => tl!("Shape"),
            };
            widgets::section_title(ui, kind);
            let r = item.rect;
            ui.label(
                egui::RichText::new(format!("{:.0} × {:.0} pt  ·  x {:.0}, y {:.0}", r.width(), r.height(), r.x0, r.y0))
                    .font(theme::regular(12.0))
                    .color(t.text_muted),
            );
            ui.add_space(10.0);
            if item.kind == ItemKind::Text {
                if app.layout_tab.draft.as_ref().is_none_or(|(id, _)| *id != item.id) {
                    let text = app.layout_tab.doc.as_mut().and_then(|d| d.text(item.id)).map(|s| normalize_breaks(&s)).unwrap_or_default();
                    app.layout_tab.draft = Some((item.id, text));
                }
                ui.label(egui::RichText::new(tl!("Text")).font(theme::medium(13.0)).color(t.text));
                let mut lost = false;
                if let Some((_, text)) = app.layout_tab.draft.as_mut() {
                    let edit = ui.add(egui::TextEdit::multiline(text).desired_rows(8).desired_width(f32::INFINITY).font(theme::regular(13.0)));
                    lost = edit.lost_focus();
                }
                if lost {
                    app.commit_layout_text();
                }
                ui.add_space(10.0);
                ui.label(egui::RichText::new(tl!("Font")).font(theme::medium(13.0)).color(t.text));
                let mut family = None;
                egui::ComboBox::from_id_salt("layout-font").selected_text(tl!("Choose a font")).width(ui.available_width()).show_ui(ui, |ui| {
                    for f in FAMILIES {
                        if ui.selectable_label(false, f).clicked() {
                            family = Some(f);
                        }
                    }
                });
                ui.add_space(6.0);
                let mut size = None;
                ui.label(egui::RichText::new(tl!("Size")).font(theme::regular(13.0)).color(t.text_muted));
                ui.horizontal_wrapped(|ui| {
                    for s in [9.0, 11.0, 14.0, 18.0, 24.0, 36.0, 48.0, 72.0] {
                        if ui.small_button(format!("{s:.0}")).clicked() {
                            size = Some(s);
                        }
                    }
                });
                if family.is_some() || size.is_some() {
                    app.commit_layout_text();
                    let doc = app.layout_tab.doc.as_mut();
                    let r = doc.map(|d| match (family, size) {
                        (Some(f), _) => d.set_font_family(item.id, f),
                        (None, Some(s)) => d.set_font_size(item.id, s),
                        (None, None) => Ok(()),
                    });
                    if let Some(Err(e)) = r {
                        app.notify_error(e);
                    }
                }
                ui.add_space(12.0);
            }
            if widgets::pill_button(ui, tl!("Delete"), false).clicked() {
                app.layout_tab.draft = None;
                let r = app.layout_tab.doc.as_mut().map(|d| d.delete(item.id));
                app.layout_tab.selected = None;
                if let Some(Err(e)) = r {
                    app.notify_error(e);
                }
            }
        });
}

/// Delete removes the selected frame and Escape drops the selection, unless a text box has the
/// keyboard; arrow keys nudge the frame by a point (ten with Shift).
fn keys(app: &mut PdfCraftApp, ui: &mut egui::Ui) {
    if ui.ctx().memory(|m| m.focused().is_some()) {
        return;
    }
    let Some(id) = app.layout_tab.selected else { return };
    let (delete, escape, nudge) = ui.input(|i| {
        let step = if i.modifiers.shift { 10.0 } else { 1.0 };
        let mut d = (0.0, 0.0);
        if i.key_pressed(egui::Key::ArrowLeft) {
            d.0 -= step;
        }
        if i.key_pressed(egui::Key::ArrowRight) {
            d.0 += step;
        }
        if i.key_pressed(egui::Key::ArrowUp) {
            d.1 -= step;
        }
        if i.key_pressed(egui::Key::ArrowDown) {
            d.1 += step;
        }
        (i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace), i.key_pressed(egui::Key::Escape), d)
    });
    let r = if delete {
        app.layout_tab.selected = None;
        app.layout_tab.draft = None;
        app.layout_tab.doc.as_mut().map(|d| d.delete(id))
    } else if nudge != (0.0, 0.0) {
        app.layout_tab.doc.as_mut().map(|d| d.move_item(id, nudge.0, nudge.1))
    } else {
        if escape {
            app.layout_tab.selected = None;
        }
        None
    };
    if let Some(Err(e)) = r {
        app.notify_error(e);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handles_drag_their_own_edges() {
        let r = PRect::new(10.0, 10.0, 110.0, 60.0);
        assert_eq!(Handle::BottomRight.drag(r, 5.0, 5.0), PRect::new(10.0, 10.0, 115.0, 65.0));
        assert_eq!(Handle::Left.drag(r, -5.0, 99.0), PRect::new(5.0, 10.0, 110.0, 60.0));
        assert_eq!(Handle::Top.drag(r, 99.0, -5.0), PRect::new(10.0, 5.0, 110.0, 60.0));
        // Dragged past the opposite edge, the rectangle turns over and stays valid.
        assert_eq!(Handle::Right.drag(r, -150.0, 0.0).normalized(), PRect::new(-40.0, 10.0, 10.0, 60.0));
    }

    #[test]
    fn page_and_screen_points_round_trip() {
        let page = Rect::from_min_size(pos2(100.0, 50.0), vec2(300.0, 400.0));
        let s = to_screen(page, 0.5, PRect::new(20.0, 40.0, 220.0, 140.0));
        assert_eq!(s, Rect::from_min_max(pos2(110.0, 70.0), pos2(210.0, 120.0)));
        assert_eq!(to_page(page, 0.5, pos2(110.0, 70.0)), (20.0, 40.0));
    }

    #[test]
    fn paragraph_breaks_read_as_new_lines() {
        assert_eq!(normalize_breaks("a\u{2029}b\r\nc\rd"), "a\nb\nc\nd");
    }
}
