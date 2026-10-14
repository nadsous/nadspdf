//! Bubbles: a tab taken out of the strip to float over the window (tab menu ▸ Float as a
//! bubble), so one PDF can sit on top of another. An open bubble is a small window with the full
//! document view. Clicking anywhere else folds it into a round head that can be dragged anywhere;
//! clicking the head opens it again. Dropping a bubble (open or folded) on the tab strip, or
//! switching to its document any other way, puts it back as a tab.

use egui::{Align2, Color32, CornerRadius, Pos2, Rect, Sense, Stroke, Vec2, pos2, vec2};
use pdfcraft_engine::DocId;

use crate::theme::{self, Tokens};
use crate::{PdfCraftApp, icons};

/// A folded bubble's diameter.
const HEAD: f32 = 52.0;
/// An open bubble's header height.
const HEADER: f32 = 32.0;
/// The smallest and the default open bubble.
const MIN_SIZE: Vec2 = vec2(260.0, 220.0);
const DEFAULT_SIZE: Vec2 = vec2(420.0, 540.0);

/// One floating document.
#[derive(Clone, Debug, PartialEq)]
pub struct Bubble {
    pub doc: DocId,
    /// An open bubble's top-left corner.
    pub pos: Pos2,
    /// An open bubble's size.
    pub size: Vec2,
    /// A folded bubble's centre.
    pub head: Pos2,
    pub open: bool,
}

/// The floating documents, oldest first (the newest is drawn on top).
#[derive(Debug, Default)]
pub struct Bubbles {
    list: Vec<Bubble>,
}

impl Bubbles {
    pub fn contains(&self, doc: DocId) -> bool {
        self.list.iter().any(|b| b.doc == doc)
    }

    pub fn list(&self) -> &[Bubble] {
        &self.list
    }

    /// Whether `doc` floats as an open bubble (its pages are on screen).
    pub fn is_open(&self, doc: DocId) -> bool {
        self.list.iter().any(|b| b.doc == doc && b.open)
    }

    /// Float `doc`, open, in `screen`'s right half.
    pub fn add(&mut self, doc: DocId, screen: Rect) {
        if self.contains(doc) {
            return;
        }
        let n = self.list.len() as f32;
        let size = DEFAULT_SIZE.min(screen.size() * 0.8).max(MIN_SIZE);
        let pos = pos2(screen.right() - size.x - 40.0 - 24.0 * n, screen.top() + 110.0 + 24.0 * n);
        // Clear of the right rail's buttons.
        let head = pos2(screen.right() - HEAD - 70.0, screen.bottom() - HEAD - 70.0 * n);
        self.list.push(Bubble { doc, pos, size, head, open: true });
    }

    pub fn remove(&mut self, doc: DocId) {
        self.list.retain(|b| b.doc != doc);
    }

    /// Forget bubbles whose document has closed.
    pub fn prune(&mut self, open: &[DocId]) {
        self.list.retain(|b| open.contains(&b.doc));
    }
}

/// What the user did to a bubble this frame.
enum Act {
    Dock(DocId),
    Close(DocId),
    Fold(DocId),
    Open(DocId),
}

impl PdfCraftApp {
    /// Tab menu ▸ Float as a bubble. The tab leaves the strip; if it was the one showing, the
    /// nearest tab still in the strip takes its place.
    pub(crate) fn float_tab(&mut self, doc: DocId, screen: Rect) {
        let Some(index) = self.views.iter().position(|v| v.id == doc) else { return };
        self.tab_groups.remove(doc);
        self.bubbles.add(doc, screen);
        if self.active == Some(index) {
            let docked = |i: &usize| self.views.get(*i).is_some_and(|v| !self.bubbles.contains(v.id));
            self.active = (0..index).rev().find(docked).or_else(|| (index + 1..self.views.len()).find(docked));
        }
    }

    /// Put a bubble back in the strip, showing.
    pub(crate) fn dock_bubble(&mut self, doc: DocId) {
        self.bubbles.remove(doc);
        if let Some(i) = self.views.iter().position(|v| v.id == doc) {
            self.active = Some(i);
        }
    }
}

/// Tabs that show a bubbled document come back to the strip: switching to it (Ctrl+Tab, a
/// search hit, Window menu) docks it.
pub(crate) fn dock_active(app: &mut PdfCraftApp) {
    let ids: Vec<DocId> = app.views.iter().map(|v| v.id).collect();
    app.bubbles.prune(&ids);
    if let Some(doc) = app.active.and_then(|i| app.views.get(i)).map(|v| v.id)
        && app.bubbles.contains(doc)
    {
        app.bubbles.remove(doc);
    }
}

