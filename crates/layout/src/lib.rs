//! NadsPDF Mise en page: pages, text and image frames, laid out and exported to PDF.
//!
//! A small API over the page layout engine in `vendor/designcraft` (Apache-2.0 OR MIT): the
//! interface works with pages, frames and text and never with that engine's own types. Positions
//! are in points from the page's top-left corner. Every change is one undoable step.
//!
//! Errors are values ([`LayoutError`]); nothing here panics on any input, and every number from
//! the interface is checked before it reaches the engine.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

use designcraft_engine::Session;
use designcraft_engine::doc::Document;
use serde_json::{Value, json};

/// What went wrong, in words the interface can show.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LayoutError {
    /// The engine refused or failed (its own message).
    #[error("{0}")]
    Engine(String),
    /// A page, frame or value that isn't there or can't be used.
    #[error("{0}")]
    Invalid(String),
}

pub type Result<T> = std::result::Result<T, LayoutError>;

fn engine(e: impl std::fmt::Display) -> LayoutError {
    LayoutError::Engine(e.to_string())
}

/// Paper for a new document.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Paper {
    A4,
    Letter,
}

impl Paper {
    fn preset(self) -> &'static str {
        match self {
            Paper::A4 => "A4",
            Paper::Letter => "Letter",
        }
    }
}

/// A rectangle in points, `x0 < x1` and `y0 < y1` once [`Rect::normalized`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
}

impl Rect {
    pub fn new(x0: f64, y0: f64, x1: f64, y1: f64) -> Self {
        Self { x0, y0, x1, y1 }
    }

    /// The same rectangle with its corners in order.
    pub fn normalized(self) -> Self {
        Self { x0: self.x0.min(self.x1), y0: self.y0.min(self.y1), x1: self.x0.max(self.x1), y1: self.y0.max(self.y1) }
    }

    pub fn width(self) -> f64 {
        self.x1 - self.x0
    }

    pub fn height(self) -> f64 {
        self.y1 - self.y0
    }

    pub fn contains(self, x: f64, y: f64) -> bool {
        x >= self.x0 && x <= self.x1 && y >= self.y0 && y <= self.y1
    }

    fn finite(self) -> bool {
        [self.x0, self.y0, self.x1, self.y1].iter().all(|v| v.is_finite())
    }

    fn offset(self, dx: f64, dy: f64) -> Self {
        Self { x0: self.x0 + dx, y0: self.y0 + dy, x1: self.x1 + dx, y1: self.y1 + dy }
    }

    fn json(self) -> Value {
        json!([self.x0, self.y0, self.x1, self.y1])
    }
}

/// The largest coordinate accepted (points): far beyond any page, short of float trouble.
const MAX_COORD: f64 = 100_000.0;
/// The smallest frame side a drag creates (points).
pub const MIN_FRAME: f64 = 4.0;
/// Font sizes the interface may set (points).
pub const FONT_SIZES: std::ops::RangeInclusive<f64> = 1.0..=1296.0;
/// The largest page raster, in pixels per side.
const MAX_RASTER_SIDE: f64 = 8192.0;
/// The largest image accepted for a frame.
const MAX_IMAGE_BYTES: usize = 64 << 20;

/// What a frame holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ItemKind {
    Text,
    Image,
    Shape,
}

/// A frame on a page.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub id: u64,
    pub kind: ItemKind,
    /// Its bounds on the page.
    pub rect: Rect,
}

/// A rendered page: premultiplied RGBA8, row by row.
pub struct PageImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// One Mise en page document.
pub struct Layout {
    session: Session,
    renderer: designcraft_render::Renderer,
    /// The revision last saved (or opened); the document is unsaved while it differs.
    saved: u64,
}

/// The extension NadsPDF saves page layouts with.
pub const EXTENSION: &str = designcraft_format::EXTENSION;

/// The extensions a page layout opens from: NadsPDF's own, DesignCraft's (the same format), an
/// IDML package, and InDesign documents and templates (converted on opening).
pub const OPENS: [&str; 5] = [designcraft_format::EXTENSION, designcraft_format::LEGACY_EXTENSION, "idml", "indd", "indt"];

/// The largest file a page layout opens from.
const MAX_DOCUMENT_BYTES: usize = 512 << 20;

