//! Drag a document tab along the strip to move it among the others, or out of the strip to open
//! it in a window of its own, for instance on another screen.
//!
//! While a tab is dragged, a label with its name follows the pointer. Released far enough from
//! the strip (or outside the window), the document opens in a new NadsPDF window placed where it
//! was dropped, and its tab here closes. The desktop app supplies how to open that window
//! ([`PdfCraftApp::open_in_new_window`]); without it (the web build) tabs don't tear off.
//!
//! The new window opens the saved file: a document with unsaved changes, or one never saved,
//! stays here and asks to be saved first, so no work can be lost between the two windows.

use std::sync::Arc;

use egui::{Pos2, Rect, Stroke, vec2};
use pdfcraft_engine::DocId;

use crate::{PdfCraftApp, theme};

/// Opens a saved PDF in a new NadsPDF window, its top-left corner near `at` (screen pixels).
pub type NewWindow = Arc<dyn Fn(&std::path::Path, Option<[f32; 2]>) -> Result<(), String> + Send + Sync>;

/// How far below or above the tab strip a tab must be dropped to tear off, in points.
const TEAR_DISTANCE: f32 = 48.0;

/// A tab being dragged.
#[derive(Clone, Copy, Debug)]
pub(crate) struct TabDrag {
    pub(crate) doc: DocId,
    /// The last pointer position seen, in window points (it can be outside the window).
    pub(crate) pos: Option<Pos2>,
}

/// `pos` (points in this window, possibly outside it) on the screen, in physical pixels: screens
/// can have different scales, so the new window converts with its own.
pub(crate) fn screen_px(ctx: &egui::Context, pos: Pos2) -> Option<[f32; 2]> {
    let inner = ctx.input(|i| i.viewport().inner_rect)?;
    let ppp = ctx.pixels_per_point();
    let p = (inner.min + pos.to_vec2()) * ppp;
    (p.x.is_finite() && p.y.is_finite()).then_some([p.x, p.y])
}

/// Where a tab dropped at `x` goes: before the first tab whose middle is right of `x`, or last.
/// `tabs` are the strip's tabs as (view index, rect), in strip order.
fn drop_slot(x: f32, tabs: &[(usize, Rect)]) -> Option<(usize, f32)> {
    let last = tabs.last()?;
    Some(tabs.iter().find(|(_, r)| x < r.center().x).map_or((last.0 + 1, last.1.right()), |(i, r)| (*i, r.left())))
}

/// Whether a tab released at `pos` (window points) tears off, the strip being `strip` and the
/// window's content `window`.
fn tears_off(pos: Pos2, strip: Rect, window: Rect) -> bool {
    !window.contains(pos) || pos.y > strip.bottom() + TEAR_DISTANCE || pos.y < strip.top() - TEAR_DISTANCE
}

impl PdfCraftApp {
    /// A document tab started to be dragged.
    pub(crate) fn start_tab_drag(&mut self, doc: DocId) {
        self.tab_drag = Some(TabDrag { doc, pos: None });
    }

    /// Move `doc`'s tab so it sits before the tab at view index `before` (`views.len()`: last).
    /// The tab showing stays the one showing.
    pub(crate) fn move_tab(&mut self, doc: DocId, before: usize) {
        let Some(from) = self.views.iter().position(|v| v.id == doc) else { return };
        let to = if before > from { before - 1 } else { before }.min(self.views.len().saturating_sub(1));
        if to == from {
            return;
        }
        let shown = self.active.and_then(|i| self.views.get(i)).map(|v| v.id);
        let view = self.views.remove(from);
        self.views.insert(to, view);
        self.active = shown.and_then(|id| self.views.iter().position(|v| v.id == id));
    }

    /// Open `doc` in a new window at `at` (screen pixels) and close its tab here.
    pub(crate) fn tear_off(&mut self, doc: DocId, at: Option<[f32; 2]>) {
        let Some(open) = self.open_in_new_window.clone() else { return };
        let Some(index) = self.views.iter().position(|v| v.id == doc) else { return };
        let Some(document) = self.session.get(doc) else { return };
        let Some(path) = document.path.clone() else {
            self.notify_tr("Save this PDF before opening it in a new window.");
            return;
        };
        if self.has_unsaved_work(index) {
            self.notify_tr("Save your changes before opening this PDF in a new window.");
            return;
        }
        match open(std::path::Path::new(&path), at) {
            Ok(()) => self.close_tab(index),
            Err(e) => self.notify_error(e),
        }
    }
}

