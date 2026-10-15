//! Measure objects: page-space clicks, live readings and persistent drawing scales.
use crate::{LeftPanel, PdfCraftApp, QuickTool, canvas::DocView, theme::Tokens, widgets};
use egui::{Color32, Pos2, Stroke};
use pdfcraft_engine::{
    Document, Edit, Style,
    measure::{self, Kind, NewMeasurement, Point, Reading, Scale},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Distance,
    Perimeter,
    Area,
    Calibrate,
    /// Point at a line of the drawing to read its length; a click saves it as a distance.
    Element,
}
impl Tool {
    pub fn name(self) -> &'static str {
        match self {
            Self::Distance => "distance",
            Self::Perimeter => "perimeter",
            Self::Area => "area",
            Self::Calibrate => "calibrate",
            Self::Element => "element",
        }
    }
    pub fn from_name(s: &str) -> Option<Self> {
        match s {
            "distance" => Some(Self::Distance),
            "perimeter" => Some(Self::Perimeter),
            "area" => Some(Self::Area),
            "calibrate" => Some(Self::Calibrate),
            "element" => Some(Self::Element),
            _ => None,
        }
    }
    fn kind(self) -> Kind {
        match self {
            Self::Area => Kind::Area,
            Self::Perimeter => Kind::Perimeter,
            _ => Kind::Distance,
        }
    }
}
/// Lengths a drawing scale is written in: the label stored in the PDF, and how many make a point.
const LENGTHS: [(&str, f64); 9] = [
    ("mm", 25.4 / 72.0),
    ("cm", 2.54 / 72.0),
    ("m", 0.0254 / 72.0),
    ("km", 0.0000254 / 72.0),
    ("in", 1.0 / 72.0),
    ("ft", 1.0 / 864.0),
    ("yd", 1.0 / 2592.0),
    ("mi", 1.0 / 4_561_920.0),
    ("pt", 1.0),
];
/// Usual plan ratios, 1:N.
const RATIOS: [u32; 14] = [1, 2, 5, 10, 20, 25, 50, 100, 200, 250, 500, 1000, 2000, 5000];

/// How many `unit` make one PDF point, for the units the scale editor offers.
fn per_point(unit: &str) -> Option<f64> {
    LENGTHS.iter().find(|(u, _)| *u == unit).map(|(_, v)| *v)
}

