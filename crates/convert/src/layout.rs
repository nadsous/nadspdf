//! The flow layout engine: sets a [`Source`] on A4 pages in the standard Helvetica faces
//! (WinAnsiEncoding), with word wrap, headings, lists, tables and images.

use pdfcraft_cos::{Dict, Document, ObjRef, Object, PdfString, Stream};
use pdfcraft_fonts::{Std14, literal, win_ansi};

use crate::model::{Align, Block, Kind, Para, Row, Run, Source, Style, Table};
use crate::{ConvertError, Mapped, map_char};

/// ISO A4 in points.
pub(crate) const A4: (f64, f64) = (595.276, 841.89);
/// 2 cm.
const MARGIN: f64 = 56.7;
/// Body text size.
pub(crate) const BODY: f64 = 11.0;
/// Smallest size a table is shrunk to so its columns fit.
const MIN_TABLE_SIZE: f64 = 6.0;
const CELL_PAD_X: f64 = 4.0;
const CELL_PAD_Y: f64 = 3.0;
/// Most pages one conversion may produce.
const MAX_PAGES: usize = 10_000;
/// The widest a column's shortest acceptable width may be: longer words break by character.
const MAX_MIN_COLUMN: f64 = 110.0;

/// The four faces used, in resource order `/F0`…`/F3`.
const FACES: [Std14; 4] = [Std14::Helvetica, Std14::HelveticaBold, Std14::HelveticaOblique, Std14::HelveticaBoldOblique];

fn face(style: Style, bold: bool) -> Std14 {
    match (style.bold || bold, style.italic) {
        (false, false) => Std14::Helvetica,
        (true, false) => Std14::HelveticaBold,
        (false, true) => Std14::HelveticaOblique,
        (true, true) => Std14::HelveticaBoldOblique,
    }
}

fn face_index(f: Std14) -> usize {
    FACES.iter().position(|x| *x == f).unwrap_or(0)
}

/// Text in one face, already reduced to characters WinAnsiEncoding has.
#[derive(Clone, Debug, PartialEq)]
struct Piece {
    text: String,
    font: Std14,
    underline: bool,
    width: f64,
}

/// One set line: pieces at x offsets from the line start.
#[derive(Clone, Debug, Default, PartialEq)]
struct Line {
    items: Vec<(f64, Piece)>,
    width: f64,
}

#[derive(Debug)]
enum Tok {
    Word(Vec<Piece>, f64),
    Space(f64),
    Break,
}

/// Splits runs into words, spaces and forced breaks, mapping each character to one Helvetica can
/// draw (`escaped` counts those written as `[U+XXXX]`).
fn tokens(runs: &[Run], size: f64, bold: bool, escaped: &mut usize) -> Vec<Tok> {
    let mut out = Vec::new();
    let mut word: Vec<Piece> = Vec::new();
    let finish = |word: &mut Vec<Piece>, out: &mut Vec<Tok>| {
        if word.is_empty() {
            return;
        }
        let mut total = 0.0;
        for p in word.iter_mut() {
            p.width = p.font.text_width(&p.text, size);
            total += p.width;
        }
        out.push(Tok::Word(std::mem::take(word), total));
    };
    for run in runs {
        let font = face(run.style, bold);
        let space = font.text_width(" ", size);
        let push = |word: &mut Vec<Piece>, s: &str| match word.last_mut() {
            Some(p) if p.font == font && p.underline == run.style.underline => p.text.push_str(s),
            _ => word.push(Piece { text: s.to_string(), font, underline: run.style.underline, width: 0.0 }),
        };
        for c in run.text.chars() {
            match c {
                ' ' => {
                    finish(&mut word, &mut out);
                    out.push(Tok::Space(space));
                }
                '\t' => {
                    finish(&mut word, &mut out);
                    out.push(Tok::Space(space * 4.0));
                }
                '\n' => {
                    finish(&mut word, &mut out);
                    out.push(Tok::Break);
                }
                _ => match map_char(c) {
                    Mapped::Char(' ') => {
                        finish(&mut word, &mut out);
                        out.push(Tok::Space(space));
                    }
                    Mapped::Char(m) => push(&mut word, m.encode_utf8(&mut [0; 4])),
                    Mapped::Str(s) => push(&mut word, s),
                    Mapped::Drop => {}
                    Mapped::Escape => {
                        *escaped = escaped.saturating_add(1);
                        push(&mut word, &format!("[U+{:04X}]", u32::from(c)));
                    }
                },
            }
        }
    }
    finish(&mut word, &mut out);
    out
}

