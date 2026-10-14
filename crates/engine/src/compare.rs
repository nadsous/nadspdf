//! Compare files: the text differences between two open documents, a report, and the
//! differences marked as comments in the newer one.

use std::sync::Arc;

pub use pdfcraft_compare::{Change, Comparison, Kind, Side};

use crate::{DocId, Edit, EditError, Markup, NewAnnotation, NoteIcon, Session, Shape};

/// Acrobat's compare colours: replaced blue, inserted green, deleted red.
pub fn colour(kind: Kind) -> crate::Rgb {
    match kind {
        Kind::Replaced => [0.2, 0.45, 0.95],
        Kind::Inserted => [0.2, 0.75, 0.3],
        Kind::Deleted => [0.9, 0.25, 0.25],
    }
}

impl crate::Document {
    /// Standards ▸ Verify PDF/A: the rules the document breaks for `level`.
    pub fn pdfa_verify(&self, level: pdfcraft_preflight::Level) -> Vec<pdfcraft_preflight::Issue> {
        self.editor.as_ref().map(|e| pdfcraft_preflight::verify(&e.cos, level)).unwrap_or_default()
    }

    /// The standards the document declares (Standards panel).
    pub fn standards(&self) -> pdfcraft_preflight::Declared {
        self.editor.as_ref().map(|e| pdfcraft_preflight::declared(&e.cos)).unwrap_or_default()
    }

    /// Every word of the document in reading order, with page and box.
    pub fn words(&self) -> Vec<pdfcraft_compare::Word> {
        let config = pdfcraft_render::RenderConfig { password: self.password.as_deref().map(Arc::from), ..Default::default() };
        let mut r = pdfcraft_render::PageRenderer::new(self.bytes.clone(), config);
        let mut out = Vec::new();
        for (page, info) in self.info.pages.iter().enumerate() {
            let res = r.render(pdfcraft_render::RenderRequest { page, kind: pdfcraft_render::RequestKind::Text, scale: 1.0, ..Default::default() });
            if let Some(t) = res.text {
                out.extend(crate::js::page_words(&t, info).into_iter().map(|(text, rect)| pdfcraft_compare::Word { text, page, rect }));
            }
        }
        out
    }
}

impl Session {
    /// Compare files: the text differences from document `old` to document `new`.
    pub fn compare(&self, old: DocId, new: DocId) -> Result<Comparison, EditError> {
        let a = self.get(old).ok_or(EditError::NoDocument)?;
        let b = self.get(new).ok_or(EditError::NoDocument)?;
        Ok(pdfcraft_compare::compare(&a.words(), &b.words()))
    }

    /// Visual compare: regions where page n of `new` looks different from page n of `old`
    /// (rendered at `dpi`), as (page, user-space box in `new`).
    pub fn compare_visual(&self, old: DocId, new: DocId, dpi: f32) -> Result<Vec<(usize, [f64; 4])>, EditError> {
        let a = self.get(old).ok_or(EditError::NoDocument)?;
        let b = self.get(new).ok_or(EditError::NoDocument)?;
        let renderer = |d: &crate::Document| {
            pdfcraft_render::PageRenderer::new(
                d.bytes.clone(),
                pdfcraft_render::RenderConfig { password: d.password.as_deref().map(Arc::from), ..Default::default() },
            )
        };
        let (mut ra, mut rb) = (renderer(a), renderer(b));
        let scale = dpi.clamp(18.0, 150.0) / 72.0;
        let mut out = Vec::new();
        for (page, info) in b.info.pages.iter().enumerate().take(a.info.pages.len()) {
            let req = pdfcraft_render::RenderRequest { page, scale, ..Default::default() };
            let (x, y) = (ra.render(req), rb.render(req));
            if x.error.is_some() || y.error.is_some() {
                continue;
            }
            let s = y.width as f32 / info.width.max(1e-3);
            for r in pdfcraft_compare::visual_regions((&y.rgba, y.width, y.height), (&x.rgba, x.width, x.height), 24) {
                let p = info.view_to_user(r[0] as f32 / s, r[1] as f32 / s);
                let q = info.view_to_user(r[2] as f32 / s, r[3] as f32 / s);
                out.push((page, [p[0].min(q[0]) as f64, p[1].min(q[1]) as f64, p[0].max(q[0]) as f64, p[1].max(q[1]) as f64]));
            }
        }
        Ok(out)
    }

    /// The compare report as a new PDF (not opened).
    pub fn compare_report(&self, old: DocId, new: DocId) -> Result<Arc<Vec<u8>>, EditError> {
        let c = self.compare(old, new)?;
        let name = |id| self.get(id).map(|d| d.name.clone()).unwrap_or_default();
        self.create_report("Compare Report", &pdfcraft_compare::report(&c, &name(old), &name(new)))
    }