/// `v` without trailing zeros: "1", "0.5", "2.54".
fn number(v: f64) -> String {
    let s = format!("{v:.6}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-" { "0".into() } else { s.into() }
}

/// The ratio a plan's scale is written as, "1 cm = 2.5 m", and as 1:N when both sides are lengths.
fn ratio_text(paper: f64, paper_unit: &str, real: f64, real_unit: &str) -> String {
    let words = format!("{} {paper_unit} = {} {real_unit}", number(paper), number(real));
    match (per_point(paper_unit), per_point(real_unit)) {
        (Some(p), Some(r)) if paper > 0.0 && p > 0.0 && real_unit != "pt" && paper_unit != "pt" => {
            // Both in points: real / paper.
            let n = (real / r) / (paper / p);
            if n.is_finite() && n > 0.0 { format!("{words} (1:{})", number((n * 1000.0).round() / 1000.0)) } else { words }
        }
        _ => words,
    }
}

pub struct MeasureView {
    pub points: Vec<Point>,
    pub page: Option<usize>,
    pub tool: Option<Tool>,
    pub reading: Option<Reading>,
    pub scale: Option<Scale>,
    pub error: Option<String>,
    pub snaps: measure::snap::SnapOptions,
    pub snap_enabled: bool,
    pub sensitivity: f64,
    pub drawing_points: f64,
    pub real_distance: f64,
    pub unit: String,
    pub precision: u8,
    pub name: String,
    pub whole_page: bool,
    /// Apply the scale to every page, not only the one showing.
    pub all_pages: bool,
    /// The unit the drawing side of the scale is shown in (the stored value is `drawing_points`).
    pub paper_unit: String,
    /// The saved measurements listed: the whole document's, or only the page showing.
    pub list_all: bool,
    pub rect: [f64; 4],
    pub label: String,
    seeded_page: Option<usize>,
    /// Snapping geometry (or why it failed) per (page, edit generation): extraction is not
    /// repeated on every hover frame.
    paths: Option<(usize, u64, Result<measure::snap::Geometry, String>)>,
    /// Saved measurements per edit generation, for the panel.
    listing: Option<(u64, Result<measure::Listing, String>)>,
}
impl Default for MeasureView {
    fn default() -> Self {
        Self {
            points: Vec::new(),
            page: None,
            tool: None,
            reading: None,
            scale: None,
            error: None,
            snaps: Default::default(),
            snap_enabled: true,
            sensitivity: 6.0,
            drawing_points: 72.0,
            real_distance: 1.0,
            unit: "in".into(),
            precision: 2,
            name: "Drawing scale".into(),
            whole_page: true,
            all_pages: false,
            paper_unit: "cm".into(),
            list_all: false,
            rect: [0.0, 0.0, 100.0, 100.0],
            label: String::new(),
            seeded_page: None,
            paths: None,
            listing: None,
        }
    }
}
impl MeasureView {
    pub fn cancel(&mut self) {
        self.points.clear();
        self.page = None;
        self.reading = None;
        self.scale = None;
    }
}
pub(crate) fn page_input(
    ui: &egui::Ui,
    resp: &egui::Response,
    doc: &Document,
    view: &mut DocView,
    cx: &crate::comments::PageCx<'_>,
    tool: Tool,
    units: crate::units::Unit,
) {
    let page = cx.page;
    let xf = cx.xf;
    let author = &cx.prefs.author;
    if !doc.allows_annotation() {
        return;
    }
    let Some(info) = doc.info.pages.get(page) else { return };
    let state = &mut view.measure;
    if state.tool != Some(tool) {
        state.cancel();
        state.tool = Some(tool);
    }
    // Keys belong to a focused text field (the label, unit or viewport name) when there is one.
    let typing = ui.ctx().egui_wants_keyboard_input();
    if !typing && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        state.cancel();
    }
    let screen = |p: Point| {
        let v = info.user_to_view(p[0] as f32, p[1] as f32);
        xf.norm_to_screen(v[0] / xf.pw, v[1] / xf.ph)
    };
    let pos = ui.input(|i| i.pointer.hover_pos()).filter(|p| xf.rect.contains(*p));
    if tool == Tool::Element {
        let Some(pos) = pos else { return };
        let (vx, vy) = xf.screen_to_view(pos);
        let [x, y] = info.view_to_user(vx, vy);
        let at = [f64::from(x), f64::from(y)];
        refresh_paths(state, doc, page);
        // A few screen pixels around the pointer, in user units, whatever the zoom.
        let pixels_per_unit = f64::from((xf.rect.width() / xf.pw).max(1e-6));
        let reach = state.sensitivity.max(4.0) / pixels_per_unit;
        let tolerance = doc.measurement_to_user(page, [f64::from(vx) + reach, f64::from(vy)]).map(|p| measure::distance(at, p)).unwrap_or(reach);
        let line = match &state.paths {
            Some((_, _, Ok(paths))) => paths.nearest_line(at, tolerance),
            Some((_, _, Err(e))) => {
                state.error = Some(e.clone());
                None
            }
            None => None,
        };
        let Some(line) = line else { return };
        let measured = doc
            .measurement_scale_or(page, line[0], units.per_point(), units.label())
            .and_then(|scale| measure::reading(Kind::Distance, &line, &scale).map(|r| (scale, r)).map_err(|e| e.to_string()));
        let (scale, reading) = match measured {
            Ok(m) => m,
            Err(e) => {
                state.error = Some(e);
                return;
            }
        };
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        let color = Color32::from_rgb(239, 62, 54);
        let (a, b) = (screen(line[0]), screen(line[1]));
        ui.painter().line_segment([a, b], Stroke::new(3.0, color));
        ui.painter().circle_filled(a, 3.5, color);
        ui.painter().circle_filled(b, 3.5, color);
        let galley = ui.painter().layout_no_wrap(reading.label.clone(), crate::theme::semibold(12.5), Color32::WHITE);
        let tag = egui::Rect::from_min_size(pos + egui::vec2(14.0, 14.0), galley.size() + egui::vec2(12.0, 6.0));
        ui.painter().rect_filled(tag, 4, color);
        ui.painter().galley(tag.min + egui::vec2(6.0, 3.0), galley, Color32::WHITE);
        if resp.clicked() {
            view.pending_edit = Some(Edit::AddMeasurement(NewMeasurement {
                page,
                kind: Kind::Distance,
                points: line.to_vec(),
                scale: scale.clone(),
                style: Style { color: [0.0, 0.47, 0.84], ..Style::default() },
                label: state.label.clone(),
                author: author.into(),
            }));
        }
        state.error = None;
        state.scale = Some(scale);
        state.reading = Some(reading);
        return;
    }
    let mut hover = None;
    let mut snap = None;
    if let Some(pos) = pos {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
        let (vx, vy) = xf.screen_to_view(pos);
        let [x, y] = info.view_to_user(vx, vy);
        let mut point = [f64::from(x), f64::from(y)];
        if state.snap_enabled {
            refresh_paths(state, doc, page);
            let pixels_per_unit = (xf.rect.width() / xf.pw).max(1e-6);
            if let Some((_, _, Err(e))) = &state.paths {
                state.error = Some(e.clone());
            }
            if let Some((_, _, Ok(paths))) = &state.paths {
                if paths.truncated {
                    state.error = Some(tl!("This drawing exceeds the snapping limit; only the first paths are used.").into());
                } else if paths.unreadable > 0 {
                    state.error = Some(tl!("Some page content couldn't be read; snapping uses the rest.").into());
                }
                let tolerance = doc
                    .measurement_to_user(page, [f64::from(vx) + state.sensitivity / f64::from(pixels_per_unit), f64::from(vy)])
                    .map(|p| measure::distance(point, p))
                    .unwrap_or(state.sensitivity / f64::from(pixels_per_unit));
                if state.snaps.intersections && paths.intersection_limited(point, tolerance) {
                    state.error = Some(tl!("This area has too many overlapping paths; intersection snapping is limited.").into());
                }
                if let Ok(Some(hit)) = paths.snap(point, tolerance, state.snaps) {
                    point = hit.point;
                    snap = Some(hit.kind);
                }
            }
        }
        if ui.input(|i| i.modifiers.shift)
            && let Some(last) = state.points.last()
        {
            point = measure::snap::constrain(*last, point);
        }
        hover = Some(point);
        if resp.clicked() {
            if state.page != Some(page) {
                state.cancel();
                state.page = Some(page);
            }
            if state.points.is_empty() {
                match doc.measurement_scale_or(page, point, units.per_point(), units.label()) {
                    Ok(scale) => {
                        state.scale = Some(scale);
                        state.error = None;
                    }
                    Err(e) => {
                        state.error = Some(e);
                        return;
                    }
                }
            }
            let closes =
                tool == Tool::Area && state.points.len() >= 3 && state.points.first().is_some_and(|first| screen(*first).distance(pos) <= 6.0);
            let repeats = state.points.last().is_some_and(|last| measure::distance(*last, point) < 1e-6);
            if !closes && !repeats && state.points.len() < measure::MAX_POINTS {
                state.points.push(point);
            }
            let finishes = closes || matches!(tool, Tool::Distance | Tool::Calibrate) && state.points.len() == 2;
            if finishes {
                finish(state, &mut view.pending_edit, doc, page, tool, author);
            }
        }
    }
    if state.page == Some(page) && !state.points.is_empty() {
        if (resp.double_clicked() || !typing && ui.input(|i| i.key_pressed(egui::Key::Enter))) && matches!(tool, Tool::Perimeter | Tool::Area) {
            finish(state, &mut view.pending_edit, doc, page, tool, author);
        }
        if !typing && ui.input(|i| i.key_pressed(egui::Key::Backspace)) {
            state.points.pop();
        }
        let mut points = state.points.clone();
        if let Some(p) = hover
            && points.last().is_none_or(|last| measure::distance(*last, p) > 1e-6)
        {
            points.push(p);
        }
        if let Some(scale) = &state.scale {
            match measure::reading(tool.kind(), &points, scale) {
                Ok(reading) => state.reading = Some(reading),
                Err(e) => state.error = Some(e.to_string()),
            }
        }
        let positions: Vec<Pos2> = points.iter().map(|&p| screen(p)).collect();
        let stroke = Stroke::new(1.5, Color32::from_rgb(0, 120, 214));
        for pair in positions.windows(2) {
            if let [a, b] = pair {
                ui.painter().line_segment([*a, *b], stroke);
            }
        }
        if tool == Tool::Area
            && let (Some(a), Some(b)) = (positions.first(), positions.last())
        {
            ui.painter().line_segment([*a, *b], stroke);
        }
        for &p in &positions {
            ui.painter().circle_filled(p, 3.0, stroke.color);
        }
        if let (Some(p), Some(kind)) = (hover, snap) {
            ui.painter().circle_stroke(screen(p), 6.0, Stroke::new(1.0, Color32::from_rgb(46, 158, 92)));
            ui.painter().text(
                screen(p) + egui::vec2(10.0, -10.0),
                egui::Align2::LEFT_BOTTOM,
                tl!(match kind {
                    measure::snap::SnapKind::Endpoint => "Endpoint",
                    measure::snap::SnapKind::Midpoint => "Midpoint",
                    measure::snap::SnapKind::Intersection => "Intersection",
                    measure::snap::SnapKind::Path => "Path",
                }),
                crate::theme::regular(11.0),
                stroke.color,
            );
        }
        if let (Some(p), Some(reading)) = (hover, &state.reading) {
            ui.painter().text(screen(p) + egui::vec2(12.0, 12.0), egui::Align2::LEFT_TOP, &reading.label, crate::theme::semibold(12.0), stroke.color);
        }
    }
}
/// The page's snapping geometry, extracted once per page and edit.
fn refresh_paths(state: &mut MeasureView, doc: &Document, page: usize) {
    if state.paths.as_ref().is_none_or(|(p, g, _)| *p != page || *g != doc.edit_generation()) {
        state.paths = Some((page, doc.edit_generation(), doc.measurement_paths(page)));
    }
}
fn finish(state: &mut MeasureView, pending: &mut Option<Edit>, doc: &Document, page: usize, tool: Tool, author: &str) {
    if tool == Tool::Calibrate {
        if let [a, b] = state.points.as_slice() {
            let a = doc.measurement_to_view(page, *a);
            let b = doc.measurement_to_view(page, *b);
            if let (Ok(a), Ok(b)) = (a, b) {
                state.drawing_points = measure::distance(a, b);
            }
        }
        state.cancel();
        return;
    }
    let minimum = if tool == Tool::Area { 3 } else { 2 };
    if state.points.len() < minimum {
        state.error = Some(crate::i18n::fmt(tl!("This measurement needs at least {n} points."), &[("n", &minimum.to_string())]));
        return;
    }
    if let Some(scale) = state.scale.clone() {
        *pending = Some(Edit::AddMeasurement(NewMeasurement {
            page,
            kind: tool.kind(),
            points: state.points.clone(),
            scale,
            style: Style { color: [0.0, 0.47, 0.84], ..Style::default() },
            label: state.label.clone(),
            author: author.into(),
        }));
        state.cancel();
    }
}
pub(crate) fn panel(app: &mut PdfCraftApp, ui: &mut egui::Ui, t: &Tokens) {
    egui::ScrollArea::vertical().id_salt("measure-panel").show(ui, |ui| panel_body(app, ui, t));
}
fn panel_body(app: &mut PdfCraftApp, ui: &mut egui::Ui, _t: &Tokens) {
    crate::widgets::begin_dialog(ui);
    let Some((index, id)) = app.active_ids() else {
        ui.label(tl!("Open a PDF to measure it."));
        return;
    };
    let mut tool = None;
    ui.horizontal_wrapped(|ui| {
        for (label, value) in [("Distance", Tool::Distance), ("Perimeter", Tool::Perimeter), ("Area", Tool::Area), ("Element", Tool::Element)] {
            if ui.selectable_label(app.quick_tool == QuickTool::Measure(value), tl!(label)).clicked() {
                tool = Some(value);
            }
        }
    });
    let Some(doc) = app.session.get(id) else { return };
    let units = app.units;
    let page = app.views[index].current;
    let info = doc.info.pages.get(page).cloned();
    let pages = doc.info.pages.len();
    let element = app.quick_tool == QuickTool::Measure(Tool::Element);
    let state = &mut app.views[index].measure;
    if state.listing.as_ref().is_none_or(|(g, _)| *g != doc.edit_generation()) {
        state.listing = Some((doc.edit_generation(), doc.measurements()));
    }
    // The middle of the page showing, where a whole-page scale certainly applies.
    let middle = info.as_ref().map_or([0.0, 0.0], |i| [f64::from(i.width) / 2.0, f64::from(i.height) / 2.0]);
    let middle = doc.measurement_to_user(page, middle);
    let page_scale = middle.as_ref().ok().and_then(|&a| doc.measurement_scale_or(page, a, units.per_point(), units.label()).ok());
    // A page without a scale measures true size in whatever unit it is asked for; one with a
    // scale answers the same whatever the unit.
    let scaled = middle.as_ref().ok().is_some_and(|&a| {
        let one = doc.measurement_scale_or(page, a, 1.0, "pt").map(|s| s.ratio);
        let two = doc.measurement_scale_or(page, a, 2.0, "pt").map(|s| s.ratio);
        matches!((one, two), (Ok(x), Ok(y)) if x == y)
    });
    if state.seeded_page != Some(page) {
        state.seeded_page = Some(page);
        let paper = per_point(&state.paper_unit).map_or(72.0, |p| 1.0 / p);
        if let Ok(a) = middle
            && let Ok(b) = doc.measurement_to_view(page, a).and_then(|v| doc.measurement_to_user(page, [v[0] + paper, v[1]]))
            && let Ok(scale) = doc.measurement_scale_or(page, a, units.per_point(), units.label())
            && let Ok(reading) = measure::reading(Kind::Distance, &[a, b], &scale)
        {
            state.drawing_points = paper;
            state.real_distance = reading.value;
            state.unit = scale.unit;
            state.precision = scale.precision;
        }
    }
    if element {
        ui.label(tl!("Point at a line of the drawing to see its length. Click it to keep the measurement."));
    } else {
        ui.label(tl!("Click vertices. Enter or double-click finishes a perimeter or area. Click the first point to close an area."));
        ui.label(tl!("Shift constrains to 45 degrees. Backspace removes a vertex. Escape cancels."));
    }
    ui.separator();
    ui.label(tl!("Measurement information"));
    if let Some(reading) = &state.reading {
        ui.label(egui::RichText::new(&reading.label).strong());
        ui.label(format!("X: {:.3}   Y: {:.3}", reading.delta_x, reading.delta_y));
        ui.label(format!("{}: {:.2}°", tl!("Angle"), reading.angle));
        if let Some(scale) = &state.scale {
            ui.label(&scale.ratio);
        }
    } else {
        ui.label(tl!("Click the first point to begin."));
    }
    ui.horizontal(|ui| {
        ui.label(tl!("Label"));
        ui.add(crate::widgets::line(&mut state.label).desired_width(130.0).char_limit(128));
    });
    ui.separator();
    ui.label(egui::RichText::new(tl!("Drawing scale")).strong());
    let now = match (&page_scale, scaled) {
        (Some(scale), true) => scale.ratio.clone(),
        _ => tl!("Actual size (no scale)").to_string(),
    };
    ui.label(format!("{}: {now}", tl!("This page")));
    let paper_per_point = per_point(&state.paper_unit).unwrap_or(1.0);
    egui::ComboBox::from_id_salt("measure-ratio").selected_text(tl!("Usual scales")).show_ui(ui, |ui| {
        for n in RATIOS {
            if ui.selectable_label(false, format!("1:{n}")).clicked() {
                // One drawing unit stands for n of the same, written in the real-world unit.
                state.drawing_points = 1.0 / paper_per_point;
                state.real_distance = match per_point(&state.unit) {
                    Some(real) => f64::from(n) * real / paper_per_point,
                    None => {
                        state.unit = state.paper_unit.clone();
                        f64::from(n)
                    }
                };
            }
        }
    });
    ui.horizontal(|ui| {
        let mut paper = state.drawing_points * paper_per_point;
        if ui.add(egui::DragValue::new(&mut paper).range(0.000001..=1e9).speed(0.01).max_decimals(4)).changed() {
            state.drawing_points = paper / paper_per_point;
        }
        egui::ComboBox::from_id_salt("measure-paper-unit").width(52.0).selected_text(state.paper_unit.as_str()).show_ui(ui, |ui| {
            for (unit, _) in LENGTHS {
                if ui.selectable_label(state.paper_unit == unit, unit).clicked() {
                    state.paper_unit = unit.into();
                }
            }
        });
        ui.label("=");
        ui.add(egui::DragValue::new(&mut state.real_distance).range(0.000001..=1e12).speed(0.01).max_decimals(4));
        egui::ComboBox::from_id_salt("measure-real-unit").width(52.0).selected_text(state.unit.as_str()).show_ui(ui, |ui| {
            for (unit, _) in LENGTHS {
                if ui.selectable_label(state.unit == unit, unit).clicked() {
                    state.unit = unit.into();
                }
            }
        });
    });
    ui.horizontal(|ui| {
        ui.label(tl!("Other unit"));
        ui.add(crate::widgets::line(&mut state.unit).desired_width(64.0).char_limit(12));
        ui.add(egui::DragValue::new(&mut state.precision).range(0..=6).prefix(format!("{} ", tl!("Decimals"))));
    });
    let ratio = ratio_text(state.drawing_points * paper_per_point, &state.paper_unit, state.real_distance, &state.unit);
    ui.label(egui::RichText::new(&ratio).weak());
    if ui.button(tl!("Calibrate from two points")).on_hover_text(tl!("Click both ends of a length you know, then type it on the right.")).clicked() {
        tool = Some(Tool::Calibrate);
    }
    ui.horizontal(|ui| {
        ui.selectable_value(&mut state.all_pages, false, tl!("This page"));
        ui.selectable_value(&mut state.all_pages, true, tl!("All pages"));
    });
    ui.add(widgets::checkbox(&mut state.whole_page, tl!("Entire page")));
    if !state.whole_page {
        ui.label(tl!("Viewport rectangle in page points (x, y, width, height)"));
        for (i, name) in ["X", "Y", "Width", "Height"].iter().enumerate() {
            if let Some(v) = state.rect.get_mut(i) {
                ui.add(egui::DragValue::new(v).range(0.0..=1e9).prefix(format!("{}: ", tl!(name))));
            }
        }
    }
    ui.horizontal(|ui| {
        ui.label(tl!("Viewport"));
        ui.add(crate::widgets::line(&mut state.name).desired_width(130.0).char_limit(128));
    });
    let apply = ui.add_enabled(doc.allows_annotation(), egui::Button::new(tl!("Apply scale"))).clicked();
    if let Some(error) = &state.error {
        ui.colored_label(Color32::from_rgb(190, 50, 50), error);
    }
    ui.separator();
    ui.horizontal(|ui| {
        ui.selectable_value(&mut state.list_all, false, tl!("This page"));
        ui.selectable_value(&mut state.list_all, true, tl!("Whole document"));
    });
    let list_all = state.list_all;
    let mut select = None;
    match state.listing.as_ref().map(|(_, l)| l) {
        None => {}
        Some(Ok(listing)) => {
            let shown: Vec<&measure::Measurement> = listing.measurements.iter().filter(|m| list_all || m.page == page).collect();
            ui.label(format!("{}: {}", tl!("Saved measurements"), shown.len()));
            if !listing.unsupported.is_empty() {
                let reasons: Vec<String> =
                    listing.unsupported.iter().map(|u| format!("{} {}: {}", tl!("Page"), u.page.saturating_add(1), u.reason)).collect();
                ui.colored_label(Color32::from_rgb(190, 120, 30), format!("{}: {}", tl!("Unsupported measurements"), listing.unsupported.len()))
                    .on_hover_text(reasons.join("\n"));
            }
            for (name, value, unit, decimals) in totals(&shown) {
                ui.label(egui::RichText::new(format!("{}: {value:.decimals$} {unit}", tl!(name))).strong());
            }
            egui::ScrollArea::vertical().id_salt("measurement-list").max_height(220.0).show(ui, |ui| {
                for m in shown {
                    let kind = tl!(match m.reading.kind {
                        Kind::Distance => "Distance",
                        Kind::Perimeter => "Perimeter",
                        Kind::Area => "Area",
                    });
                    let text = if list_all {
                        format!("{} {} · {kind} {} {}", tl!("Page"), m.page.saturating_add(1), m.reading.label, m.label)
                    } else {
                        format!("{kind} {} {}", m.reading.label, m.label)
                    };
                    if ui.button(text.trim_end()).clicked() {
                        select = Some((m.page, m.index, m.reading.clone(), m.scale.clone()));
                    }
                }
            });
        }
        Some(Err(e)) => {
            ui.colored_label(Color32::from_rgb(190, 50, 50), e);
        }
    }
    ui.add(widgets::checkbox(&mut state.snap_enabled, tl!("Snap to drawing")));
    if state.snap_enabled {
        ui.horizontal(|ui| {
            ui.add(widgets::checkbox(&mut state.snaps.endpoints, tl!("Endpoints")));
            ui.add(widgets::checkbox(&mut state.snaps.midpoints, tl!("Midpoints")));
        });
        ui.horizontal(|ui| {
            ui.add(widgets::checkbox(&mut state.snaps.intersections, tl!("Intersections")));
            ui.add(widgets::checkbox(&mut state.snaps.paths, tl!("Paths")));
        });
        ui.add(egui::Slider::new(&mut state.sensitivity, 1.0..=20.0).text(tl!("Sensitivity")));
    }
    ui.separator();
    let export = ui.button(tl!("Export measurements as CSV")).clicked();
    if apply {
        let factor = state.real_distance / state.drawing_points;
        let targets: Vec<usize> = if state.all_pages { (0..pages).collect() } else { vec![page] };
        let mut edits = Vec::new();
        let mut failed = None;
        for target in targets {
            let Some(info) = doc.info.pages.get(target) else { continue };
            let made = doc
                .measurement_to_user(target, [0.0, 0.0])
                .and_then(|a| doc.measurement_to_user(target, [1.0, 0.0]).map(|b| (a, b)))
                .and_then(|(a, b)| Scale::calibrate(a, b, factor, &state.unit, state.precision).map_err(|e| e.to_string()));
            match made {
                Ok(mut scale) => {
                    scale.ratio = ratio.clone();
                    let [x, y, w, h] = if state.whole_page { [0.0, 0.0, f64::from(info.width), f64::from(info.height)] } else { state.rect };
                    let a = info.view_to_user(x as f32, y as f32);
                    let b = info.view_to_user((x + w) as f32, (y + h) as f32);
                    edits.push(Edit::SetMeasurementScale {
                        page: target,
                        bbox: [f64::from(a[0].min(b[0])), f64::from(a[1].min(b[1])), f64::from(a[0].max(b[0])), f64::from(a[1].max(b[1]))],
                        name: state.name.clone(),
                        scale,
                    });
                }
                Err(e) => {
                    failed = Some(e);
                    break;
                }
            }
        }
        match failed {
            Some(e) => state.error = Some(e),
            None if edits.len() == 1 => app.views[index].pending_edit = edits.pop(),
            None if !edits.is_empty() => app.views[index].pending_edit = Some(Edit::Batch { label: "Set measurement scale".into(), edits }),
            None => {}
        }
    }
    if let Some((page, annotation, reading, scale)) = select {
        app.views[index].current = page;
        app.views[index].comments.selected = Some((page, annotation));
        app.views[index].measure.reading = Some(reading);
        app.views[index].measure.scale = Some(scale);
        app.quick_tool = QuickTool::Select;
    }
    if let Some(tool) = tool {
        app.views[index].measure.cancel();
        app.quick_tool = QuickTool::Measure(tool);
    }
    if export {
        export_csv(app);
    }
}
/// The lengths and the areas of `shown` added up, per unit: (wording, total, unit, decimals).
fn totals(shown: &[&measure::Measurement]) -> Vec<(&'static str, f64, String, usize)> {
    let mut out: Vec<(&'static str, f64, String, usize)> = Vec::new();
    for m in shown {
        let name = if m.reading.kind == Kind::Area { "Total area" } else { "Total length" };
        let decimals = usize::from(m.scale.precision);
        match out.iter_mut().find(|(n, _, u, _)| *n == name && *u == m.reading.unit) {
            Some(total) => {
                total.1 += m.reading.value;
                total.3 = total.3.max(decimals);
            }
            None => out.push((name, m.reading.value, m.reading.unit.clone(), decimals)),
        }
    }
    out.retain(|(_, v, _, _)| v.is_finite());
    out
}
pub(crate) fn command(app: &mut PdfCraftApp, id: &str) {
    app.left = LeftPanel::Tool("measure");
    app.left_open = true;
    if let Some((index, _)) = app.active_ids()
        && let Some(tool) = id.strip_prefix("measure.").and_then(Tool::from_name)
    {
        app.views[index].measure.cancel();
        app.quick_tool = QuickTool::Measure(tool);
    }
    if id == "measure.export" {
        export_csv(app);
    }
}
fn export_csv(app: &mut PdfCraftApp) {
    let Some((_, id)) = app.active_ids() else { return };
    let result = app.session.get(id).ok_or_else(|| "no such document".to_string()).and_then(|d| d.measurements());
    match result {
        Err(e) => app.notify(&e),
        Ok(listing) => {
            let csv = measure::csv(&listing.measurements);
            if !listing.unsupported.is_empty() {
                app.notify(crate::i18n::fmt(
                    tl!("{n} measurements use an unsupported format and are left out."),
                    &[("n", &listing.unsupported.len().to_string())],
                ));
            }
            #[cfg(not(target_arch = "wasm32"))]
            {
                let write = move |app: &mut PdfCraftApp, path: std::path::PathBuf| match crate::editing::write_atomically(
                    &path.to_string_lossy(),
                    csv.as_bytes(),
                ) {
                    Ok(()) => app.notify(tl!("Saved measurements.")),
                    Err(e) => app.notify(e.to_string()),
                };
                match app.export_dir_override.as_ref().map(|d| std::path::PathBuf::from(d).join("measurements.csv")) {
                    Some(path) => write(app, path),
                    None => {
                        let dialog = rfd::AsyncFileDialog::new().set_file_name("measurements.csv").add_filter("CSV", &["csv"]);
                        app.ask_one(crate::pickers::Ask::Save(dialog), None, write);
                    }
                }
            }
            #[cfg(target_arch = "wasm32")]
            if let Err(e) = crate::editing::download("measurements.csv", csv.as_bytes()) {
                app.notify(&e);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scales_read_as_plan_ratios() {
        assert_eq!(ratio_text(1.0, "cm", 1.0, "m"), "1 cm = 1 m (1:100)");
        assert_eq!(ratio_text(1.0, "mm", 50.0, "mm"), "1 mm = 50 mm (1:50)");
        assert_eq!(ratio_text(0.25, "in", 1.0, "ft"), "0.25 in = 1 ft (1:48)");
        assert_eq!(ratio_text(10.0, "pt", 1.0, "m"), "10 pt = 1 m", "points are no paper length");
        assert_eq!(ratio_text(1.0, "cm", 3.0, "pas"), "1 cm = 3 pas", "an unknown unit has no ratio");
        assert_eq!(number(2.540000), "2.54");
        assert_eq!(number(0.0), "0");
    }

    #[test]
    fn totals_add_lengths_and_areas_per_unit() {
        let scale = Scale::new(1.0, "m", 2).unwrap();
        let m = |kind, points: Vec<Point>| measure::Measurement {
            page: 0,
            index: 0,
            id: String::new(),
            author: String::new(),
            label: String::new(),
            reading: measure::reading(kind, &points, &scale).unwrap(),
            points,
            scale: scale.clone(),
        };
        let a = m(Kind::Distance, vec![[0.0, 0.0], [3.0, 4.0]]);
        let b = m(Kind::Perimeter, vec![[0.0, 0.0], [2.0, 0.0], [2.0, 1.0]]);
        let c = m(Kind::Area, vec![[0.0, 0.0], [2.0, 0.0], [2.0, 2.0], [0.0, 2.0]]);
        let t = totals(&[&a, &b, &c]);
        assert_eq!(t.len(), 2, "{t:?}");
        assert_eq!((t[0].0, t[0].2.as_str()), ("Total length", "m"));
        assert!((t[0].1 - 8.0).abs() < 1e-9);
        assert_eq!(t[1].0, "Total area");
        assert!((t[1].1 - 4.0).abs() < 1e-9);
    }
}