/// Greedy line breaking within `width`; words longer than a line break by character.
fn wrap(toks: Vec<Tok>, size: f64, width: f64) -> Vec<Line> {
    let width = width.max(1.0);
    let mut lines = Vec::new();
    let mut cur = Line::default();
    let (mut pending, mut wrapped) = (0.0f64, false);
    // A word one plain space after a word in the same face joins its text, space included, so
    // the line is one text object that reads (and copies) as words.
    let place = |cur: &mut Line, at: f64, pieces: Vec<Piece>| {
        let mut x = at;
        let line_end = cur.width;
        for (i, p) in pieces.into_iter().enumerate() {
            let w = p.width;
            match cur.items.last_mut() {
                Some((_, last))
                    if i == 0
                        && last.font == p.font
                        && last.underline == p.underline
                        && (at - line_end - p.font.text_width(" ", size)).abs() < 1e-6 =>
                {
                    last.text.push(' ');
                    last.text.push_str(&p.text);
                    last.width += at - line_end + w;
                }
                _ => cur.items.push((x, p)),
            }
            x += w;
        }
        cur.width = x;
    };
    for tok in toks {
        match tok {
            Tok::Space(w) => {
                if !(cur.items.is_empty() && wrapped) {
                    pending += w;
                }
            }
            Tok::Break => {
                lines.push(std::mem::take(&mut cur));
                pending = 0.0;
                wrapped = false;
            }
            Tok::Word(pieces, w) => {
                let start = cur.width + pending;
                if start + w <= width {
                    place(&mut cur, start, pieces);
                } else if w <= width {
                    if !cur.items.is_empty() {
                        lines.push(std::mem::take(&mut cur));
                        wrapped = true;
                    }
                    place(&mut cur, 0.0, pieces);
                } else {
                    // Longer than a line: break it by character.
                    if !cur.items.is_empty() {
                        lines.push(std::mem::take(&mut cur));
                        wrapped = true;
                    }
                    let mut chunk = Line::default();
                    for p in pieces {
                        for ch in p.text.chars() {
                            let s = ch.encode_utf8(&mut [0; 4]).to_string();
                            let cw = p.font.text_width(&s, size);
                            if chunk.width + cw > width && !chunk.items.is_empty() {
                                lines.push(std::mem::take(&mut chunk));
                                wrapped = true;
                            }
                            let at = chunk.width;
                            match chunk.items.last_mut() {
                                Some((_, last)) if last.font == p.font && last.underline == p.underline => {
                                    last.text.push(ch);
                                    last.width += cw;
                                }
                                _ => chunk.items.push((at, Piece { text: s, font: p.font, underline: p.underline, width: cw })),
                            }
                            chunk.width = at + cw;
                        }
                    }
                    cur = chunk;
                }
                pending = 0.0;
            }
        }
    }
    lines.push(cur);
    lines
}

/// The width of runs set on one line, and of their longest word.
fn measure(runs: &[Run], size: f64, bold: bool) -> (f64, f64) {
    let mut ignored = 0;
    let (mut line, mut widest, mut longest) = (0.0f64, 0.0f64, 0.0f64);
    for tok in tokens(runs, size, bold, &mut ignored) {
        match tok {
            Tok::Word(_, w) => {
                line += w;
                longest = longest.max(w);
            }
            Tok::Space(w) => line += w,
            Tok::Break => {
                widest = widest.max(line);
                line = 0.0;
            }
        }
    }
    (widest.max(line), longest)
}

