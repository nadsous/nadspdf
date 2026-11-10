//! NadsPDF: Window › Output › Separations Preview is left out with print production; the panel
//! (should a saved layout still name it) says so.

use crate::DesignApp;
use crate::theme::Tokens;

pub fn show(app: &mut DesignApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    ui.label(egui::RichText::new(crate::i18n::tr(&app.ui.language, crate::separations::UNAVAILABLE)).color(t.text_dim));
}
