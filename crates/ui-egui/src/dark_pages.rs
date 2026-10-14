//! View ▸ Dark pages (⇧⌘D / Ctrl+Shift+D): pages shown with their lightness inverted, a
//! preference that every open and future view follows and that survives a restart. The pixels
//! are changed once per rendered raster as it arrives ([`crate::canvas::dark_pixel`]); the
//! document, printing and exports never are.

use crate::PdfCraftApp;
use crate::canvas::DarkPages;

impl PdfCraftApp {
    /// Apply a Dark pages preference to the defaults and every open view.
    pub fn set_dark_pages(&mut self, dark: DarkPages) {
        self.view_defaults.dark = dark;
        for v in &mut self.views {
            v.set_dark(dark);
        }
        if let Some(ctx) = &self.ctx {
            ctx.request_repaint();
        }
    }

    /// View ▸ Dark pages.
    pub(crate) fn toggle_dark_pages(&mut self) {
        let dark = self.view_defaults.dark;
        self.set_dark_pages(DarkPages { pages: !dark.pages, ..dark });
    }

    /// `set dark-pages on|off` and `set dark-thumbnails on|off` (CLI shots, the control channel).
    pub(crate) fn set_dark_option(&mut self, key: &str, value: &str) -> Result<(), String> {
        let on = match value {
            "on" | "true" => true,
            "off" | "false" => false,
            _ => return Err(format!("{key} must be on or off")),
        };
        let mut dark = self.view_defaults.dark;
        if key == "dark-thumbnails" {
            dark.thumbnails = on;
        } else {
            dark.pages = on;
        }
        self.set_dark_pages(dark);
        Ok(())
    }

    /// Restore Dark pages and the Rasterize level from settings; anything malformed is ignored.
    pub(crate) fn restore_view_extras(&mut self, v: &serde_json::Value) {
        let mut dark = self.view_defaults.dark;
        if let Some(on) = v["dark_pages"].as_bool() {
            dark.pages = on;
        }
        if let Some(on) = v["dark_thumbnails"].as_bool() {
            dark.thumbnails = on;
        }
        self.set_dark_pages(dark);
        if let Some(level) = v["raster_level"].as_str().and_then(pdfcraft_engine::rasterize::RasterLevel::from_id) {
            self.raster_level = level;
        }
    }
}

#[cfg(test)]
mod tests {
    use egui::Color32;

    use super::*;
    use crate::canvas::dark_pixel;

    #[test]
    fn dark_pixels_invert_lightness_and_keep_hue() {
        let paper = dark_pixel(Color32::WHITE);
        let ink = dark_pixel(Color32::BLACK);
        assert!(paper.r() < 0x30 && paper.r() == paper.g() && paper.g() == paper.b(), "{paper:?}");
        assert!(ink.r() > 0xD0 && ink.r() == ink.g() && ink.g() == ink.b(), "{ink:?}");
        // Pure red keeps its lightness (50%) and its hue.
        let red = dark_pixel(Color32::from_rgb(255, 0, 0));
        assert!(red.r() > 0xC0 && red.g() < 0x30 && red.b() < 0x30, "{red:?}");
        // A light blue tint becomes a dark blue one.
        let blue = dark_pixel(Color32::from_rgb(0xDD, 0xEE, 0xFF));
        assert!(blue.b() > blue.r() && blue.r() < 0x60, "{blue:?}");
        // Transparent stays transparent; premultiplied channels never exceed alpha.
        assert_eq!(dark_pixel(Color32::TRANSPARENT), Color32::TRANSPARENT);
        for a in [1u8, 77, 128, 254] {
            let p = dark_pixel(Color32::from_rgba_premultiplied(a, a / 2, 0, a));
            assert!(p.r() <= a && p.g() <= a && p.b() <= a && p.a() == a, "{p:?}");
        }
    }

    #[test]
    fn dark_pages_follow_every_view_and_survive_a_restart() {
        let mut app = PdfCraftApp::new();
        app.set_option("language", "en").unwrap();
        app.open_bytes("notes.txt", None, b"hello".to_vec()).unwrap();
        assert!(!app.views[0].dark.pages);
        assert!(app.execute("view.dark_pages"));
        assert!(app.views[0].dark.pages && app.view_defaults.dark.pages);
        // A document opened now starts dark too.
        app.open_bytes("more.txt", None, b"more".to_vec()).unwrap();
        assert!(app.views.iter().all(|v| v.dark.pages));
        app.set_option("dark-thumbnails", "off").unwrap();
        assert!(app.set_option("dark-pages", "maybe").is_err());

        let saved = app.persist();
        let mut next = PdfCraftApp::new();
        next.restore(&saved);
        assert_eq!(next.view_defaults.dark, DarkPages { pages: true, thumbnails: false });
        // Malformed settings are ignored.
        next.restore(r#"{"dark_pages": "yes", "raster_level": 3}"#);
        assert!(next.view_defaults.dark.pages);

        app.set_option("dark-pages", "off").unwrap();
        assert!(app.views.iter().all(|v| !v.dark.pages));
    }
}