/// What a conversion produced besides the document.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    /// Pages written.
    pub pages: usize,
    /// Characters Helvetica can't draw, written as `[U+XXXX]`.
    pub escaped: usize,
    /// Images that could not be embedded (a placeholder is shown instead).
    pub images_skipped: usize,
    /// What else could not be carried over.
    pub notes: Vec<String>,
}

struct Writer {
    doc: Document,
    pages: ObjRef,
    fonts: Dict,
    size: (f64, f64),
    y: f64,
    content: Vec<u8>,
    xobjects: Dict,
    has_content: bool,
    report: Report,
}

/// One cell of a laid-out row: its first grid column, column span, and set lines.
struct Slot {
    col: usize,
    span: usize,
    lines: Vec<Line>,
}

struct LaidRow {
    slots: Vec<Slot>,
    header: bool,
    lines: usize,
}

impl Writer {
    fn new() -> Result<Self, ConvertError> {
        let mut doc = Document::new_empty();
        // `Document::new_empty` always has a page tree.
        let pages = doc
            .root()
            .and_then(|r| doc.get(r).as_dict().and_then(|d| d.reference(b"Pages")))
            .ok_or_else(|| ConvertError::Pdf("the new document has no page tree".into()))?;
        let mut fonts = Dict::new();
        for (i, f) in FACES.iter().enumerate() {
            let mut font = Dict::new();
            font.set(b"Type".to_vec(), Object::name("Font"));
            font.set(b"Subtype".to_vec(), Object::name("Type1"));
            font.set(b"BaseFont".to_vec(), Object::name(f.base_font()));
            font.set(b"Encoding".to_vec(), Object::name("WinAnsiEncoding"));
            let r = doc.add(font);
            fonts.set(format!("F{i}").into_bytes(), Object::Ref(r));
        }
        Ok(Self {
            doc,
            pages,
            fonts,
            size: A4,
            y: A4.1 - MARGIN,
            content: Vec::new(),
            xobjects: Dict::new(),
            has_content: false,
            report: Report::default(),
        })
    }

    fn left(&self) -> f64 {
        MARGIN
    }

    fn text_width(&self) -> f64 {
        (self.size.0 - 2.0 * MARGIN).max(72.0)
    }

    fn top(&self) -> f64 {
        self.size.1 - MARGIN
    }

    fn bottom(&self) -> f64 {
        MARGIN
    }

    /// Write the current page into the document and start a fresh one.
    fn flush_page(&mut self) -> Result<(), ConvertError> {
        if self.report.pages >= MAX_PAGES {
            return Err(ConvertError::TooLarge(format!("the document would have more than {MAX_PAGES} pages")));
        }
        let (w, h) = self.size;
        let mut res = Dict::new();
        res.set(b"Font".to_vec(), Object::Dict(self.fonts.clone()));
        if !self.xobjects.is_empty() {
            res.set(b"XObject".to_vec(), Object::Dict(std::mem::take(&mut self.xobjects)));
        }
        let mut page = Dict::new();
        page.set(b"Type".to_vec(), Object::name("Page"));
        page.set(b"Parent".to_vec(), Object::Ref(self.pages));
        page.set(b"MediaBox".to_vec(), Object::Array(vec![0.into(), 0.into(), Object::Real(w), Object::Real(h)]));
        page.set(b"Resources".to_vec(), Object::Dict(res));
        let s = self.doc.add(Object::Stream(Stream::flate(Dict::new(), &std::mem::take(&mut self.content))));
        page.set(b"Contents".to_vec(), Object::Ref(s));
        let r = self.doc.add(page);
        self.doc
            .update_dict(self.pages, |d| {
                let mut kids = d.get(b"Kids").and_then(|k| k.as_array().cloned()).unwrap_or_default();
                kids.push(Object::Ref(r));
                d.set(b"Count".to_vec(), Object::Int(i64::try_from(kids.len()).unwrap_or(i64::MAX)));
                d.set(b"Kids".to_vec(), Object::Array(kids));
            })
            .map_err(|e| ConvertError::Pdf(e.to_string()))?;
        self.report.pages += 1;
        self.has_content = false;
        self.y = self.top();
        Ok(())
    }

