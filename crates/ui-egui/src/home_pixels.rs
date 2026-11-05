//! NadsPDF: the living top of Home. A field of pixels in the nads reds, crossed by a slow sheen
//! with a few pixels twinkling, behind Osman (the owner's pixel portrait) and the welcome.
//!
//! Decorative, so it stays quiet: constant linear motion at a low frame rate, only while the
//! window has focus and animation effects are on (Preferences ▸ Animation effects, which follows
//! the system's reduced-motion setting); otherwise a still frame. Nothing waits for it.
//!
//! Home's cards also come in once, staggered ([`entrance`]): fading and rising 8 pt with a strong
//! ease-out, 40 ms apart, each in 280 ms, so the page feels alive without slowing anyone down.

use egui::{Color32, CornerRadius, Rect, Sense, Stroke, vec2};

use crate::theme::{self, Tokens};

/// Height of the band.
const HEIGHT: f32 = 72.0;
/// Side of one pixel of the field.
const CELL: f32 = 8.0;
/// Frames per second while it moves: plenty for pixels, light on the processor.
const FPS: f32 = 24.0;

/// Where an entrance starts, and the pass Home was last drawn in.
#[derive(Clone, Copy)]
struct Entered {
    at: f64,
    pass: u64,
}

fn entered_id() -> egui::Id {
    egui::Id::unique("nadspdf-home-entered")
}

/// A stable pseudo-random value in 0..1 for cell (`x`, `y`).
fn noise(x: i64, y: i64) -> f32 {
    let mut h = (x.wrapping_mul(0x9E37_79B9) ^ y.wrapping_mul(0x85EB_CA6B)) as u64;
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    (h % 1000) as f32 / 1000.0
}

/// Whether Home animates now.
fn moving(ctx: &egui::Context) -> bool {
    crate::motion::animated(ctx) && ctx.input(|i| i.focused)
}

/// The band at the top of Home: the pixel field, Osman and the welcome.
pub(crate) fn hero(ui: &mut egui::Ui, t: &Tokens) {
    let ctx = ui.ctx().clone();
    // Home was not drawn last pass (another tab showed): its entrance plays again.
    let pass = ctx.cumulative_pass_nr();
    let now = ctx.input(|i| i.time);
    ctx.data_mut(|d| {
        let e = d.get_temp::<Entered>(entered_id());
        let at = match e {
            Some(e) if e.pass.saturating_add(1) >= pass => e.at,
            _ => now,
        };
        d.insert_temp(entered_id(), Entered { at, pass });
    });

    let width = ui.available_width().max(200.0);
    let (rect, resp) = ui.allocate_exact_size(vec2(width, HEIGHT), Sense::hover());
    // Drawn, not laid out: say what it says to screen readers (and tests).
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, tl!("Welcome to NadsPDF")));
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, CornerRadius::same(14), t.card);
    let animate = moving(&ctx);
    let time = if animate { now as f32 } else { 0.0 };
    let cols = (rect.width() / CELL).floor() as i64;
    let rows = (rect.height() / CELL).floor() as i64;
    for row in 0..rows {
        for col in 0..cols {
            let x = col as f32 / cols.max(1) as f32;
            // The pixels gather on the right, leaving the welcome clear.
            let presence = ((x - 0.38) / 0.4).clamp(0.0, 1.0);
            if presence <= 0.0 {
                continue;
            }
            let n = noise(col, row);
            // A slow diagonal sheen sweeping right to left.
            let wave = ((col as f32 * 0.22 + row as f32 * 0.35) + time * 1.4).sin().max(0.0).powi(8);
            // A few pixels twinkling on their own.
            let spark = if n > 0.93 { 0.5 + 0.5 * (time * 2.2 + n * 40.0).sin() } else { 0.0 };
            let level = (n * 0.28 + wave * 0.55 + spark * 0.6) * presence;
            if level < 0.08 {
                continue;
            }
            let color = t.card.lerp_to_gamma(t.accent, level.min(1.0));
            let min = rect.min + vec2(col as f32 * CELL, row as f32 * CELL);
            painter.rect_filled(Rect::from_min_size(min, vec2(CELL - 1.0, CELL - 1.0)), CornerRadius::ZERO, color);
        }
    }
    painter.rect_stroke(rect, CornerRadius::same(14), Stroke::new(1.0, t.border), egui::StrokeKind::Inside);

    // Osman, the assistant who greets you, crisp at four screen pixels per art pixel.
    let (alpha, rise) = entrance(&ctx, 0);
    let avatar = Rect::from_min_size(rect.min + vec2(14.0, (HEIGHT - 56.0) / 2.0 + rise), vec2(56.0, 56.0));
    egui::Image::from_bytes("bytes://nads/osman.svg", include_bytes!("../../../assets/app-icon/assistants/osman.svg"))
        .tint(Color32::WHITE.gamma_multiply(alpha))
        .paint_at(ui, avatar);
    let text_left = avatar.right() + 18.0;
    let title = painter.layout_no_wrap(tl!("Welcome to NadsPDF").to_owned(), theme::semibold(20.0), t.text.gamma_multiply(alpha));
    let subtitle = painter.layout(
        tl!("An open-source PDF workbench — local, private, and scriptable.").to_owned(),
        theme::regular(13.0),
        t.text_muted.gamma_multiply(alpha),
        (rect.right() - text_left - 24.0).max(120.0),
    );
    let block = title.size().y + 3.0 + subtitle.size().y;
    let top = rect.center().y - block / 2.0 + rise;
    painter.galley(egui::pos2(text_left, top), title.clone(), t.text);
    painter.galley(egui::pos2(text_left, top + title.size().y + 3.0), subtitle, t.text_muted);

    if animate {
        ctx.request_repaint_after(std::time::Duration::from_secs_f32(1.0 / FPS));
    }
}

/// How far item `index` of Home is into its entrance: (opacity 0..1, downward offset in points).
/// Settled at once when animation effects are off.
pub(crate) fn entrance(ctx: &egui::Context, index: usize) -> (f32, f32) {
    if !crate::motion::animated(ctx) {
        return (1.0, 0.0);
    }
    let Some(e) = ctx.data(|d| d.get_temp::<Entered>(entered_id())) else { return (1.0, 0.0) };
    let elapsed = (ctx.input(|i| i.time) - e.at) as f32;
    let delay = 0.06 + 0.04 * index.min(20) as f32;
    let p = ((elapsed - delay) / 0.28).clamp(0.0, 1.0);
    if p < 1.0 {
        ctx.request_repaint();
    }
    // A strong ease-out (quint): fast at first, settling softly.
    let eased = 1.0 - (1.0 - p).powi(5);
    (eased, 8.0 * (1.0 - eased))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noise_is_stable_and_in_range() {
        for (x, y) in [(0, 0), (5, 9), (-3, 7), (i64::MAX, i64::MIN)] {
            let n = noise(x, y);
            assert!((0.0..1.0).contains(&n), "{n}");
            assert_eq!(n, noise(x, y));
        }
    }
}