/// Draw the bubbles over everything else in the window. `strip` is the tab strip, where a
/// bubble dropped goes back to being a tab.
pub fn show(app: &mut PdfCraftApp, ctx: &egui::Context, strip: Rect) {
    if app.bubbles.list.is_empty() {
        return;
    }
    let t = Tokens::get(ctx);
    let screen = ctx.content_rect();
    let mut acts = Vec::new();
    let mut dragging_over_strip = false;
    let mut hit = Vec::new();
    let bubbles = app.bubbles.list.clone();
    for b in &bubbles {
        let Some(index) = app.views.iter().position(|v| v.id == b.doc) else { continue };
        let name = app.session.get(b.doc).map(|d| d.display_name()).unwrap_or_default();
        if b.open {
            let area = egui::Area::new(egui::Id::new(("bubble", b.doc.0))).order(egui::Order::Foreground).fixed_pos(b.pos);
            let mut moved = Vec2::ZERO;
            let mut resized = Vec2::ZERO;
            let shown = area.show(ctx, |ui| {
                let frame = egui::Frame::NONE
                    .fill(t.panel)
                    .stroke(Stroke::new(1.0, t.accent.gamma_multiply(0.6)))
                    .corner_radius(14)
                    .shadow(egui::epaint::Shadow { offset: [0, 10], blur: 28, spread: 0, color: Color32::from_black_alpha(110) });
                frame.show(ui, |ui| {
                    let (rect, _) = ui.allocate_exact_size(b.size, Sense::hover());
                    let header = Rect::from_min_size(rect.min, vec2(rect.width(), HEADER));
                    // The header moves the bubble; dropped on the tab strip, it docks.
                    let grip = ui.interact(header, ui.id().with("grip"), Sense::click_and_drag());
                    if grip.dragged() {
                        moved = grip.drag_delta();
                    }
                    if grip.drag_stopped() && ui.ctx().pointer_latest_pos().is_some_and(|p| strip.contains(p)) {
                        acts.push(Act::Dock(b.doc));
                    }
                    if grip.dragged() && ui.ctx().pointer_latest_pos().is_some_and(|p| strip.contains(p)) {
                        dragging_over_strip = true;
                    }
                    let mark = Rect::from_center_size(header.left_center() + vec2(18.0, 0.0), vec2(14.0, 14.0));
                    egui::Image::from_bytes("bytes://nads-n.svg", include_bytes!("../../../assets/app-icon/nads-n.svg")).paint_at(ui, mark);
                    let buttons = 3.0 * 26.0 + 8.0;
                    crate::widgets::row_text(
                        ui,
                        header.left_center() + vec2(32.0, 0.0),
                        name.clone(),
                        theme::medium(12.5),
                        t.text,
                        (header.width() - 40.0 - buttons).max(10.0),
                    );
                    let mut x = header.right() - 6.0;
                    for (icon, tip, act) in [
                        ("x", tl!("Close"), Act::Close(b.doc)),
                        ("app-window", tl!("Put back as a tab"), Act::Dock(b.doc)),
                        ("minus", tl!("Fold into a bubble"), Act::Fold(b.doc)),
                    ] {
                        let r = Rect::from_min_size(pos2(x - 26.0, header.center().y - 13.0), vec2(26.0, 26.0));
                        x -= 28.0;
                        let resp = ui.put(r, |ui: &mut egui::Ui| icons::button(ui, icon, 26.0, false, tip));
                        if resp.clicked() {
                            acts.push(act);
                        }
                    }
                    ui.painter().hline(header.x_range(), header.bottom(), Stroke::new(1.0, t.divider));
                    // The document itself: the same view as in a tab.
                    let body = Rect::from_min_max(pos2(rect.left(), header.bottom() + 1.0), rect.max);
                    ui.scope_builder(egui::UiBuilder::new().max_rect(body), |ui| {
                        ui.set_clip_rect(body.intersect(ui.clip_rect()));
                        app.in_bubble = true;
                        crate::canvas::document_area(app, index, ui);
                        app.in_bubble = false;
                    });
                    // Resize from the bottom-right corner.
                    let corner = Rect::from_min_size(rect.max - vec2(16.0, 16.0), vec2(16.0, 16.0));
                    let handle = ui.interact(corner, ui.id().with("resize"), Sense::drag());
                    if handle.dragged() {
                        resized = handle.drag_delta();
                    }
                    let c = t.text_faint;
                    for k in [4.0, 8.0, 12.0] {
                        ui.painter().line_segment(
                            [pos2(rect.right() - k, rect.bottom() - 3.0), pos2(rect.right() - 3.0, rect.bottom() - k)],
                            Stroke::new(1.0, c),
                        );
                    }
                    theme::hand(handle).on_hover_cursor(egui::CursorIcon::ResizeNwSe);
                });
            });
            hit.push(shown.response.rect);
            if let Some(live) = app.bubbles.list.iter_mut().find(|x| x.doc == b.doc) {
                live.pos += moved;
                live.size = (live.size + resized).max(MIN_SIZE).min(screen.size());
                // Keep the header reachable.
                live.pos.x = live.pos.x.clamp(screen.left() - live.size.x + 80.0, screen.right() - 80.0);
                live.pos.y = live.pos.y.clamp(screen.top(), screen.bottom() - HEADER);
            }
        } else {
            let area =
                egui::Area::new(egui::Id::new(("bubble-head", b.doc.0))).order(egui::Order::Foreground).fixed_pos(b.head - Vec2::splat(HEAD / 2.0));
            let mut moved = Vec2::ZERO;
            let shown = area.show(ctx, |ui| {
                let (rect, resp) = ui.allocate_exact_size(Vec2::splat(HEAD), Sense::click_and_drag());
                resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &name));
                let c = rect.center();
                ui.painter().circle_filled(c + vec2(0.0, 4.0), HEAD / 2.0, Color32::from_black_alpha(90));
                ui.painter().circle_filled(c, HEAD / 2.0, t.card);
                let ring = if resp.hovered() || resp.dragged() { t.accent } else { t.accent.gamma_multiply(0.7) };
                ui.painter().circle_stroke(c, HEAD / 2.0 - 1.0, Stroke::new(2.5, ring));
                egui::Image::from_bytes("bytes://nads-n.svg", include_bytes!("../../../assets/app-icon/nads-n.svg"))
                    .paint_at(ui, Rect::from_center_size(c - vec2(0.0, 5.0), vec2(20.0, 20.0)));
                // The document's initials under the mark tell bubbles apart.
                let stem = name.rsplit_once('.').map_or(name.as_str(), |(stem, _)| stem);
                let initials: String = stem.chars().filter(|ch| ch.is_alphanumeric()).take(3).collect();
                ui.painter().text(c + vec2(0.0, 12.0), Align2::CENTER_CENTER, initials.to_uppercase(), theme::semibold(9.5), t.text_muted);
                if resp.dragged() {
                    moved = resp.drag_delta();
                    if ui.ctx().pointer_latest_pos().is_some_and(|p| strip.contains(p)) {
                        dragging_over_strip = true;
                    }
                }
                if resp.drag_stopped() && ui.ctx().pointer_latest_pos().is_some_and(|p| strip.contains(p)) {
                    acts.push(Act::Dock(b.doc));
                } else if resp.clicked() {
                    acts.push(Act::Open(b.doc));
                }
                theme::hand(resp).on_hover_text(format!("{name}\n{}", tl!("Click to open, drag onto the tabs to put it back")));
            });
            hit.push(shown.response.rect);
            if let Some(live) = app.bubbles.list.iter_mut().find(|x| x.doc == b.doc) {
                live.head += moved;
                let r = HEAD / 2.0;
                live.head = pos2(live.head.x.clamp(screen.left() + r, screen.right() - r), live.head.y.clamp(screen.top() + r, screen.bottom() - r));
            }
        }
    }
    // Over the tab strip: show where the bubble will land.
    if dragging_over_strip {
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("bubble-dock-hint")));
        painter.rect_stroke(strip.shrink(2.0), CornerRadius::same(8), Stroke::new(2.0, t.accent), egui::StrokeKind::Inside);
    }
    // A click anywhere outside the open bubbles folds them.
    let pressed_at = ctx.input(|i| if i.pointer.primary_pressed() { i.pointer.interact_pos() } else { None });
    if let Some(p) = pressed_at
        && !hit.iter().any(|r| r.contains(p))
    {
        for b in app.bubbles.list.iter().filter(|b| b.open) {
            acts.push(Act::Fold(b.doc));
        }
    }
    for act in acts {
        match act {
            Act::Dock(doc) => app.dock_bubble(doc),
            Act::Close(doc) => {
                if let Some(i) = app.views.iter().position(|v| v.id == doc) {
                    app.request_close_tab(i);
                }
            }
            Act::Fold(doc) => {
                if let Some(b) = app.bubbles.list.iter_mut().find(|b| b.doc == doc) {
                    b.open = false;
                }
            }
            Act::Open(doc) => {
                if let Some(pos) = app.bubbles.list.iter().position(|b| b.doc == doc) {
                    let mut b = app.bubbles.list.remove(pos);
                    b.open = true;
                    // Open next to the head, inside the window, and on top of the others.
                    let x = if b.head.x > screen.center().x { b.head.x - b.size.x - HEAD / 2.0 } else { b.head.x + HEAD / 2.0 };
                    b.pos = pos2(
                        x.clamp(screen.left(), (screen.right() - b.size.x).max(screen.left())),
                        (b.head.y - 40.0).clamp(screen.top(), (screen.bottom() - b.size.y).max(screen.top())),
                    );
                    app.bubbles.list.push(b);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bubbles_are_added_once_and_go_with_their_document() {
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(1440.0, 900.0));
        let mut b = Bubbles::default();
        b.add(DocId(1), screen);
        b.add(DocId(1), screen);
        b.add(DocId(2), screen);
        assert_eq!(b.list().len(), 2);
        assert!(b.is_open(DocId(1)));
        // A new bubble stays inside a small window.
        let tiny = Rect::from_min_size(Pos2::ZERO, vec2(100.0, 100.0));
        b.add(DocId(3), tiny);
        assert!(b.list().iter().all(|x| x.size.x >= MIN_SIZE.x && x.size.y.is_finite()));
        b.prune(&[DocId(2)]);
        assert_eq!(b.list().iter().map(|x| x.doc).collect::<Vec<_>>(), [DocId(2)]);
        b.remove(DocId(2));
        assert!(!b.contains(DocId(2)));
    }
}