    /// Start a new page unless this one is still empty.
    fn break_page(&mut self) -> Result<(), ConvertError> {
        if self.has_content { self.flush_page() } else { Ok(()) }
    }

    /// Make room for `need` points, starting a new page if this one has too little left.
    fn ensure(&mut self, need: f64) -> Result<(), ConvertError> {
        if self.y - need < self.bottom() && self.has_content { self.flush_page() } else { Ok(()) }
    }

    fn at_top(&self) -> bool {
        !self.has_content
    }

    fn draw_line(&mut self, line: &Line, x: f64, baseline: f64, size: f64) {
        for (dx, p) in &line.items {
            if p.text.is_empty() {
                continue;
            }
            let px = x + dx;
            self.content.extend(format!("BT /F{} {size:.2} Tf {px:.2} {baseline:.2} Td ", face_index(p.font)).into_bytes());
            self.content.extend(literal(&win_ansi(&p.text)));
            self.content.extend_from_slice(b" Tj ET\n");
            if p.underline {
                self.content
                    .extend(format!("{px:.2} {:.2} {:.2} {:.2} re f\n", baseline - size * 0.15, p.width, (size * 0.06).max(0.4)).into_bytes());
            }
            self.has_content = true;
        }
    }

    fn para(&mut self, p: &Para) -> Result<(), ConvertError> {
        let (size, bold, before, after) = match p.kind {
            Kind::Title => (24.0, true, 6.0, 14.0),
            Kind::Heading(1) => (18.0, true, 16.0, 8.0),
            Kind::Heading(2) => (15.0, true, 14.0, 6.0),
            Kind::Heading(3) => (13.0, true, 12.0, 5.0),
            Kind::Heading(_) => (11.5, true, 10.0, 4.0),
            Kind::Body => (BODY, false, 0.0, 6.0),
            Kind::ListItem { .. } => (BODY, false, 0.0, 3.0),
        };
        let lh = size * 1.25;
        let indent = f64::from(p.indent.min(10)) * 24.0;
        let mut x = self.left() + indent;
        let mut marker = None;
        if let Kind::ListItem { level, marker: m } = &p.kind {
            let base = x + f64::from((*level).min(8)) * 18.0;
            let mut ignored = 0;
            let mlines = wrap(tokens(&[Run::plain(m.clone())], size, false, &mut ignored), size, f64::MAX);
            let mw = mlines.first().map_or(0.0, |l| l.width);
            x = base + (mw + 6.0).max(18.0);
            marker = mlines.into_iter().next().map(|l| (l, base));
        }
        let width = (self.left() + self.text_width() - x).max(36.0);
        let lines = wrap(tokens(&p.runs, size, bold, &mut self.report.escaped), size, width);
        if matches!(p.kind, Kind::Title | Kind::Heading(_)) {
            // Keep a heading with the start of what follows it.
            let n = lines.len().min(3) as f64;
            self.ensure(before + n * lh + 3.0 * BODY * 1.25)?;
        }
        if !self.at_top() {
            self.y -= before;
        }
        for (i, line) in lines.iter().enumerate() {
            self.ensure(lh)?;
            let baseline = self.y - size * 0.95;
            if i == 0
                && let Some((m, mx)) = &marker
            {
                self.draw_line(m, *mx, baseline, size);
            }
            let dx = match p.align {
                Align::Left => 0.0,
                Align::Center => ((width - line.width) / 2.0).max(0.0),
                Align::Right => (width - line.width).max(0.0),
            };
            self.draw_line(line, x + dx, baseline, size);
            self.y -= lh;
            // An empty paragraph still takes its line.
            self.has_content = true;
        }
        self.y -= after;
        Ok(())
    }

    fn placeholder(&mut self, text: String) -> Result<(), ConvertError> {
        self.para(&Para::new(Kind::Body, vec![Run::styled(text, Style { italic: true, ..Style::default() })]))
    }

