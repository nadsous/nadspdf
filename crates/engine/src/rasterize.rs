//! Compress a PDF ▸ Rasterize pages: the strongest compression, for scans and image-heavy files.
//!
//! Every page is rendered as it is displayed (comments, form fields and visible layers included)
//! at a resolution, encoded as a JPEG, and a new PDF is built with one full-page image per page at
//! the page's displayed size. Text is no longer selectable or searchable, links and bookmarks are
//! gone, and the copy is never encrypted: it is a picture of the document.
//!
//! Like [`crate::optimizer`], it is split so the UI can run it on a worker thread:
//! [`Session::rasterize_job`] captures what is needed, and [`RasterizeJob::run`] reports each page
//! and stops when asked to.

use std::sync::Arc;

use pdfcraft_cos::{SaveOptions, write_full};

use crate::export::{ExportSource, Exporter, over_white};
use crate::optimizer::{OptimizeStage, Optimized};
use crate::{DocId, EditError, OptimizeReport, Session, guard};

/// The most JPEG data a rasterized copy may hold before it is refused (2 GiB).
const MAX_OUTPUT: usize = 2 << 30;

/// The three ready-made strengths.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RasterLevel {
    /// 150 dpi, JPEG quality 80: comfortable to read and print.
    Light,
    /// 110 dpi, quality 65.
    #[default]
    Medium,
    /// 80 dpi, quality 50: smallest, for on-screen reading.
    Strong,
}

impl RasterLevel {
    pub const ALL: [RasterLevel; 3] = [RasterLevel::Light, RasterLevel::Medium, RasterLevel::Strong];

    /// A stable id (agents, saved settings).
    pub fn id(self) -> &'static str {
        match self {
            RasterLevel::Light => "light",
            RasterLevel::Medium => "medium",
            RasterLevel::Strong => "strong",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|l| l.id() == id)
    }

    /// The English label (translated by the interface).
    pub fn label(self) -> &'static str {
        match self {
            RasterLevel::Light => "Light",
            RasterLevel::Medium => "Medium",
            RasterLevel::Strong => "Strong",
        }
    }

    pub fn settings(self) -> RasterSettings {
        match self {
            RasterLevel::Light => RasterSettings { dpi: 150.0, quality: 80 },
            RasterLevel::Medium => RasterSettings { dpi: 110.0, quality: 65 },
            RasterLevel::Strong => RasterSettings { dpi: 80.0, quality: 50 },
        }
    }
}

/// The resolution pages are rendered at and the JPEG quality they are stored with.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RasterSettings {
    /// Dots per inch, 36–600 (clamped).
    pub dpi: f64,
    /// JPEG quality, 1–100 (clamped).
    pub quality: u8,
}

impl RasterSettings {
    fn clamped(self) -> Self {
        let dpi = if self.dpi.is_finite() { self.dpi.clamp(36.0, 600.0) } else { 110.0 };
        Self { dpi, quality: self.quality.clamp(1, 100) }
    }
}

/// What rasterizing a document needs, independent of the session.
pub struct RasterizeJob {
    src: ExportSource,
    title: String,
    settings: RasterSettings,
    opts: SaveOptions,
}

impl Session {
    /// Capture what rasterizing document `id` needs. Refused when the document's security
    /// doesn't allow copying its content (the copy is a picture of every page).
    pub fn rasterize_job(&self, id: DocId, settings: RasterSettings) -> Result<RasterizeJob, EditError> {
        let doc = self.get(id).ok_or(EditError::NoDocument)?;
        if !doc.allows_copying() {
            return Err(EditError::NotPermitted("copying its content"));
        }
        let title = doc.name.rsplit_once('.').map_or(doc.name.as_str(), |(stem, _)| stem).to_string();
        Ok(RasterizeJob {
            src: doc.export_source(),
            title,
            settings: settings.clamped(),
            opts: SaveOptions { mod_date: self.now().map(pdfcraft_cos::pdf_date), ..SaveOptions::default() },
        })
    }

    /// Rasterize document `id` in one go (automation): the new file's bytes.
    pub fn rasterized_bytes(&self, id: DocId, settings: RasterSettings) -> Result<Arc<Vec<u8>>, EditError> {
        self.rasterize_job(id, settings)?.run(|_| true).map(|(bytes, _)| bytes)
    }
}