/// Whether `name` (by its extension) or `bytes` (a page layout package) is a file that opens in
/// the Mise en page tab rather than as a PDF.
pub fn is_layout_file(name: &str, bytes: &[u8]) -> bool {
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    OPENS.contains(&ext.as_str()) || designcraft_format::is_designcraft(bytes)
}

impl Layout {
    /// A new document of `pages` pages (at least one, at most 999) on `paper`.
    pub fn new(paper: Paper, pages: usize) -> Result<Self> {
        let pages = pages.clamp(1, 999);
        let mut session = Session::new();
        session.execute("file.new", &json!({ "preset": paper.preset(), "pages": pages, "facingPages": false })).map_err(engine)?;
        Ok(Self::with(session))
    }

    /// The sample magazine (four pages), to show what the mode does.
    pub fn sample() -> Result<Self> {
        let mut session = Session::new();
        session.execute("file.newSample", &json!({})).map_err(engine)?;
        Ok(Self::with(session))
    }

    fn with(session: Session) -> Self {
        let saved = session.doc().map_or(0, |d| d.revision);
        Self { session, renderer: designcraft_render::Renderer::new(), saved }
    }

    /// Open a page layout (`.nadsmep`), an IDML package or an InDesign document from its bytes;
    /// `name` is the file name (its extension helps tell InDesign files apart). Also returns what
    /// could not be carried over (fonts, missing parts), in words.
    pub fn open(name: &str, bytes: &[u8]) -> Result<(Self, Vec<String>)> {
        if bytes.is_empty() || bytes.len() > MAX_DOCUMENT_BYTES {
            return Err(LayoutError::Invalid(format!("{name}: the file is empty or too large")));
        }
        let mut session = Session::new();
        let v = session.execute("file.openBytes", &json!({ "name": name, "base64": base64(bytes) })).map_err(engine)?;
        let warnings = v["warnings"].as_array().map(|w| w.iter().filter_map(|x| x.as_str().map(str::to_owned)).collect()).unwrap_or_default();
        Ok((Self::with(session), warnings))
    }

    /// The document as a `.nadsmep` file. It counts as saved once the caller has written it
    /// ([`Layout::mark_saved`] with the [`Layout::revision`] taken before).
    pub fn save_bytes(&self) -> Result<Vec<u8>> {
        designcraft_format::save(self.doc()?).map_err(engine)
    }

    /// The document as it was at `revision` is now in a file.
    pub fn mark_saved(&mut self, revision: u64) {
        self.saved = revision;
    }

    /// Whether the document changed since it was last saved or opened.
    pub fn is_dirty(&self) -> bool {
        self.revision() != self.saved
    }

    fn doc(&self) -> Result<&Document> {
        self.session.doc().map(|d| &*d.doc).map_err(engine)
    }

    fn run(&mut self, id: &str, params: Value) -> Result<Value> {
        self.session.execute(id, &params).map_err(engine)
    }

    /// Changes whenever the document does (for caching rendered pages).
    pub fn revision(&self) -> u64 {
        self.session.doc().map_or(0, |d| d.revision)
    }

    pub fn page_count(&self) -> usize {
        self.doc().map_or(0, Document::page_count)
    }

    /// Page `page`'s width and height in points.
    pub fn page_size(&self, page: usize) -> Option<(f64, f64)> {
        self.doc().ok()?.page(page).map(|p| (p.width, p.height))
    }

    /// The spread holding page `page`, and the page's left edge in that spread.
    fn locate(&self, page: usize) -> Result<(usize, f64)> {
        let doc = self.doc()?;
        let (spread, _) = doc.page_loc(page).ok_or_else(|| LayoutError::Invalid(format!("no page {}", page + 1)))?;
        let x = doc.page(page).map(|p| p.x).ok_or_else(|| LayoutError::Invalid(format!("no page {}", page + 1)))?;
        Ok((spread, x))
    }

    /// Page `page` drawn at `scale` pixels per point (white paper), or `None` when there is no
    /// such page or the scale would make it too large.
    pub fn render_page(&mut self, page: usize, scale: f64) -> Option<PageImage> {
        let (w, h) = self.page_size(page)?;
        if !(scale.is_finite() && scale > 0.0) || w * scale > MAX_RASTER_SIDE || h * scale > MAX_RASTER_SIDE {
            return None;
        }
        let doc = self.session.doc().ok()?.doc.clone();
        let cache = self.session.cache.clone();
        let r = self.renderer.render_page(&doc, &cache, page, scale, false, &render_options())?;
        Some(PageImage { width: r.width, height: r.height, rgba: r.pixels })
    }