    fn image(&mut self, name: &str, bytes: &[u8], size: Option<(f64, f64)>) -> Result<(), ConvertError> {
        let (r, natural) = match pdfcraft_create::image_xobject(&mut self.doc, name, bytes) {
            Ok(x) => x,
            Err(_) => {
                self.report.images_skipped += 1;
                return self.placeholder(format!("[image {name} could not be converted]"));
            }
        };
        let valid = |(w, h): (f64, f64)| w.is_finite() && h.is_finite() && w > 0.1 && h > 0.1;
        let Some((w, h)) = size.filter(|s| valid(*s)).or(Some(natural).filter(|s| valid(*s))) else {
            self.report.images_skipped += 1;
            return self.placeholder(format!("[image {name} has no size]"));
        };
        let k = (self.text_width() / w).min((self.top() - self.bottom()) / h).min(1.0);
        let (w, h) = (w * k, h * k);
        self.ensure(h + 6.0)?;
        let n = format!("Im{}", self.xobjects.len());
        self.content.extend(format!("q {w:.3} 0 0 {h:.3} {:.3} {:.3} cm /{n} Do Q\n", self.left(), self.y - h).into_bytes());
        self.xobjects.set(n.into_bytes(), Object::Ref(r));
        self.has_content = true;
        self.y -= h + 6.0;
        Ok(())
    }

    /// Lay out the cells of `row` that start within `cols` (a column group), with the widths of
    /// that group.
    fn lay_row(&mut self, row: &Row, cols: &std::ops::Range<usize>, widths: &[f64], size: f64) -> LaidRow {
        let mut slots = Vec::new();
        let mut col = 0usize;
        for cell in &row.cells {
            let span = cell.span.max(1);
            let start = col;
            col = col.saturating_add(span);
            if start < cols.start || start >= cols.end {
                continue;
            }
            let span = span.min(cols.end - start);
            let w: f64 = widths.get(start - cols.start..start - cols.start + span).map_or(0.0, |s| s.iter().sum());
            let mut lines = Vec::new();
            for para in &cell.paras {
                lines.extend(wrap(tokens(para, size, row.header, &mut self.report.escaped), size, w - 2.0 * CELL_PAD_X));
            }
            if lines.is_empty() {
                lines.push(Line::default());
            }
            slots.push(Slot { col: start - cols.start, span, lines });
        }
        // Columns the row has no cell for are drawn empty, so the grid stays whole.
        let n = cols.end - cols.start;
        let mut covered = vec![false; n];
        for s in &slots {
            for c in covered.iter_mut().skip(s.col).take(s.span) {
                *c = true;
            }
        }
        for (c, used) in covered.iter().enumerate() {
            if !used {
                slots.push(Slot { col: c, span: 1, lines: vec![Line::default()] });
            }
        }
        let lines = slots.iter().map(|s| s.lines.len()).max().unwrap_or(1).max(1);
        LaidRow { slots, header: row.header, lines }
    }

    /// Draw lines `from..to` of a laid-out row at the current position.
    fn draw_row(&mut self, row: &LaidRow, from: usize, to: usize, widths: &[f64], size: f64) {
        let lh = size * 1.2;
        let h = (to - from) as f64 * lh + 2.0 * CELL_PAD_Y;
        let top = self.y;
        for slot in &row.slots {
            let x = self.left() + widths.iter().take(slot.col).sum::<f64>();
            let w: f64 = widths.iter().skip(slot.col).take(slot.span).sum();
            if row.header {
                self.content.extend(format!("q 0.9 g {x:.2} {:.2} {w:.2} {h:.2} re f Q\n", top - h).into_bytes());
            }
            self.content.extend(format!("q 0.5 w 0.55 G {x:.2} {:.2} {w:.2} {h:.2} re S Q\n", top - h).into_bytes());
            for (i, line) in slot.lines.iter().enumerate().skip(from).take(to - from) {
                let baseline = top - CELL_PAD_Y - (i - from) as f64 * lh - size * 0.92;
                self.draw_line(line, x + CELL_PAD_X, baseline, size);
            }
        }
        self.has_content = true;
        self.y -= h;
    }

