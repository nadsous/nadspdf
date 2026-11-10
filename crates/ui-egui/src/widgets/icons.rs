//! Vector icons: embedded Lucide SVGs, recoloured to white, rasterized by egui_extras at the exact
//! on-screen pixel size, and tinted per use (PhotoCraft lesson: never use Unicode glyphs as icons).

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

use craft_ui::buttons::{ButtonVisuals, IconButton, IconButtonStyle};
use craft_ui::icons::{SvgIconSet, SvgStyle};

use egui::{Color32, Rect, Response, Stroke, Vec2};

use crate::icon_data::ICONS;
use crate::pixel_icon_data::PIXEL_ICONS;
use crate::theme::{self, Tokens};

fn white_icons() -> &'static SvgIconSet {
    static MAP: OnceLock<SvgIconSet> = OnceLock::new();
    MAP.get_or_init(|| SvgIconSet::new(ICONS, SvgStyle { current_color: "#ffffff", stroke_width: Some("1.75") }, "bytes://icons/", "square"))
}

/// NadsPDF: Pixelarticons, for Preferences ▸ Icons ▸ Pixelarticons. Same names as the Lucide
/// set, so every place that asks for an icon gets its pixel counterpart.
fn pixel_icons() -> &'static SvgIconSet {
    static MAP: OnceLock<SvgIconSet> = OnceLock::new();
    MAP.get_or_init(|| SvgIconSet::new(PIXEL_ICONS, SvgStyle { current_color: "#ffffff", stroke_width: None }, "bytes://pixel-icons/", "square"))
}

/// Preferences ▸ Icons: Lucide outlines (the default, as before) or Pixelarticons.
static PIXEL_STYLE: AtomicBool = AtomicBool::new(false);

/// Draw the icons with Pixelarticons (`true`) or as Lucide outlines.
pub fn set_pixel_style(on: bool) {
    PIXEL_STYLE.store(on, Ordering::Relaxed);
}

pub fn pixel_style() -> bool {
    PIXEL_STYLE.load(Ordering::Relaxed)
}

fn icons() -> &'static SvgIconSet {
    if pixel_style() { pixel_icons() } else { white_icons() }
}

pub fn exists(name: &str) -> bool {
    white_icons().contains(name)
}

pub(crate) fn source(name: &str) -> egui::ImageSource<'static> {
    // NadsPDF caches an unknown name under the resolved fallback name.
    let set = icons();
    set.source(set.canonical_name(name).unwrap_or("square"))
}

pub fn image(name: &str, size: f32, tint: Color32) -> egui::Image<'static> {
    egui::Image::new(source(name)).fit_to_exact_size(Vec2::splat(size)).tint(tint)
}

pub fn paint(ui: &egui::Ui, rect: Rect, name: &str, size: f32, tint: Color32) {
    let ppp = ui.ctx().pixels_per_point();
    let size = if pixel_style() { pixel_size(size, ppp) } else { size };
    // Whole screen pixels, so each icon pixel covers the same number of them.
    let center = if pixel_style() && ppp > 0.0 { (rect.center().to_vec2() * ppp).round().to_pos2() / ppp } else { rect.center() };
    image(name, size, tint).paint_at(ui, Rect::from_center_size(center, Vec2::splat(size)));
}

/// Pixelarticons' grid: 24 × 24 pixels.
const PIXEL_GRID: f32 = 24.0;

/// A size near `size` points at which every pixel of a pixel icon covers the same whole number
/// of screen pixels. Drawn at any other size, one- and two-pixel-wide cells alternate, and below
/// 24 screen pixels some of the icon's lines disappear. A little smaller (to 88 %) or larger (to
/// 150 %: a 16-point icon on a 100 % screen becomes 24 pixels) is fine; when neither fits, the
/// icon keeps its size rather than becoming too large for its place.
pub(crate) fn pixel_size(size: f32, ppp: f32) -> f32 {
    if !(size.is_finite() && ppp.is_finite() && size > 0.0 && ppp > 0.0) {
        return size;
    }
    let target = size * ppp;
    let cells = target / PIXEL_GRID;
    let below = cells.floor() * PIXEL_GRID;
    let above = cells.ceil().max(1.0) * PIXEL_GRID;
    let px = if below >= PIXEL_GRID && below >= 0.88 * target {
        below
    } else if above <= 1.5 * target {
        above
    } else {
        target.round().max(1.0)
    };
    px / ppp
}

/// Square icon button: transparent until hovered; `selected` gets the accent treatment.
/// `tooltip` is both its accessible name and its one tooltip.
pub fn button(ui: &mut egui::Ui, name: &str, box_size: f32, selected: bool, tooltip: &str) -> Response {
    labeled_button(ui, name, box_size, selected, tooltip, tooltip)
}