    /// The frames on page `page`, back to front, in page coordinates.
    pub fn items(&self, page: usize) -> Vec<Item> {
        let Ok((spread, x)) = self.locate(page) else { return Vec::new() };
        let Ok(doc) = self.doc() else { return Vec::new() };
        let Some(s) = doc.spreads.get(spread) else { return Vec::new() };
        let Some((w, h)) = self.page_size(page) else { return Vec::new() };
        let page_rect = Rect::new(0.0, 0.0, w, h);
        s.items
            .iter()
            .filter_map(|it| {
                let b = it.bounds();
                let rect = Rect::new(b.x0 - x, b.y0, b.x1 - x, b.y1);
                // Items of the facing page of a spread stay with it.
                let overlaps = rect.x1 > page_rect.x0 && rect.x0 < page_rect.x1 && rect.y1 > page_rect.y0 && rect.y0 < page_rect.y1;
                let kind = if it.is_text_frame() {
                    ItemKind::Text
                } else if it.graphic().is_some() {
                    ItemKind::Image
                } else {
                    ItemKind::Shape
                };
                (overlaps && rect.finite()).then_some(Item { id: it.id.0, kind, rect })
            })
            .collect()
    }

    /// The frame `id` on page `page`.
    pub fn item(&self, page: usize, id: u64) -> Option<Item> {
        self.items(page).into_iter().find(|i| i.id == id)
    }

    /// The front-most frame at (`x`, `y`) on page `page`.
    pub fn item_at(&self, page: usize, x: f64, y: f64) -> Option<Item> {
        self.items(page).into_iter().rev().find(|i| i.rect.contains(x, y))
    }

    fn check_rect(rect: Rect) -> Result<Rect> {
        let r = rect.normalized();
        if !r.finite() || [r.x0, r.y0, r.x1, r.y1].iter().any(|v| v.abs() > MAX_COORD) {
            return Err(LayoutError::Invalid("the frame is outside the page area".into()));
        }
        if r.width() < MIN_FRAME || r.height() < MIN_FRAME {
            return Err(LayoutError::Invalid("the frame is too small".into()));
        }
        Ok(r)
    }

    fn id_of(v: &Value) -> Result<u64> {
        v["id"].as_u64().ok_or_else(|| LayoutError::Engine("the engine didn't name the new frame".into()))
    }

    /// A text frame holding `text` on page `page`.
    pub fn add_text_frame(&mut self, page: usize, rect: Rect, text: &str) -> Result<u64> {
        let r = Self::check_rect(rect)?;
        let (spread, x) = self.locate(page)?;
        let v =
            self.run("frame.create", json!({ "spread": spread, "rect": r.offset(x, 0.0).json(), "content": "text", "text": text, "caret": false }))?;
        Self::id_of(&v)
    }

    /// An image frame on page `page` showing the image in `bytes` (PNG, JPEG…), fitted inside.
    pub fn add_image(&mut self, page: usize, rect: Rect, bytes: &[u8], name: &str) -> Result<u64> {
        if bytes.is_empty() || bytes.len() > MAX_IMAGE_BYTES {
            return Err(LayoutError::Invalid("the image is empty or too large".into()));
        }
        let r = Self::check_rect(rect)?;
        let (spread, x) = self.locate(page)?;
        let id = Self::id_of(&self.run("frame.create", json!({ "spread": spread, "rect": r.offset(x, 0.0).json(), "content": "graphic" }))?)?;
        let placed = self.run("file.place", json!({ "base64": base64(bytes), "name": name, "frame": id }));
        if let Err(e) = placed {
            // No empty frame left behind.
            let _ = self.run("edit.clear", json!({ "ids": [id] }));
            return Err(e);
        }
        let _ = self.run("object.fit", json!({ "mode": "fitProportionally", "ids": [id] }));
        Ok(id)
    }

    /// Move frame `id` by (`dx`, `dy`) points.
    pub fn move_item(&mut self, id: u64, dx: f64, dy: f64) -> Result<()> {
        if !(dx.is_finite() && dy.is_finite()) || dx.abs() > MAX_COORD || dy.abs() > MAX_COORD {
            return Err(LayoutError::Invalid("the move is too far".into()));
        }
        if dx == 0.0 && dy == 0.0 {
            return Ok(());
        }
        self.run("transform.move", json!({ "dx": dx, "dy": dy, "ids": [id] })).map(|_| ())
    }

