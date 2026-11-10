//! Compare files: the text differences between two open documents, a report, and the
//! differences marked as comments in the newer one.

use std::collections::HashMap;
use std::sync::Arc;

use pdfcraft_cos::ObjRef;

pub use pdfcraft_compare::{Change, Comparison, Kind, Side};
// How far Office export reads into form XObjects; `OfficeExport::truncated_pages` lists the pages
// that draw more.
pub use pdfcraft_edit::{MAX_FORM_DEPTH, MAX_FORM_VISITS};

use crate::{DocId, Edit, EditError, Markup, NewAnnotation, NoteIcon, Session, Shape};

/// Most bytes of encoded pictures one Office export prepares, counting each image object once
/// (the Word and PowerPoint writers keep at most 512 MiB of them as well).
pub const MAX_EXPORT_IMAGE_BYTES: usize = 512 << 20;

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
    /// Compare files: the text differences from document `old` to document `new`. The result
    /// carries both documents' text, so both must allow copying it.
    pub fn compare(&self, old: DocId, new: DocId) -> Result<Comparison, EditError> {
        let a = self.get(old).ok_or(EditError::NoDocument)?;
        let b = self.get(new).ok_or(EditError::NoDocument)?;
        a.check_copying()?;
        b.check_copying()?;
        Ok(pdfcraft_compare::compare(&a.words(), &b.words()))
    }

    /// [`Session::compare`], keeping the changes on `pages` of `new` (0-based; empty: every page).
    pub fn compare_pages(&self, old: DocId, new: DocId, pages: &[usize]) -> Result<Comparison, EditError> {
        let mut c = self.compare(old, new)?;
        if !pages.is_empty() {
            c.changes.retain(|ch| pages.iter().any(|page| (ch.new.page..=ch.new.last_page).contains(page)));
        }
        Ok(c)
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
        self.compare_report_of(old, new, &[])
    }

    /// [`Session::compare_report`] of the changes on `pages` ([`Session::compare_pages`]).
    pub fn compare_report_of(&self, old: DocId, new: DocId, pages: &[usize]) -> Result<Arc<Vec<u8>>, EditError> {
        let c = self.compare_pages(old, new, pages)?;
        let name = |id| self.get(id).map(|d| d.name.clone()).unwrap_or_default();
        self.create_report("Compare Report", &pdfcraft_compare::report(&c, &name(old), &name(new)))
    }

    /// Mark the differences in `new` as comments: highlights over replaced and inserted text
    /// (blue, green) and a note where text was deleted (red), authored "Compare". One undoable
    /// step; returns how many comments were added.
    pub fn mark_differences(&mut self, old: DocId, new: DocId) -> Result<usize, EditError> {
        self.mark_differences_of(old, new, &[])
    }

    /// [`Session::mark_differences`] of the changes on `pages` ([`Session::compare_pages`]).
    pub fn mark_differences_of(&mut self, old: DocId, new: DocId, pages: &[usize]) -> Result<usize, EditError> {
        let c = self.compare_pages(old, new, pages)?;
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
    /// CSS-free HTML that preserves text and inline images but drops colours and layout styling.
    HtmlSimple,
    Rtf,
    Markdown,
}

impl OfficeFormat {
    pub fn extension(self) -> &'static str {
        match self {
            OfficeFormat::Docx => "docx",
            OfficeFormat::Pptx => "pptx",
            OfficeFormat::Html => "html",
            OfficeFormat::HtmlSimple => "html",
            OfficeFormat::Rtf => "rtf",
            OfficeFormat::Markdown => "md",
        }
    }

    pub fn from_extension(ext: &str) -> Option<OfficeFormat> {
        match ext.to_ascii_lowercase().as_str() {
            "docx" => Some(OfficeFormat::Docx),
            "pptx" => Some(OfficeFormat::Pptx),
            "html" | "htm" => Some(OfficeFormat::Html),
            "rtf" => Some(OfficeFormat::Rtf),
            "md" | "markdown" => Some(OfficeFormat::Markdown),
            _ => None,
        }
    }
}

/// A Word, PowerPoint, HTML, RTF or Markdown export ([`crate::Document::export_office_checked`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OfficeExport {
    /// The exported file.
    pub bytes: Vec<u8>,
    /// Pages (0-based) that draw forms nested deeper than [`MAX_FORM_DEPTH`] or more of them
    /// than [`MAX_FORM_VISITS`], or pictures past [`MAX_EXPORT_IMAGE_BYTES`] or past what one export
    /// may decode: export stops reading there to stay bounded on hostile files, so the text and images of those forms, or
    /// those pictures, are missing from `bytes`.
    pub truncated_pages: Vec<usize>,
}