/// Follow a dragged tab, and tear it off where it's dropped (each frame, after the strip).
pub(crate) fn update(app: &mut PdfCraftApp, ctx: &egui::Context) {
    let Some(mut drag) = app.tab_drag else { return };
    let (latest, down, window) = ctx.input(|i| (i.pointer.latest_pos(), i.pointer.primary_down(), i.content_rect()));
    if latest.is_some() {
        drag.pos = latest;
    }
    let Some(pos) = drag.pos else {
        // No position yet (the drag just started): wait for one.
        app.tab_drag = (down).then_some(drag);
        return;
    };
    let strip = app.strip_rect;
    let away = app.open_in_new_window.is_some() && tears_off(pos, strip, window);
    let slot = (!away).then(|| drop_slot(pos.x, &app.tab_rects)).flatten();
    if down {
        app.tab_drag = Some(drag);
        if let Some((_, x)) = slot {
            // Where the tab would land: a bar between tabs.
            let t = theme::Tokens::get(ctx);
            let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("tab-drop-slot")));
            painter.line_segment([egui::pos2(x, strip.top() + 6.0), egui::pos2(x, strip.bottom() - 4.0)], Stroke::new(2.0, t.accent));
        }
        ghost(app, ctx, drag.doc, pos, away);
        ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
        ctx.request_repaint();
        return;
    }
    app.tab_drag = None;
    if away {
        // A little up and left of the pointer, so the new window's tab strip lands under it.
        app.tear_off(drag.doc, screen_px(ctx, pos - vec2(120.0, 16.0)));
    } else if let Some((before, _)) = slot {
        app.move_tab(drag.doc, before);
    }
}

/// The dragged tab's name at the pointer, with a hint once releasing would tear it off.
fn ghost(app: &PdfCraftApp, ctx: &egui::Context, doc: DocId, pos: Pos2, away: bool) {
    let t = theme::Tokens::get(ctx);
    let name = app.session.get(doc).map(|d| d.display_name()).unwrap_or_default();
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("tab-tear-off")));
    let font = theme::regular(13.0);
    let label = crate::panels::ellipsized_prefix(&name, |s| s.chars().count() <= 40);
    let galley = painter.layout_no_wrap(label, font, t.text);
    let hint = away.then(|| painter.layout_no_wrap(tl!("Release to open in a new window").to_string(), theme::regular(11.5), t.text_muted));
    let width = galley.size().x.max(hint.as_ref().map_or(0.0, |h| h.size().x)) + 44.0;
    let height = if hint.is_some() { 48.0 } else { 30.0 };
    let rect = Rect::from_min_size(pos + vec2(10.0, 10.0), vec2(width, height));
    painter.rect(rect, 8, t.card, Stroke::new(1.0, if away { t.accent } else { t.border }), egui::StrokeKind::Inside);
    painter.galley(rect.min + vec2(30.0, 15.0 - galley.size().y / 2.0), galley, t.text);
    if let Some(hint) = hint {
        painter.galley(rect.min + vec2(30.0, 33.0 - hint.size().y / 2.0), hint, t.text_muted);
    }
    let mark = Rect::from_center_size(rect.min + vec2(16.0, 15.0), vec2(13.0, 13.0));
    painter.rect_filled(mark.shrink(1.0), 2, t.accent);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_drop_lands_before_the_first_tab_whose_middle_is_right_of_it() {
        let tab = |i: usize, x: f32| (i, Rect::from_min_size(egui::pos2(x, 0.0), vec2(100.0, 30.0)));
        let tabs = [tab(0, 100.0), tab(1, 200.0), tab(2, 300.0)];
        assert_eq!(drop_slot(90.0, &tabs), Some((0, 100.0)));
        assert_eq!(drop_slot(160.0, &tabs), Some((1, 200.0)), "past tab 0's middle");
        assert_eq!(drop_slot(340.0, &tabs), Some((2, 300.0)));
        assert_eq!(drop_slot(360.0, &tabs), Some((3, 400.0)), "past the last tab's middle: last");
        assert_eq!(drop_slot(10.0, &[]), None);
    }

    #[test]
    fn only_drops_away_from_the_strip_or_outside_the_window_tear_off() {
        let window = Rect::from_min_size(Pos2::ZERO, vec2(1200.0, 800.0));
        let strip = Rect::from_min_max(Pos2::ZERO, egui::pos2(1200.0, 40.0));
        assert!(!tears_off(egui::pos2(300.0, 20.0), strip, window), "on the strip");
        assert!(!tears_off(egui::pos2(300.0, 40.0 + TEAR_DISTANCE), strip, window), "just below it");
        assert!(tears_off(egui::pos2(300.0, 41.0 + TEAR_DISTANCE), strip, window), "into the page");
        assert!(tears_off(egui::pos2(1500.0, 20.0), strip, window), "past the window's edge");
        assert!(tears_off(egui::pos2(300.0, -80.0), strip, window), "above the window");
    }
}