    fn table(&mut self, t: &Table) -> Result<(), ConvertError> {
        let ncols = t.columns();
        if ncols == 0 || t.rows.is_empty() {
            return Ok(());
        }
        let avail = self.text_width();
        let pad = 2.0 * CELL_PAD_X;
        // Natural and smallest widths per column at `size` (cells spanning columns don't count).
        let widths_at = |size: f64| {
            let (mut natural, mut least) = (vec![0.0f64; ncols], vec![0.0f64; ncols]);
            for row in &t.rows {
                let mut col = 0usize;
                for cell in &row.cells {
                    let span = cell.span.max(1);
                    if span == 1
                        && let (Some(n), Some(l)) = (natural.get_mut(col), least.get_mut(col))
                    {
                        for para in &cell.paras {
                            let (w, longest) = measure(para, size, row.header);
                            *n = n.max(w.min(avail));
                            *l = l.max(longest.min(MAX_MIN_COLUMN));
                        }
                    }
                    col = col.saturating_add(span);
                }
            }
            let natural: Vec<f64> = natural.iter().map(|w| (w + pad).max(24.0)).collect();
            let least: Vec<f64> = least.iter().map(|w| (w + pad).max(24.0)).collect();
            (natural, least)
        };
        let mut size = t.size;
        let (mut natural, mut least) = widths_at(size);
        if t.widths.is_none() || !t.fill {
            for _ in 0..6 {
                let need: f64 = least.iter().sum();
                if need <= avail || size <= MIN_TABLE_SIZE {
                    break;
                }
                size = (size * avail / need * 0.98).max(MIN_TABLE_SIZE);
                (natural, least) = widths_at(size);
            }
        }
        // Column groups: all columns, or as many as fit across the page each.
        let mut groups = Vec::new();
        if t.split_columns && least.iter().sum::<f64>() > avail {
            let mut start = 0usize;
            let mut used = 0.0;
            for (c, w) in least.iter().enumerate() {
                if c > start && used + w > avail {
                    groups.push(start..c);
                    start = c;
                    used = 0.0;
                }
                used += w;
            }
            groups.push(start..ncols);
        } else {
            groups.push(0..ncols);
        }
        for (gi, cols) in groups.iter().enumerate() {
            let (nat, min) = (natural.get(cols.clone()).unwrap_or_default(), least.get(cols.clone()).unwrap_or_default());
            let widths = column_widths(t, cols, nat, min, avail);
            if gi > 0 {
                self.y -= 10.0;
            }
            self.rows(t, cols, &widths, size)?;
        }
        self.y -= 8.0;
        Ok(())
    }

    fn rows(&mut self, t: &Table, cols: &std::ops::Range<usize>, widths: &[f64], size: f64) -> Result<(), ConvertError> {
        let lh = size * 1.2;
        let page_room = self.top() - self.bottom();
        // Leading header rows repeat at the top of each new page, unless they are very tall.
        let headers: Vec<LaidRow> = t.rows.iter().take_while(|r| r.header).map(|r| self.lay_row(r, cols, widths, size)).collect();
        let header_h: f64 = headers.iter().map(|r| r.lines as f64 * lh + 2.0 * CELL_PAD_Y).sum();
        let repeat = header_h < page_room / 3.0;
        for (i, row) in t.rows.iter().enumerate() {
            let laid = self.lay_row(row, cols, widths, size);
            let full = laid.lines as f64 * lh + 2.0 * CELL_PAD_Y;
            let mut from = 0usize;
            // At most one page break before each piece of the row, so the loop always advances.
            let mut flushed = false;
            while from < laid.lines {
                let room = self.y - self.bottom() - 2.0 * CELL_PAD_Y;
                let fit = if room > 0.0 { (room / lh).floor() as usize } else { 0 };
                let whole_fits_on_a_page = full <= page_room - if repeat { header_h } else { 0.0 };
                if self.has_content && !flushed && (fit == 0 || (from == 0 && fit < laid.lines && whole_fits_on_a_page)) {
                    self.flush_page()?;
                    flushed = true;
                    if repeat && i >= headers.len() {
                        for h in &headers {
                            self.draw_row(h, 0, h.lines, widths, size);
                        }
                    }
                    continue;
                }
                let n = fit.clamp(1, laid.lines - from);
                self.draw_row(&laid, from, from + n, widths, size);
                from += n;
                flushed = false;
            }
        }
        Ok(())
    }