/// [`button`] whose tooltip says more than its accessible name (`label`). It shows that one
/// tooltip, so callers add no `on_hover_text` of their own (#6548: two tooltips stacked).
pub fn labeled_button(ui: &mut egui::Ui, name: &str, box_size: f32, selected: bool, label: &str, tooltip: &str) -> Response {
    let t = Tokens::get(ui.ctx());
    // The shared button paints no fill of its own: the eased press wash below replaces it.
    let clear = |icon| ButtonVisuals { fill: Color32::TRANSPARENT, stroke: Stroke::NONE, icon };
    let style = IconButtonStyle {
        normal: clear(t.icon),
        hovered: clear(t.icon),
        pressed: Some(clear(t.icon)),
        selected: clear(t.accent_text),
        corner_radius: t.radius.into(),
        focus_stroke: Stroke::new(1.0, t.accent),
    };
    let label = if label.is_empty() { name } else { label };
    // The wash eases from the response, so reserve its place under the icon and focus ring.
    let wash = ui.painter().add(egui::Shape::Noop);
    let mut painted = None;
    let resp = IconButton::new(label, Vec2::splat(box_size), &style).selected(selected).show(ui, |_, rect, tint| painted = Some((rect, tint)));
    if let Some((rect, tint)) = painted {
        let press = theme::Press::track(ui, &resp);
        let mut shapes = Vec::with_capacity(2);
        let fill = press.wash_color(&t, selected);
        if fill.a() > 0 {
            shapes.push(egui::Shape::rect_filled(rect, t.radius, fill));
        }
        if selected {
            shapes.push(egui::Shape::rect_stroke(
                rect.shrink(0.5),
                t.radius,
                Stroke::new(1.0, t.accent.gamma_multiply(0.45)),
                egui::StrokeKind::Inside,
            ));
        }
        ui.painter().set(wash, egui::Shape::Vec(shapes));
        // The glyph shrinks a little and drops a pixel while the pointer is down.
        let size = (box_size * 0.5).round() * (1.0 - 0.08 * press.down);
        paint(ui, rect.translate(press.offset()), name, size, tint);
    }
    let resp = theme::hand(resp);
    if tooltip.is_empty() { resp } else { resp.on_hover_text(tooltip) }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pixel_icons_cover_whole_screen_pixels() {
        let px = |size: f32, ppp: f32| pixel_size(size, ppp) * ppp;
        // 16 points at 150 % is exactly the grid; at 100 % it grows to the grid (no lost lines).
        assert!((px(16.0, 1.5) - 24.0).abs() < 1e-3);
        assert!((px(16.0, 1.0) - 24.0).abs() < 1e-3);
        assert!((px(17.0, 1.25) - 24.0).abs() < 1e-3);
        assert!((px(18.0, 2.0) - 48.0).abs() < 1e-3);
        // 10 points at 100 %: 24 would be too large for its place, so it stays 10.
        assert!((px(10.0, 1.0) - 10.0).abs() < 1e-3);
        for ppp in [1.0, 1.25, 1.5, 1.75, 2.0] {
            for size in [8.0, 10.0, 12.0, 14.0, 16.0, 17.0, 18.0, 20.0, 24.0] {
                let got = pixel_size(size, ppp);
                assert!(got >= size * 0.87 && got <= size * 1.51, "{size} at {ppp}: {got}");
            }
        }
        assert!(pixel_size(f32::NAN, 1.0).is_nan());
        assert_eq!(pixel_size(16.0, 0.0), 16.0);
    }

    #[test]
    fn every_lucide_icon_has_a_pixel_counterpart() {
        let pixel: std::collections::HashSet<&str> = PIXEL_ICONS.iter().map(|(n, _)| *n).collect();
        for (name, _) in ICONS {
            assert!(pixel.contains(name), "{name} has no Pixelarticons counterpart");
        }
        assert_eq!(PIXEL_ICONS.len(), ICONS.len(), "a pixel icon stands for no Lucide icon");
        for (name, svg) in PIXEL_ICONS {
            let text = std::str::from_utf8(svg).unwrap_or_default();
            assert!(text.contains("viewBox=\"0 0 24 24\"") && text.contains("currentColor"), "{name}");
        }
    }

    #[test]
    fn every_catalog_icon_exists() {
        for g in pdfcraft_engine::catalog::TOOL_GROUPS {
            assert!(super::exists(g.icon), "missing icon {}", g.icon);
            for s in g.sections {
                for i in s.items {
                    assert!(super::exists(i.icon), "missing icon {}", i.icon);
                }
            }
        }
    }

    #[test]
    fn every_command_icon_exists() {
        for c in pdfcraft_engine::commands::COMMANDS {
            assert!(super::exists(c.icon), "missing icon {} ({})", c.icon, c.id);
        }
    }

    #[test]
    fn shared_catalog_preserves_existing_icons_and_fallback_uri() {
        assert_eq!(ICONS.len(), 191);
        assert!(exists("minimize-2"));
        for (name, original) in ICONS {
            let expected =
                String::from_utf8_lossy(original).replace("currentColor", "#ffffff").replace("stroke-width=\"2\"", "stroke-width=\"1.75\"");
            let egui::ImageSource::Bytes { uri, bytes } = source(name) else { panic!("expected SVG bytes") };
            assert_eq!(uri.as_ref(), format!("bytes://icons/{name}.svg"));
            assert_eq!(bytes.as_ref(), expected.as_bytes(), "{name}");
        }
        let egui::ImageSource::Bytes { uri, bytes } = source("missing-icon") else { panic!("expected SVG bytes") };
        let egui::ImageSource::Bytes { bytes: fallback, .. } = source("square") else { panic!("expected SVG bytes") };
        assert_eq!(uri.as_ref(), "bytes://icons/square.svg");
        assert_eq!(bytes.as_ref(), fallback.as_ref());
        assert!(!exists("missing-icon"));
    }

    #[test]
    fn shared_button_preserves_pointer_responses() {
        // The shared button must respond like the original allocate-and-paint path in both
        // themes. Its painting differs on purpose (the eased press wash, the selected outline
        // and the sinking glyph); the keyboard-focus outline is covered in craft-ui.
        fn frame(
            ctx: &egui::Context,
            events: Vec<egui::Event>,
            selected: bool,
            enabled: bool,
            legacy: bool,
        ) -> (egui::Response, Vec<egui::epaint::ClippedShape>) {
            let mut response = None;
            let mut output = ctx.run_ui(egui::RawInput { events, ..Default::default() }, |ui| {
                response = Some(
                    ui.add_enabled_ui(enabled, |ui| {
                        if legacy {
                            let t = Tokens::get(ui.ctx());
                            let (rect, response) = ui.allocate_exact_size(Vec2::splat(28.0), egui::Sense::click());
                            if selected {
                                ui.painter().rect_filled(rect, t.radius, t.accent_soft);
                            } else if response.is_pointer_button_down_on() {
                                ui.painter().rect_filled(rect, t.radius, t.pressed);
                            } else if response.hovered() {
                                ui.painter().rect_filled(rect, t.radius, t.hover);
                            }
                            let tint = if selected { t.accent_text } else { t.icon };
                            paint(ui, rect, "square", (28.0_f32 * 0.5).round(), tint);
                            response
                        } else {
                            button(ui, "square", 28.0, selected, "")
                        }
                    })
                    .inner,
                );
            });
            output.textures_delta.clear();
            (response.unwrap(), output.shapes)
        }
        for theme in [crate::theme::ThemeKind::Dark, crate::theme::ThemeKind::Light] {
            for selected in [false, true] {
                for enabled in [false, true] {
                    let old = egui::Context::default();
                    let new = egui::Context::default();
                    for ctx in [&old, &new] {
                        crate::theme::apply(ctx, theme);
                        egui_extras::install_image_loaders(ctx);
                    }
                    let mut center = egui::Pos2::ZERO;
                    for step in 0..7 {
                        let events = match step {
                            2 => vec![egui::Event::PointerMoved(center)],
                            3 | 4 => vec![egui::Event::PointerButton {
                                pos: center,
                                button: egui::PointerButton::Primary,
                                pressed: step == 3,
                                modifiers: egui::Modifiers::NONE,
                            }],
                            5 => vec![egui::Event::PointerMoved(egui::pos2(300.0, 200.0))],
                            _ => vec![],
                        };
                        let (a, _) = frame(&old, events.clone(), selected, enabled, true);
                        let (b, after) = frame(&new, events, selected, enabled, false);
                        center = a.rect.center();
                        assert_eq!(a.rect, b.rect);
                        assert_eq!(
                            (a.clicked(), a.hovered(), a.is_pointer_button_down_on()),
                            (b.clicked(), b.hovered(), b.is_pointer_button_down_on())
                        );
                        assert!(!after.is_empty(), "{theme:?}, selected={selected}, enabled={enabled}, step={step}");
                    }
                }
            }
        }
    }
}
