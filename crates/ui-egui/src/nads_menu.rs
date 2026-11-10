//! NadsPDF's main menu: the nads mark at the tab strip's left end opens the app's menus (File,
//! Edit, View…), as nads.site's brand opens its menu.
//!
//! It is craft-menu's own menu bar, folded entirely into its overflow menu: the bar is given no
//! room, so every menu moves into the overflow title, which this module labels "Menu" and draws
//! as the nads mark. Items, states, shortcuts, keyboard navigation and accessibility stay those of
//! the shared menu bar ([`PdfCraftApp::menu_bar`]).

use craft_menu::{BarStyle, MenuBarUi, Target};
use egui::{Rect, pos2, vec2};

use crate::theme::Tokens;
use crate::{PdfCraftApp, icons};

/// The label craft-menu gives the overflow title (U+22EF), which here is the whole menu.
const OVERFLOW: &str = "\u{22ef}";

/// Menu titles and items in the interface language; the overflow title is "Menu".
fn label(english: &'static str) -> String {
    if english == OVERFLOW { tl!("Menu").to_owned() } else { crate::menus::translate(english) }
}

impl PdfCraftApp {
    /// The nads mark and its menu (`size`: the strip's button size). Hidden when the native macOS
    /// menu bar has the menus, and when the active document hides the menu bar (`HideMenubar`,
    /// ISO 32000-2 Table 147; F8 brings it back): the mark is NadsPDF's menu bar.
    pub(crate) fn nads_menu(&mut self, ui: &mut egui::Ui, size: f32) {
        if self.native_menu.is_some() || self.hidden_interface().menubar {
            return;
        }
        let t = Tokens::get(ui.ctx());
        let ctx = ui.ctx().clone();
        let bar = craft_menu::layout(&self.menu_bar(&ctx), Target::current(), &crate::menus::layout_options()).bar;
        let gate = self.menu_gate(&ctx);
        // The title's text is drawn invisibly: the mark takes its place.
        let style = BarStyle { title_color: egui::Color32::TRANSPARENT, ..BarStyle::from_style(ui.style()) };
        let out = ui.scope(|ui| {
            ui.set_max_width(size + 14.0);
            MenuBarUi::new(&bar).gate(gate).style(style).label(&label).id(egui::Id::unique("nadspdf-menu")).shrink_to_fit(0.0).show(ui)
        });
        let rect = out.response.rect;
        let open = craft_menu::bar::is_open(&ctx);
        let mark = Rect::from_center_size(rect.center() - vec2(3.0, 0.0), vec2(18.0, 18.0));
        egui::Image::from_bytes("bytes://nads-n.svg", include_bytes!("../../../assets/app-icon/nads-n.svg")).paint_at(ui, mark);
        // A small chevron says the mark opens a menu.
        let hovered = ui.rect_contains_pointer(rect);
        let tint = if hovered || open { t.text } else { t.text_faint };
        icons::paint(ui, Rect::from_center_size(pos2(mark.right() + 6.0, mark.bottom() - 2.0), vec2(9.0, 9.0)), "chevron-down", 9.0, tint);
        for command in out.inner {
            self.run_menu_command(&ctx, &command);
        }
    }
}
