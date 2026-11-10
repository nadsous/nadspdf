//! NadsPDF: View › Separations Preview separates plates with print production (craft-prepress),
//! which NadsPDF's Mise en page leaves out (vendor/designcraft/NADSPDF.md). This keeps the names
//! the rest of the interface uses: the view never turns on and nothing is drawn.

use designcraft_tools::CanvasLayout;

use crate::DesignApp;
use crate::canvas::Xf;

/// View › Separations Preview settings (`None` in `UiState::separations` is off; it stays off).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Separations {
    pub plate: Option<String>,
    pub ink_limit: Option<f32>,
}

/// The canvas's separated sheets: none.
#[derive(Default)]
pub struct Preview {
    pub pointer: Option<(usize, designcraft_geom::Point)>,
    pub last_ms: f64,
}

/// Why the view is unavailable, in the words the command and the panel give.
pub const UNAVAILABLE: &str = "Separations Preview needs print production, which NadsPDF doesn't include.";

/// The preview's state for `ui.inspect`: always off.
pub fn inspect(_app: &DesignApp) -> serde_json::Value {
    serde_json::Value::Null
}

/// Nothing to draw over the spreads.
pub fn paint(_app: &mut DesignApp, _ctx: &egui::Context, _painter: &egui::Painter, _xf: &Xf, _layout: &CanvasLayout, _screen: egui::Rect) {}