/// An image object prepared for Office export: its file extension and encoded bytes, or why it is
/// left out (`Err(true)`: past [`MAX_EXPORT_IMAGE_BYTES`], or what the export may decode;
/// `Err(false)`: it can't be encoded).
type PreparedImage = Result<(&'static str, Arc<[u8]>), bool>;

/// Word's layout mode draws the page behind the text at this resolution.
const BACKGROUND_DPI: f64 = 150.0;
/// And stores the picture with this JPEG quality.
const BACKGROUND_QUALITY: u8 = 85;

/// Maps PDF user space onto the visible page (points, y up) for Word's layout mode.
struct View<'a> {
    info: &'a pdfcraft_render::PageInfo,
    /// Points per user-space unit on the displayed page (`/UserUnit`, crop scaling).
    unit: f64,
}

impl<'a> View<'a> {
    fn new(info: &'a pdfcraft_render::PageInfo) -> Self {
        Self { info, unit: display_unit(info) }
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
        let (bold, italic) = weight_and_slant(&b.base_font);
        pdfcraft_export::Block {
            space_around: [b.editor_text.starts_with(char::is_whitespace), b.editor_text.ends_with(char::is_whitespace)],
            text: b.text.clone(),
            rect: self.rect(b.rect),
            size: b.size * self.unit,
            bold: bold || b.bold,
            italic: italic || b.italic,
            color: b.color,
            rotation: b.rotation - f64::from(self.info.rotation),
        }
    }