    /// Give frame `id` on page `page` the bounds `to`.
    pub fn resize_item(&mut self, page: usize, id: u64, to: Rect) -> Result<()> {
        let to = Self::check_rect(to)?;
        let (_, x) = self.locate(page)?;
        let from = self.item(page, id).ok_or_else(|| LayoutError::Invalid("no such frame".into()))?.rect;
        self.run("transform.resize", json!({ "from": from.offset(x, 0.0).json(), "to": to.offset(x, 0.0).json(), "ids": [id] })).map(|_| ())
    }

    /// Delete frame `id`.
    pub fn delete(&mut self, id: u64) -> Result<()> {
        self.run("edit.clear", json!({ "ids": [id] })).map(|_| ())
    }

    /// The text of the story in text frame `id` (all of it, across threaded frames).
    pub fn text(&mut self, id: u64) -> Option<String> {
        let v = self.session.execute("story.get", &json!({ "frame": id })).ok()?;
        v["text"].as_str().map(str::to_owned)
    }

    fn story_of(&mut self, id: u64) -> Result<Value> {
        let v = self.run("story.get", json!({ "frame": id }))?;
        let story = v.get("id").cloned().unwrap_or(Value::Null);
        if story.is_null() { Err(LayoutError::Invalid("that frame holds no text".into())) } else { Ok(story) }
    }

    /// Replace the whole text of the story in text frame `id`.
    pub fn set_text(&mut self, id: u64, text: &str) -> Result<()> {
        let story = self.story_of(id)?;
        self.run("story.setText", json!({ "story": story, "text": text })).map(|_| ())
    }

    /// Set the font size of all the text in text frame `id`.
    pub fn set_font_size(&mut self, id: u64, size: f64) -> Result<()> {
        if !(size.is_finite() && FONT_SIZES.contains(&size)) {
            return Err(LayoutError::Invalid("font sizes go from 1 to 1296 pt".into()));
        }
        self.char_attrs(id, json!({ "size": size }))
    }

    /// Set the font family of all the text in text frame `id`.
    pub fn set_font_family(&mut self, id: u64, family: &str) -> Result<()> {
        self.char_attrs(id, json!({ "fontFamily": family }))
    }

    fn char_attrs(&mut self, id: u64, attrs: Value) -> Result<()> {
        self.run("selection.set", json!({ "ids": [id] }))?;
        let r = self.run("type.char", json!({ "attrs": attrs }));
        let _ = self.run("selection.set", json!({ "ids": [] }));
        r.map(|_| ())
    }

    /// Add a page after the last one; returns its index.
    pub fn add_page(&mut self) -> Result<usize> {
        self.run("layout.pages.insert", json!({ "count": 1 }))?;
        Ok(self.page_count().saturating_sub(1))
    }

    /// Undo the last change; `false` when there is nothing to undo.
    pub fn undo(&mut self) -> bool {
        self.session.execute("edit.undo", &json!({})).is_ok()
    }

    pub fn redo(&mut self) -> bool {
        self.session.execute("edit.redo", &json!({})).is_ok()
    }

    /// The document as a PDF.
    pub fn export_pdf(&mut self) -> Result<Vec<u8>> {
        let doc = self.doc()?.clone();
        designcraft_pdf::export_pdf(&doc, &self.session.cache, &designcraft_pdf::PdfOptions::default()).map_err(engine)
    }
}

/// How pages are drawn on screen: white paper, no shadow, every object.
fn render_options() -> designcraft_render::RenderOptions {
    designcraft_render::RenderOptions { paper: true, ..Default::default() }
}

/// Standard base64 (RFC 4648) with padding.
fn base64(data: &[u8]) -> String {
    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let sym = |i: u32| char::from(ABC.get(i as usize & 63).copied().unwrap_or(b'A'));
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [chunk.first().copied().unwrap_or(0), chunk.get(1).copied().unwrap_or(0), chunk.get(2).copied().unwrap_or(0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(sym(n >> 18));
        out.push(sym(n >> 12));
        out.push(if chunk.len() > 1 { sym(n >> 6) } else { '=' });
        out.push(if chunk.len() > 2 { sym(n) } else { '=' });
    }
    out
}

#[cfg(test)]
mod tests;
