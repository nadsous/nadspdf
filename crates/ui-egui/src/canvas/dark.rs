//! NadsPDF's View ▸ Dark pages, on the page view: pages shown with their lightness inverted
//! (dark paper, light text) while hues stay recognisable. Display only: the document is never
//! changed, and printing and exports are not affected. The app-wide preference lives in
//! `crate::dark_pages`.

use egui::Color32;

use super::DocView;
use super::PRINT_TAG;
use super::cache::{GRID_TAG, STALE_TAG, THUMB_TAG};

/// View ▸ Dark pages, as an app-wide preference every view follows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DarkPages {
    pub pages: bool,
    /// Whether page thumbnails (and the Organize grid) follow.
    pub thumbnails: bool,
}

impl DarkPages {
    /// The blank paper drawn under a page (or a thumbnail) until its raster arrives; `None`
    /// when pages show as they are.
    pub(crate) fn paper(self, thumbnail: bool) -> Option<Color32> {
        (self.pages && (self.thumbnails || !thumbnail)).then(|| dark_pixel(Color32::WHITE))
    }
}

impl Default for DarkPages {
    fn default() -> Self {
        Self { pages: false, thumbnails: true }
    }
}

/// The darkest and lightest values a dark page uses: near-black paper and off-white ink are
/// easier on the eyes than pure black and white.
const DARK_PAPER: u32 = 0x1C;
const DARK_INK: u32 = 0xE4;

/// One premultiplied pixel of a dark page: its HSL lightness inverted (`L' = 1 − L`) with hue and
/// saturation kept, which is `c + a − (max + min)` per channel, then squeezed into
/// [`DARK_PAPER`]..[`DARK_INK`]. White paper becomes dark grey, black text light grey, and a red
/// stamp stays red.
pub(crate) fn dark_pixel(p: Color32) -> Color32 {
    let [r, g, b, a] = p.to_array().map(u32::from);
    // Premultiplied channels never exceed alpha; clamp in case a renderer rounds over.
    let (r, g, b) = (r.min(a), g.min(a), b.min(a));
    let sum = r.max(g).max(b) + r.min(g).min(b);
    let squeeze = |c: u32| {
        // `c + a − sum` is within 0..=a since min ≤ c ≤ max ≤ a; saturate anyway.
        let v = (c + a).saturating_sub(sum).min(a);
        ((DARK_PAPER * a + v * (DARK_INK - DARK_PAPER)) / 255).min(a) as u8
    };
    Color32::from_rgba_premultiplied(squeeze(r), squeeze(g), squeeze(b), a as u8)
}

/// Turn a rendered page into a dark page, in place.
pub(crate) fn darken_image(img: &mut egui::ColorImage) {
    for p in &mut img.pixels {
        *p = dark_pixel(*p);
    }
}

impl DocView {
    /// Follow View ▸ Dark pages. Pages (and thumbnails, when they follow) are re-rendered; the
    /// old rasters stay on screen until their replacements arrive.
    pub fn set_dark(&mut self, dark: DarkPages) {
        if dark == self.dark {
            return;
        }
        let pages_change = dark.pages != self.dark.pages;
        let thumbs_change = dark.pages && dark.thumbnails != self.dark.thumbnails || pages_change && (dark.thumbnails || self.dark.thumbnails);
        self.dark = dark;
        if pages_change {
            for p in self.pages.values_mut() {
                p.tag = STALE_TAG;
            }
            self.tiles.clear();
            self.stale_tiles.clear();
            self.waiting_since.clear();
        }
        if thumbs_change {
            self.stale_thumbs.extend(self.thumbs.keys().copied());
            self.grid_pages.clear();
        }
        self.last_queue.clear();
    }

    /// Whether a raster with render tag `tag` is shown dark.
    pub(super) fn shows_dark(&self, tag: u64) -> bool {
        if tag & PRINT_TAG != 0 {
            return false;
        }
        self.dark.pages && (self.dark.thumbnails || tag & (THUMB_TAG | GRID_TAG) == 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canvas::ViewDefaults;
    use pdfcraft_engine::DocId;

    #[test]
    fn dark_pages_cover_screen_rasters_but_never_print_previews() {
        let info = crate::canvas::tests::thumbnail_info(3);
        let mut v = DocView::new(DocId(1), &info, ViewDefaults::default());
        assert!(!v.shows_dark(0));
        v.set_dark(DarkPages { pages: true, thumbnails: true });
        assert!(v.shows_dark(0) && v.shows_dark(THUMB_TAG) && v.shows_dark(GRID_TAG));
        assert!(!v.shows_dark(PRINT_TAG), "print previews show the paper as printed");
        v.set_dark(DarkPages { pages: true, thumbnails: false });
        assert!(v.shows_dark(0) && !v.shows_dark(THUMB_TAG) && !v.shows_dark(GRID_TAG));
        assert_eq!(v.dark.paper(false), Some(dark_pixel(Color32::WHITE)));
        assert_eq!(v.dark.paper(true), None);
        let mut img = egui::ColorImage::filled([2, 2], Color32::WHITE);
        darken_image(&mut img);
        assert!(img.pixels.iter().all(|p| *p == dark_pixel(Color32::WHITE)));
    }
}