    fn run(&self, l: &pdfcraft_edit::TextLine) -> pdfcraft_export::LayoutRun {
        let (bold, italic) = weight_and_slant(&l.base_font);
        pdfcraft_export::LayoutRun {
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

/// Whether a rendered page is (nearly) plain white paper: fewer than one pixel in 2,000 differs.
fn blank(premultiplied: &[u8]) -> bool {
    let rgb = crate::export::over_white(premultiplied);
    let px = rgb.as_chunks::<3>().0;
    let inked = px.iter().filter(|[r, g, b]| *r < 245 || *g < 245 || *b < 245).count();
    inked.saturating_mul(2000) < px.len()
}

/// Displayed points per user-space unit on `info`'s page (`/UserUnit`), 1 when the page has no
/// usable size.
pub(crate) fn display_unit(info: &pdfcraft_render::PageInfo) -> f64 {
    let crop_width = f64::from(info.crop[2] - info.crop[0]);
    let display_width = f64::from(if info.rotation.is_multiple_of(180) { info.width } else { info.height });
    if crop_width > 0.0 && crop_width.is_finite() && display_width.is_finite() { display_width / crop_width } else { 1.0 }
}

/// Bold and italic as a font's name says them (`Arial-BoldItalicMT`).
pub(crate) fn weight_and_slant(base_font: &str) -> (bool, bool) {
    let f = base_font.to_ascii_lowercase();
    (f.contains("bold") || f.contains("black") || f.contains("heavy"), f.contains("italic") || f.contains("oblique"))
}

/// NadsPDF Mise en page: a page's size and paragraphs, as [`crate::Document::layout_sources`]
/// reads them for the text frames that replace them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LayoutPage {
    /// Points.
    pub width: f64,
    pub height: f64,
    pub blocks: Vec<LayoutBlock>,
}

/// A paragraph of a page: its words, where and how the PDF drew them, and the font's name.
#[derive(Clone, Debug, PartialEq)]
pub struct LayoutBlock {
    pub text: String,
    /// [x0, y0, x1, y1] on the displayed page in points, y up.
    pub rect: [f64; 4],
    pub size: f64,
    pub bold: bool,
    pub italic: bool,
    /// sRGB, 0..=1.
    pub color: [f64; 3],
    /// Counterclockwise baseline angle (degrees).
    pub rotation: f64,
    /// The PDF font's name (`/BaseFont`, a subset's `ABCDEF+` prefix included).
    pub font: String,
}

impl crate::Document {
    /// The pages as paragraphs and images (for Word, HTML and RTF export), including what
    /// form XObjects draw.
    pub fn export_pages(&self) -> Vec<pdfcraft_export::Page> {
        self.export_pages_checked(true, MAX_EXPORT_IMAGE_BYTES).0
    }

    /// [`Self::export_pages`], and the pages whose forms were left partly unread or whose
    /// pictures went past `image_budget` bytes ([`OfficeExport::truncated_pages`]). Without
    /// `images` no picture is prepared.
    pub(crate) fn export_pages_checked(&self, images: bool, image_budget: usize) -> (Vec<pdfcraft_export::Page>, Vec<usize>) {
        let Some(editor) = self.editor.as_ref() else { return (Vec::new(), Vec::new()) };
        self.export_pages_within(images, image_budget, &mut pdfcraft_cos::ImageBudget::new(&editor.cos))
    }

    /// [`Self::export_pages_checked`], its pictures decoded within `image_work`, which is shared
    /// by the whole export: a picture past it is left out like one past `image_budget`.
    pub(crate) fn export_pages_within(
        &self,
        images: bool,
        image_budget: usize,
        image_work: &mut pdfcraft_cos::ImageBudget,
    ) -> (Vec<pdfcraft_export::Page>, Vec<usize>) {
        let Some(cos) = self.editor.as_ref().map(|e| &e.cos) else { return (Vec::new(), Vec::new()) };
        let mut truncated = Vec::new();
        // Each image object is encoded once and shared by every place that draws it.
        let mut encoded: HashMap<ObjRef, PreparedImage> = HashMap::new();
        let mut image_bytes = image_budget;
        let pages = self
            .info
            .pages
            .iter()
            .enumerate()
            .map(|(i, info)| {
                // Export uses the visible page in points, y up. Raw PDF coordinates include
                // CropBox offsets and precede /Rotate and /UserUnit.
                let visible_rect = |r: [f64; 4]| {
                    let a = info.user_to_view(r[0] as f32, r[1] as f32);
                    let b = info.user_to_view(r[2] as f32, r[3] as f32);
                    [a[0].min(b[0]) as f64, (info.height - a[1].max(b[1])) as f64, a[0].max(b[0]) as f64, (info.height - a[1].min(b[1])) as f64]
                };
                let unit = display_unit(info);
                let reading = pdfcraft_edit::reading_blocks_checked(cos, i).unwrap_or_default();
                let forms_unread = reading.truncated;
                let blocks = reading
                    .blocks
                    .into_iter()
                    .filter(|b| !b.text.trim().is_empty())
                    .map(|b| {
                        let (bold, italic) = weight_and_slant(&b.base_font);
                        // `text` is trimmed; the editor text keeps the source's own spaces.
                        let space_around = [b.editor_text.starts_with(char::is_whitespace), b.editor_text.ends_with(char::is_whitespace)];
                        pdfcraft_export::Block {
                            space_around,
                            text: b.text,
                            rect: visible_rect(b.rect),
                            size: b.size * unit,
                            bold,
                            italic,
                            color: b.color,
                            rotation: b.rotation - f64::from(info.rotation),
                        }
                    })
                    .collect();
                let placed = if images { pdfcraft_edit::reading_images(cos, i).unwrap_or_default() } else { Vec::new() };
                let mut over_budget = false;
                let images = placed
                    .iter()
                    .filter_map(|im| {
                        let object = im.object?;
                        let prepared = encoded.entry(object).or_insert_with(|| {
                            let refused = image_work.refused();
                            let (ext, bytes) =
                                pdfcraft_create::image_file_within(cos, object, &mut *image_work).map_err(|_| image_work.refused() > refused)?;
                            image_bytes = image_bytes.checked_sub(bytes.len()).ok_or(true)?;
                            Ok((if ext == "jpg" { "jpg" } else { "png" }, Arc::from(bytes)))
                        });
                        over_budget |= matches!(prepared, Err(true));
                        let (ext, bytes) = prepared.clone().ok()?;
                        Some(pdfcraft_export::Image {
                            ext,
                            bytes,
                            rect: visible_rect(im.rect),
                            size: [im.matrix[0].hypot(im.matrix[1]) * unit, im.matrix[2].hypot(im.matrix[3]) * unit],
                            rotation: im.matrix[1].atan2(im.matrix[0]).to_degrees() - f64::from(info.rotation),
                        })
                    })
                    .collect();
                if forms_unread || over_budget {
                    truncated.push(i);
                }
                // The page colour (Edit a PDF ▸ Background, a producer's page fill) for formats that keep one.
                let background = pdfcraft_edit::page_background(cos, i).ok().flatten();
                // The ruled lines tell a drawn table from text that merely lines up (#5149).
                let rules = pdfcraft_edit::page_rules(cos, i).unwrap_or_default().into_iter().map(visible_rect).collect();
                pdfcraft_export::Page { width: info.width as f64, height: info.height as f64, blocks, images, background, layout: None, rules }
            })
            .collect();
        (pages, truncated)
    }

    /// NadsPDF Mise en page: what turns this PDF into an editable page layout. The document with
    /// every text-showing operator removed (written unencrypted: the pages' artwork, shapes and
    /// pictures, drawn behind), and its pages as paragraphs (the words, to become text frames).
    /// `None` when the document's security doesn't allow copying its content, or it can't be
    /// written.
    pub fn layout_sources(&self) -> Option<(Vec<u8>, Vec<LayoutPage>)> {
        if !self.allows_copying() {
            return None;
        }
        let cos = self.editor.as_ref().map(|e| &e.cos)?;
        let bytes = pdfcraft_cos::write_full(&pdfcraft_edit::without_text(cos), &pdfcraft_cos::SaveOptions::default()).ok()?;
        let pages = self
            .info
            .pages
            .iter()
            .enumerate()
            .map(|(i, info)| {
                // As Word export reads them (`export_pages_within`): the visible page in points,
                // y up, after CropBox offsets, /Rotate and /UserUnit.
                let visible_rect = |r: [f64; 4]| {
                    let a = info.user_to_view(r[0] as f32, r[1] as f32);
                    let b = info.user_to_view(r[2] as f32, r[3] as f32);
                    [a[0].min(b[0]) as f64, (info.height - a[1].max(b[1])) as f64, a[0].max(b[0]) as f64, (info.height - a[1].min(b[1])) as f64]
                };
                let unit = display_unit(info);
                let reading = pdfcraft_edit::reading_blocks_checked(cos, i).unwrap_or_default();
                let blocks = reading
                    .blocks
                    .into_iter()
                    .filter(|b| !b.text.trim().is_empty())
                    .map(|b| {
                        let (bold, italic) = weight_and_slant(&b.base_font);
                        LayoutBlock {
                            text: b.text,
                            rect: visible_rect(b.rect),
                            size: b.size * unit,
                            bold: bold || b.bold,
                            italic: italic || b.italic,
                            color: b.color,
                            rotation: b.rotation - f64::from(info.rotation),
                            font: b.base_font,
                        }
                    })
                    .collect();
                LayoutPage { width: f64::from(info.width), height: f64::from(info.height), blocks }
            })
            .collect();
        Some((bytes, pages))
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
            let background = pdfcraft_export::Image { ext: "jpg", bytes: Arc::from(jpeg), rect: [0.0, 0.0, w, h], size: [w, h], rotation: 0.0 };
            page.layout = Some(pdfcraft_export::Layout { background, paras });
        }
    }

    /// The document as a Word, PowerPoint, HTML or RTF file.
    pub fn export_office(&self, format: OfficeFormat) -> Vec<u8> {
        self.export_office_checked(format).bytes
    }

    /// [`Self::export_office`], and the pages whose text it had to leave out
    /// ([`OfficeExport::truncated_pages`]).
    pub fn export_office_checked(&self, format: OfficeFormat) -> OfficeExport {
        // RTF and Markdown leave pictures out.
        let (mut pages, truncated_pages) =
            self.export_pages_checked(!matches!(format, OfficeFormat::Rtf | OfficeFormat::Markdown), MAX_EXPORT_IMAGE_BYTES);
        let title = self.info.title.clone().unwrap_or_else(|| self.name.trim_end_matches(".pdf").to_string());
        // Word and HTML name the document's language (`/Lang`), so spelling is checked in it.
        let language = self.initial_view().language;
        let bytes = match format {
            OfficeFormat::Docx => {
                self.add_word_layout(&mut pages);
                pdfcraft_export::docx_with_language(&pages, &title, language.as_deref())
            }
            OfficeFormat::Pptx => pdfcraft_export::pptx(&pages, &title),
            OfficeFormat::Html => pdfcraft_export::html_with_language(&pages, &title, language.as_deref()).into_bytes(),
            OfficeFormat::HtmlSimple => pdfcraft_export::html_simple_with_language(&pages, &title, language.as_deref()).into_bytes(),
            OfficeFormat::Rtf => pdfcraft_export::rtf(&pages).into_bytes(),
            OfficeFormat::Markdown => pdfcraft_export::markdown(&pages).into_bytes(),
        };
        OfficeExport { bytes, truncated_pages }
    }
}