    /// Mark the differences in `new` as comments: highlights over replaced and inserted text
    /// (blue, green) and a note where text was deleted (red), authored "Compare". One undoable
    /// step; returns how many comments were added.
    pub fn mark_differences(&mut self, old: DocId, new: DocId) -> Result<usize, EditError> {
        let c = self.compare(old, new)?;
        let mut edits = Vec::new();
        for ch in &c.changes {
            let color = colour(ch.kind);
            let contents = match ch.kind {
                Kind::Replaced => format!("Replaced: \"{}\" with \"{}\"", ch.old.text, ch.new.text),
                Kind::Inserted => format!("Inserted: \"{}\"", ch.new.text),
                Kind::Deleted => format!("Deleted: \"{}\"", ch.old.text),
            };
            let shape = if ch.new.rects.is_empty() {
                let Some(r) = ch.new.near else { continue };
                Shape::Note { at: [r[0], r[3]], icon: NoteIcon::Note }
            } else {
                Shape::TextMarkup {
                    kind: Markup::Highlight,
                    quads: ch.new.rects.iter().map(|r| [r[0], r[3], r[2], r[3], r[0], r[1], r[2], r[1]]).collect(),
                }
            };
            let mut style = crate::Style::default_for(&shape);
            style.color = color;
            edits.push(Edit::AddAnnotation(NewAnnotation { page: ch.new.page, shape, style, contents, author: "Compare".into() }));
        }
        let n = edits.len();
        if n > 0 {
            self.apply(new, Edit::Batch { label: "Mark differences".into(), edits })?;
        }
        Ok(n)
    }
}

/// Export a PDF ▸ Word, PowerPoint, HTML or RTF.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OfficeFormat {
    Docx,
    Pptx,
    Html,
    Rtf,
}

impl OfficeFormat {
    pub fn extension(self) -> &'static str {
        match self {
            OfficeFormat::Docx => "docx",
            OfficeFormat::Pptx => "pptx",
            OfficeFormat::Html => "html",
            OfficeFormat::Rtf => "rtf",
        }
    }

    pub fn from_extension(ext: &str) -> Option<OfficeFormat> {
        match ext.to_ascii_lowercase().as_str() {
            "docx" => Some(OfficeFormat::Docx),
            "pptx" => Some(OfficeFormat::Pptx),
            "html" | "htm" => Some(OfficeFormat::Html),
            "rtf" => Some(OfficeFormat::Rtf),
            _ => None,
        }
    }
}

/// Word's layout mode draws the page behind the text at this resolution.
const BACKGROUND_DPI: f64 = 150.0;
/// And stores the picture with this JPEG quality.
const BACKGROUND_QUALITY: u8 = 85;

/// Maps PDF user space onto the visible page (points, y up) for export.
struct View<'a> {
    info: &'a pdfcraft_render::PageInfo,
    /// Points per user-space unit on the displayed page (`/UserUnit`, crop scaling).
    unit: f64,
}

impl<'a> View<'a> {
    fn new(info: &'a pdfcraft_render::PageInfo) -> Self {
        let crop_width = f64::from(info.crop[2] - info.crop[0]);
        let display_width = f64::from(if info.rotation.is_multiple_of(180) { info.width } else { info.height });
        let unit = if crop_width > 0.0 && crop_width.is_finite() && display_width.is_finite() { display_width / crop_width } else { 1.0 };
        Self { info, unit }
    }

    /// Export uses the visible page in points, y up. Raw PDF coordinates include CropBox offsets
    /// and precede /Rotate and /UserUnit.
    fn rect(&self, r: [f64; 4]) -> [f64; 4] {
        let a = self.info.user_to_view(r[0] as f32, r[1] as f32);
        let b = self.info.user_to_view(r[2] as f32, r[3] as f32);
        let h = self.info.height;
        [a[0].min(b[0]) as f64, (h - a[1].max(b[1])) as f64, a[0].max(b[0]) as f64, (h - a[1].min(b[1])) as f64]
    }

    fn block(&self, b: &pdfcraft_edit::TextBlock) -> pdfcraft_export::Block {
        let (bold, italic) = style_of(&b.base_font);
        pdfcraft_export::Block {
            text: b.text.clone(),
            rect: self.rect(b.rect),
            size: b.size * self.unit,
            bold: bold || b.bold,
            italic: italic || b.italic,
            color: b.color,
            rotation: b.rotation - f64::from(self.info.rotation),
        }
    }

    fn run(&self, l: &pdfcraft_edit::TextLine) -> pdfcraft_export::Run {
        let (bold, italic) = style_of(&l.base_font);
        pdfcraft_export::Run {
            text: l.text.clone(),
            rect: self.rect(l.rect),
            size: l.size * self.unit,
            bold: bold || l.bold,
            italic: italic || l.italic,
            color: l.color,
            font: l.base_font.clone(),
        }
    }
}

/// Bold and italic, as a font's name says them.
fn style_of(base_font: &str) -> (bool, bool) {
    let f = base_font.to_ascii_lowercase();
    (f.contains("bold") || f.contains("black") || f.contains("heavy"), f.contains("italic") || f.contains("oblique"))
}

/// Whether a rendered page is (nearly) plain white paper: fewer than one pixel in 2,000 differs.
fn blank(premultiplied: &[u8]) -> bool {
    let rgb = crate::export::over_white(premultiplied);
    let px = rgb.as_chunks::<3>().0;
    let inked = px.iter().filter(|[r, g, b]| *r < 245 || *g < 245 || *b < 245).count();
    inked.saturating_mul(2000) < px.len()
}