    fn finish(mut self, title: &str) -> Result<(Document, Report), ConvertError> {
        if self.has_content || self.report.pages == 0 {
            self.flush_page()?;
        }
        let mut info = Dict::new();
        info.set(b"Title".to_vec(), PdfString::text(title));
        info.set(b"Producer".to_vec(), PdfString::text("NadsPDF"));
        let r = self.doc.add(info);
        self.doc.trailer_mut().set(b"Info".to_vec(), Object::Ref(r));
        Ok((self.doc, self.report))
    }
}

/// Widths of the columns `cols` of `t` within `avail` points, from the source's grid when it has
/// one, else from the content: natural widths when they fit, else shares between the smallest and
/// natural widths, else the smallest widths scaled down.
fn column_widths(t: &Table, cols: &std::ops::Range<usize>, natural: &[f64], least: &[f64], avail: f64) -> Vec<f64> {
    if let Some(grid) = &t.widths
        && let Some(grid) = grid.get(cols.clone())
        && grid.len() == natural.len()
        && grid.iter().all(|w| w.is_finite() && *w > 0.0)
    {
        let total: f64 = grid.iter().sum();
        if total > 0.0 {
            return grid.iter().map(|w| w / total * avail).collect();
        }
    }
    let (sum_nat, sum_min): (f64, f64) = (natural.iter().sum(), least.iter().sum());
    let mut widths: Vec<f64> = if sum_nat <= avail {
        natural.to_vec()
    } else if sum_min <= avail && sum_nat > sum_min {
        let k = (avail - sum_min) / (sum_nat - sum_min);
        natural.iter().zip(least).map(|(n, m)| m + (n - m).max(0.0) * k).collect()
    } else if sum_min > 0.0 {
        least.iter().map(|m| m * avail / sum_min).collect()
    } else {
        vec![avail / natural.len().max(1) as f64; natural.len()]
    };
    let sum: f64 = widths.iter().sum();
    if t.fill && sum < avail && sum > 0.0 {
        widths.iter_mut().for_each(|w| *w *= avail / sum);
    }
    widths
}

/// Lay `source` out as a PDF titled `title`.
pub(crate) fn render(title: &str, source: &Source) -> Result<(Document, Report), ConvertError> {
    let mut w = Writer::new()?;
    for block in &source.blocks {
        match block {
            Block::Para(p) => w.para(p)?,
            Block::Table(t) => w.table(t)?,
            Block::Image { name, bytes, size } => w.image(name, bytes, *size)?,
            Block::PageBreak => w.break_page()?,
            Block::Section { landscape } => {
                w.break_page()?;
                w.size = if *landscape { (A4.1, A4.0) } else { A4 };
                w.y = w.top();
            }
        }
    }
    w.report.notes.extend(source.notes.iter().cloned());
    w.finish(title)
}

/// Whether a table with these rows would need a landscape page to show its columns at `size`.
pub(crate) fn wants_landscape(rows: &[Row], size: f64) -> bool {
    let ncols = rows.iter().map(|r| r.cells.len()).max().unwrap_or(0);
    let mut natural = vec![0.0f64; ncols];
    for row in rows {
        for (c, cell) in row.cells.iter().enumerate() {
            if let Some(n) = natural.get_mut(c) {
                for para in &cell.paras {
                    *n = n.max(measure(para, size, row.header).0.min(200.0));
                }
            }
        }
    }
    let total: f64 = natural.iter().map(|w| w + 2.0 * CELL_PAD_X).sum();
    total > A4.0 - 2.0 * MARGIN
}

#[cfg(test)]
pub(crate) fn wrap_text(text: &str, size: f64, width: f64) -> Vec<String> {
    let mut e = 0;
    wrap(tokens(&[Run::plain(text)], size, false, &mut e), size, width)
        .into_iter()
        .map(|l| l.items.iter().map(|(_, p)| p.text.as_str()).collect::<Vec<_>>().join(" "))
        .collect()
}