/// Encode a rendered page as a JPEG over white paper: greyscale when every pixel is grey (most
/// scans and text pages), which is a third of the data.
pub fn encode_page(width: u32, height: u32, premultiplied: &[u8], quality: u8) -> Result<Vec<u8>, String> {
    use image::ImageEncoder;
    let rgb = over_white(premultiplied);
    let grey = rgb.as_chunks::<3>().0.iter().all(|[r, g, b]| r.abs_diff(*g) <= 3 && g.abs_diff(*b) <= 3 && r.abs_diff(*b) <= 3);
    let mut out = Vec::new();
    let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality.clamp(1, 100));
    if grey {
        let luma: Vec<u8> = rgb.as_chunks::<3>().0.iter().map(|[_, g, _]| *g).collect();
        encoder.write_image(&luma, width, height, image::ExtendedColorType::L8)
    } else {
        encoder.write_image(&rgb, width, height, image::ExtendedColorType::Rgb8)
    }
    .map_err(|e| e.to_string())?;
    Ok(out)
}

impl RasterizeJob {
    /// Render, encode and rebuild. `progress` is called before each page; returning `false`
    /// stops with [`EditError::Cancelled`]. A panic is returned as an error, so a worker thread
    /// always reports back.
    pub fn run(self, mut progress: impl FnMut(OptimizeStage) -> bool) -> Result<Optimized, EditError> {
        guard(move || self.work(&mut progress)).map_err(EditError::Optimize)?
    }

    fn work(self, progress: &mut dyn FnMut(OptimizeStage) -> bool) -> Result<Optimized, EditError> {
        let RasterizeJob { src, title, settings, opts } = self;
        let total = src.pages;
        if total == 0 {
            return Err(EditError::Invalid("the document has no pages".into()));
        }
        let sizes = src.sizes.clone();
        let mut exporter = Exporter::from_source(src);
        let mut pages = Vec::with_capacity(total.min(10_000));
        let mut held = 0usize;
        for page in 0..total {
            if !progress(OptimizeStage::Rasterizing { done: page, total }) {
                return Err(EditError::Cancelled);
            }
            let r = exporter.render(page, settings.dpi).map_err(EditError::Optimize)?;
            let jpeg = encode_page(r.width, r.height, &r.rgba, settings.quality).map_err(EditError::Optimize)?;
            held = held.saturating_add(jpeg.len());
            if held > MAX_OUTPUT {
                return Err(EditError::Optimize("the rasterized copy would be too large; choose a lower resolution".into()));
            }
            let (w, h) = sizes.get(page).copied().unwrap_or((612.0, 792.0));
            pages.push((jpeg, f64::from(w), f64::from(h)));
        }
        if !progress(OptimizeStage::Writing) {
            return Err(EditError::Cancelled);
        }
        let doc = pdfcraft_create::from_page_images(&pages, &title)?;
        let bytes = write_full(&doc, &opts).map_err(|e| EditError::Write(e.to_string()))?;
        Ok((Arc::new(bytes), OptimizeReport::default()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_get_smaller_and_round_trip_their_ids() {
        let s: Vec<RasterSettings> = RasterLevel::ALL.iter().map(|l| l.settings()).collect();
        assert!(s.windows(2).all(|w| w[0].dpi > w[1].dpi && w[0].quality > w[1].quality));
        for l in RasterLevel::ALL {
            assert_eq!(RasterLevel::from_id(l.id()), Some(l));
        }
        assert_eq!(RasterLevel::from_id("max"), None);
    }

    #[test]
    fn hostile_settings_are_clamped() {
        let s = RasterSettings { dpi: f64::NAN, quality: 0 }.clamped();
        assert_eq!((s.dpi, s.quality), (110.0, 1));
        let s = RasterSettings { dpi: 1e9, quality: 255 }.clamped();
        assert_eq!((s.dpi, s.quality), (600.0, 100));
    }

    #[test]
    fn grey_pages_are_encoded_as_greyscale() {
        let grey = [200u8, 200, 200, 255].repeat(16 * 16);
        let colour = [200u8, 20, 20, 255].repeat(16 * 16);
        let g = encode_page(16, 16, &grey, 60).unwrap();
        let c = encode_page(16, 16, &colour, 60).unwrap();
        let comps = |j: &[u8]| image::load_from_memory(j).unwrap().color().channel_count();
        assert_eq!(comps(&g), 1);
        assert_eq!(comps(&c), 3);
    }
}