impl crate::Document {
    /// The pages as paragraphs and images (for Word, HTML and RTF export), including what
    /// form XObjects draw.
    pub fn export_pages(&self) -> Vec<pdfcraft_export::Page> {
        let Some(cos) = self.editor.as_ref().map(|e| &e.cos) else { return Vec::new() };
        self.info
            .pages
            .iter()
            .enumerate()
            .map(|(i, info)| {
                let view = View::new(info);
                let blocks = pdfcraft_edit::reading_blocks(cos, i)
                    .unwrap_or_default()
                    .iter()
                    .filter(|b| !b.text.trim().is_empty())
                    .map(|b| view.block(b))
                    .collect();
                let images = pdfcraft_edit::reading_images(cos, i)
                    .unwrap_or_default()
                    .iter()
                    .filter_map(|im| {
                        let (ext, bytes) = pdfcraft_create::image_file(cos, im.object?).ok()?;
                        Some(pdfcraft_export::Image {
                            ext: if ext == "jpg" { "jpg" } else { "png" },
                            bytes,
                            rect: view.rect(im.rect),
                            size: [im.matrix[0].hypot(im.matrix[1]) * view.unit, im.matrix[2].hypot(im.matrix[3]) * view.unit],
                            rotation: im.matrix[1].atan2(im.matrix[0]).to_degrees() - f64::from(info.rotation),
                        })
                    })
                    .collect();
                pdfcraft_export::Page { width: info.width as f64, height: info.height as f64, blocks, images, layout: None }
            })
            .collect()
    }

    /// Word's layout mode for `pages`: each page whose artwork shows gets a picture of itself
    /// without text, and its paragraphs row by row. Plain text pages (nothing but words on white)
    /// and OCR layers over scans keep flowing.
    pub fn add_word_layout(&self, pages: &mut [pdfcraft_export::Page]) {
        let Some(cos) = self.editor.as_ref().map(|e| &e.cos) else { return };
        let Ok(bytes) = pdfcraft_cos::write_full(&pdfcraft_edit::without_text(cos), &pdfcraft_cos::SaveOptions::default()) else { return };
        // The copy is written unencrypted; reading colours are a display setting, not the page.
        let config = pdfcraft_render::RenderConfig { password: None, reading_colors: Default::default(), ..self.config.clone() };
        let src = crate::export::ExportSource {
            bytes: Arc::new(bytes),
            config,
            pages: self.info.pages.len(),
            sizes: self.info.pages.iter().map(|p| (p.width, p.height)).collect(),
        };
        let mut exporter = crate::export::Exporter::from_source(src);
        for (i, (page, info)) in pages.iter_mut().zip(&self.info.pages).enumerate() {
            let Ok((blocks, lines)) = pdfcraft_edit::reading_layout(cos, i) else { continue };
            let blocks: Vec<&pdfcraft_edit::TextBlock> = blocks.iter().filter(|b| !b.text.trim().is_empty()).collect();
            if blocks.is_empty() || blocks.iter().all(|b| b.invisible) {
                continue;
            }
            let Ok(r) = exporter.render(i, BACKGROUND_DPI) else { continue };
            if blank(&r.rgba) {
                continue;
            }
            let Ok(jpeg) = crate::rasterize::encode_page(r.width, r.height, &r.rgba, BACKGROUND_QUALITY) else { continue };
            let view = View::new(info);
            let (w, h) = (f64::from(info.width), f64::from(info.height));
            let paras = blocks
                .iter()
                // Invisible words (an OCR layer) are already in the picture.
                .filter(|b| !b.invisible)
                .map(|b| pdfcraft_export::Para {
                    block: view.block(b),
                    rows: b
                        .rows
                        .iter()
                        .map(|row| row.iter().filter_map(|li| lines.get(*li)).filter(|l| !l.text.is_empty()).map(|l| view.run(l)).collect())
                        .collect(),
                })
                .collect();
            let background = pdfcraft_export::Image { ext: "jpg", bytes: jpeg, rect: [0.0, 0.0, w, h], size: [w, h], rotation: 0.0 };
            page.layout = Some(pdfcraft_export::Layout { background, paras });
        }
    }

    /// The document as a Word, PowerPoint, HTML or RTF file.
    pub fn export_office(&self, format: OfficeFormat) -> Vec<u8> {
        let mut pages = self.export_pages();
        let title = self.info.title.clone().unwrap_or_else(|| self.name.trim_end_matches(".pdf").to_string());
        match format {
            OfficeFormat::Docx => {
                self.add_word_layout(&mut pages);
                pdfcraft_export::docx(&pages, &title)
            }
            OfficeFormat::Pptx => pdfcraft_export::pptx(&pages, &title),
            OfficeFormat::Html => pdfcraft_export::html(&pages, &title).into_bytes(),
            OfficeFormat::Rtf => pdfcraft_export::rtf(&pages).into_bytes(),
        }
    }
}
