//! Spreadsheets and tables: Excel workbooks (`.xlsx`, `.xlsm`), OpenDocument spreadsheets
//! (`.ods`) and delimited text (`.csv`, `.tsv`). Each sheet becomes a heading and a table.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use roxmltree::Node;

use crate::ConvertError;
use crate::layout::wants_landscape;
use crate::model::{Block, Cell, Kind, Row, Run, Source, Style, Table};
use crate::xml::{MAX_DEPTH, attr, child, children, is, parse};
use crate::zipfile::{Archive, resolve};

/// Most non-empty cells one file may contribute; the rest are left out with a visible note.
const MAX_CELLS: usize = 300_000;
/// Columns and rows beyond these are left out.
const MAX_COLS: usize = 1024;
const MAX_ROWS: usize = 100_000;
/// Cell text size.
const SHEET_SIZE: f64 = 9.0;

/// One sheet's non-empty cells by (row, column).
#[derive(Default)]
struct Grid {
    name: String,
    cells: BTreeMap<(usize, usize), String>,
}

/// Counts cells against [`MAX_CELLS`] across a whole file.
struct Budget {
    left: usize,
    truncated: bool,
}

impl Budget {
    fn new() -> Self {
        Self { left: MAX_CELLS, truncated: false }
    }

    /// Store `text` at (row, col) if there is room; false once the budget is spent.
    fn put(&mut self, grid: &mut Grid, row: usize, col: usize, text: String) -> bool {
        if row >= MAX_ROWS || col >= MAX_COLS {
            self.truncated = true;
            return true;
        }
        if self.left == 0 {
            self.truncated = true;
            return false;
        }
        if text.trim().is_empty() {
            return true;
        }
        self.left -= 1;
        grid.cells.insert((row, col), text);
        true
    }
}

/// The blocks for `grids`: each non-empty sheet on its own page (landscape when its columns
/// need it), under its name when `headings` is set.
fn blocks(grids: Vec<Grid>, headings: bool, budget: &Budget) -> Source {
    let mut source = Source::default();
    let shown = grids.iter().filter(|g| !g.cells.is_empty()).count();
    for grid in grids.into_iter().filter(|g| !g.cells.is_empty()) {
        // Empty rows and columns are left out: they would only add blank grid.
        let cols: BTreeSet<usize> = grid.cells.keys().map(|(_, c)| *c).collect();
        let index: HashMap<usize, usize> = cols.iter().enumerate().map(|(i, c)| (*c, i)).collect();
        let mut rows: Vec<Row> = Vec::new();
        let mut current: Option<usize> = None;
        for ((r, c), text) in grid.cells {
            if current != Some(r) {
                current = Some(r);
                rows.push(Row { cells: vec![Cell::text(""); cols.len()], header: false });
            }
            if let (Some(row), Some(i)) = (rows.last_mut(), index.get(&c))
                && let Some(cell) = row.cells.get_mut(*i)
            {
                *cell = Cell::text(text);
            }
        }
        if rows.len() > 1
            && let Some(first) = rows.first_mut()
        {
            first.header = true;
        }
        let landscape = wants_landscape(&rows, SHEET_SIZE);
        source.blocks.push(Block::Section { landscape });
        if headings && (shown > 1 || !grid.name.is_empty()) {
            source.push_para(Kind::Heading(2), vec![Run::plain(grid.name)]);
        }
        source.blocks.push(Block::Table(Table { rows, widths: None, size: SHEET_SIZE, fill: false, split_columns: true }));
    }
    if shown == 0 {
        source.push_para(Kind::Body, vec![Run::styled("(no data)", Style { italic: true, ..Style::default() })]);
    }
    if budget.truncated {
        let note = format!("Only the first {MAX_CELLS} cells, {MAX_ROWS} rows and {MAX_COLS} columns are converted.");
        source.push_para(Kind::Body, vec![Run::styled(format!("[{note}]"), Style { italic: true, ..Style::default() })]);
        source.notes.push(note);
    }
    source
}

// ── numbers ─────────────────────────────────────────────────────────────────────────────────

/// How a number is displayed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NumKind {
    General,
    Date,
    Time,
    DateTime,
    Percent,
}

/// A number as a spreadsheet shows it by default: integers without decimals, others to ten
/// significant digits (so binary noise such as 0.30000000000000004 reads 0.3).
pub(crate) fn general(v: f64) -> String {
    if !v.is_finite() {
        return v.to_string();
    }
    if v.fract() == 0.0 && v.abs() < 1e15 {
        return format!("{v:.0}");
    }
    let magnitude = if v == 0.0 { 0 } else { v.abs().log10().floor() as i32 };
    if !(-5..15).contains(&magnitude) {
        return format!("{v:e}");
    }
    let decimals = (9 - magnitude).clamp(0, 15) as usize;
    let s = format!("{v:.decimals$}");
    if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s }
}

/// Days since 1970-01-01 → (year, month, day) (proleptic Gregorian).
fn civil(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// A spreadsheet date serial (days since 1899-12-30) as `yyyy-mm-dd`, `hh:mm` or both.
pub(crate) fn date_text(serial: f64, kind: NumKind) -> Option<String> {
    if !serial.is_finite() || !(-700_000.0..3_000_000.0).contains(&serial) {
        return None;
    }
    let days = serial.floor();
    let secs = ((serial - days) * 86_400.0).round() as i64;
    let (y, m, d) = civil(days as i64 - 25_569);
    let (h, min, s) = (secs / 3600 % 24, secs / 60 % 60, secs % 60);
    let date = format!("{y:04}-{m:02}-{d:02}");
    let time = if s == 0 { format!("{h:02}:{min:02}") } else { format!("{h:02}:{min:02}:{s:02}") };
    Some(match kind {
        NumKind::Time => time,
        NumKind::DateTime if secs != 0 => format!("{date} {time}"),
        _ => date,
    })
}

fn number_text(v: &str, kind: NumKind) -> String {
    let Ok(n) = v.trim().parse::<f64>() else { return v.to_string() };
    match kind {
        NumKind::General => general(n),
        NumKind::Percent => format!("{}%", general(n * 100.0)),
        NumKind::Date | NumKind::Time | NumKind::DateTime => date_text(n, kind).unwrap_or_else(|| general(n)),
    }
}

/// What a number format code displays: dates and times by their letters (outside quotes and
/// brackets), percentages by `%`.
pub(crate) fn classify_code(code: &str) -> NumKind {
    let mut plain = String::new();
    let (mut quoted, mut bracket, mut escape) = (false, false, false);
    for c in code.chars() {
        if escape {
            escape = false;
            continue;
        }
        match c {
            '\\' => escape = true,
            '"' => quoted = !quoted,
            '[' if !quoted => bracket = true,
            ']' if !quoted => bracket = false,
            _ if !quoted && !bracket => plain.push(c.to_ascii_lowercase()),
            _ => {}
        }
    }
    // Only the first section (positive numbers) matters.
    let first = plain.split(';').next().unwrap_or_default();
    // `m` is minutes next to hours, seconds or a colon, and months otherwise.
    let (h, s, m, colon) = (first.contains('h'), first.contains('s'), first.contains('m'), first.contains(':'));
    let date = first.contains('y') || first.contains('d') || (m && !h && !s && !colon);
    let time = h || s || (m && colon);
    match (date, time) {
        (true, true) => NumKind::DateTime,
        (true, false) => NumKind::Date,
        (false, true) => NumKind::Time,
        _ if first.contains('%') => NumKind::Percent,
        _ => NumKind::General,
    }
}

fn builtin_format(id: u32) -> NumKind {
    match id {
        9 | 10 => NumKind::Percent,
        14..=17 | 27..=31 | 34..=36 | 50..=58 => NumKind::Date,
        18..=21 | 32 | 33 | 45..=47 => NumKind::Time,
        22 => NumKind::DateTime,
        _ => NumKind::General,
    }
}

/// `xl/styles.xml` → the display kind of each cell format (`c/@s` indexes it).
fn xlsx_formats(text: &str) -> Result<Vec<NumKind>, ConvertError> {
    let xml = parse(text, "xl/styles.xml")?;
    let root = xml.root_element();
    let custom: HashMap<u32, NumKind> = child(root, "numFmts")
        .map(|n| {
            children(n, "numFmt")
                .filter_map(|f| Some((attr(f, "numFmtId")?.parse().ok()?, classify_code(attr(f, "formatCode").unwrap_or_default()))))
                .collect()
        })
        .unwrap_or_default();
    Ok(child(root, "cellXfs")
        .map(|x| {
            children(x, "xf")
                .take(100_000)
                .map(|xf| {
                    let id = attr(xf, "numFmtId").and_then(|v| v.parse::<u32>().ok()).unwrap_or(0);
                    custom.get(&id).copied().unwrap_or_else(|| builtin_format(id))
                })
                .collect()
        })
        .unwrap_or_default())
}

// ── xlsx ────────────────────────────────────────────────────────────────────────────────────

/// `B12` → (row 11, column 1).
pub(crate) fn cell_ref(r: &str) -> Option<(usize, usize)> {
    let split = r.find(|c: char| c.is_ascii_digit())?;
    let (letters, digits) = (r.get(..split)?, r.get(split..)?);
    if letters.is_empty() || letters.len() > 3 {
        return None;
    }
    let mut col = 0usize;
    for c in letters.chars() {
        if !c.is_ascii_alphabetic() {
            return None;
        }
        col = col * 26 + (c.to_ascii_uppercase() as usize - 'A' as usize + 1);
    }
    let row = digits.parse::<usize>().ok()?;
    Some((row.checked_sub(1)?, col.checked_sub(1)?))
}

/// The text of a shared or inline string: its `t` elements, leaving out phonetic runs.
fn string_item(si: Node) -> String {
    si.descendants().filter(|d| is(*d, "t") && !d.ancestors().any(|a| is(a, "rPh"))).filter_map(|d| d.text()).collect()
}

fn rels_map(text: &str, what: &str) -> Result<HashMap<String, String>, ConvertError> {
    let xml = parse(text, what)?;
    Ok(children(xml.root_element(), "Relationship").filter_map(|r| Some((attr(r, "Id")?.to_string(), attr(r, "Target")?.to_string()))).collect())
}

pub(crate) fn xlsx(bytes: &[u8]) -> Result<Source, ConvertError> {
    let mut zip = Archive::open(bytes, "Excel (.xlsx)")?;
    let workbook = zip
        .text("xl/workbook.xml")?
        .ok_or_else(|| ConvertError::Malformed("this .xlsx file has no xl/workbook.xml: it isn't an Excel workbook".into()))?;
    let rels = match zip.text("xl/_rels/workbook.xml.rels")? {
        Some(t) => rels_map(&t, "xl/_rels/workbook.xml.rels")?,
        None => HashMap::new(),
    };
    let shared: Vec<String> = match zip.text("xl/sharedStrings.xml")? {
        Some(t) => {
            let xml = parse(&t, "xl/sharedStrings.xml")?;
            children(xml.root_element(), "si").take(2_000_000).map(string_item).collect()
        }
        None => Vec::new(),
    };
    let formats = match zip.text("xl/styles.xml")? {
        Some(t) => xlsx_formats(&t)?,
        None => Vec::new(),
    };
    let wb = parse(&workbook, "xl/workbook.xml")?;
    let sheets: Vec<(String, String)> = child(wb.root_element(), "sheets")
        .map(|s| {
            children(s, "sheet")
                .filter(|s| !matches!(attr(*s, "state"), Some("hidden" | "veryHidden")))
                .filter_map(|s| {
                    let name = attr(s, "name").unwrap_or_default().to_string();
                    let target = rels.get(attr(s, "id")?)?;
                    Some((name, resolve("xl/", target)))
                })
                .collect()
        })
        .unwrap_or_default();
    let mut budget = Budget::new();
    let mut grids = Vec::new();
    'sheets: for (name, path) in sheets {
        let Some(text) = zip.text(&path)? else { continue };
        let xml = parse(&text, &path)?;
        let Some(data) = child(xml.root_element(), "sheetData") else { continue };
        let mut grid = Grid { name, cells: BTreeMap::new() };
        let mut next_row = 0usize;
        for row in children(data, "row") {
            let r = attr(row, "r").and_then(|v| v.parse::<usize>().ok()).and_then(|v| v.checked_sub(1)).unwrap_or(next_row);
            next_row = r.saturating_add(1);
            let mut next_col = 0usize;
            for c in children(row, "c") {
                let (r, col) = attr(c, "r").and_then(cell_ref).unwrap_or((r, next_col));
                next_col = col.saturating_add(1);
                let v = child(c, "v").and_then(|v| v.text()).unwrap_or_default();
                let text = match attr(c, "t") {
                    Some("s") => v.trim().parse::<usize>().ok().and_then(|i| shared.get(i)).cloned().unwrap_or_default(),
                    Some("inlineStr") => child(c, "is").map(string_item).unwrap_or_default(),
                    Some("b") => if v.trim() == "1" { "TRUE" } else { "FALSE" }.to_string(),
                    Some("str" | "e" | "d") => v.to_string(),
                    _ => {
                        let kind =
                            attr(c, "s").and_then(|s| s.parse::<usize>().ok()).and_then(|s| formats.get(s)).copied().unwrap_or(NumKind::General);
                        if v.is_empty() { String::new() } else { number_text(v, kind) }
                    }
                };
                if !budget.put(&mut grid, r, col, text) {
                    grids.push(grid);
                    break 'sheets;
                }
            }
        }
        grids.push(grid);
    }
    Ok(blocks(grids, true, &budget))
}

// ── ods ─────────────────────────────────────────────────────────────────────────────────────

/// A cell's displayed text: its paragraphs, one per line.
fn ods_text(n: Node, depth: usize, out: &mut String) {
    if depth > MAX_DEPTH {
        return;
    }
    for c in n.children() {
        if c.is_text() {
            out.push_str(c.text().unwrap_or_default());
            continue;
        }
        if !c.is_element() {
            continue;
        }
        match c.tag_name().name() {
            "p" | "h" => {
                if !out.is_empty() {
                    out.push('\n');
                }
                ods_text(c, depth + 1, out);
            }
            "s" => {
                let n = attr(c, "c").and_then(|v| v.parse::<usize>().ok()).unwrap_or(1).min(100);
                out.push_str(&" ".repeat(n));
            }
            "tab" => out.push('\t'),
            "line-break" => out.push('\n'),
            "span" | "a" | "list" | "list-item" => ods_text(c, depth + 1, out),
            // Comments are not shown in the cell.
            _ => {}
        }
    }
}

/// The rows of a table, in order, through row groups and header rows.
fn ods_rows<'a, 'i>(n: Node<'a, 'i>, depth: usize, out: &mut Vec<Node<'a, 'i>>) {
    if depth > MAX_DEPTH {
        return;
    }
    for c in n.children().filter(Node::is_element) {
        match c.tag_name().name() {
            "table-row" => out.push(c),
            "table-row-group" | "table-header-rows" | "table-rows" => ods_rows(c, depth + 1, out),
            _ => {}
        }
    }
}

pub(crate) fn ods(bytes: &[u8]) -> Result<Source, ConvertError> {
    let mut zip = Archive::open(bytes, "OpenDocument spreadsheet (.ods)")?;
    let text = zip
        .text("content.xml")?
        .ok_or_else(|| ConvertError::Malformed("this .ods file has no content.xml: it isn't an OpenDocument spreadsheet".into()))?;
    let xml = parse(&text, "content.xml")?;
    let sheet = xml
        .descendants()
        .find(|d| is(*d, "spreadsheet"))
        .ok_or_else(|| ConvertError::Malformed("content.xml holds no spreadsheet: this isn't an OpenDocument spreadsheet".into()))?;
    let mut budget = Budget::new();
    let mut grids = Vec::new();
    'tables: for table in children(sheet, "table") {
        let mut grid = Grid { name: attr(table, "name").unwrap_or_default().to_string(), cells: BTreeMap::new() };
        let mut rows = Vec::new();
        ods_rows(table, 0, &mut rows);
        let mut r = 0usize;
        for row in rows {
            let repeat = attr(row, "number-rows-repeated").and_then(|v| v.parse::<usize>().ok()).unwrap_or(1).max(1);
            let mut cells = Vec::new();
            let mut col = 0usize;
            for c in row.children().filter(|c| is(*c, "table-cell") || is(*c, "covered-table-cell")) {
                let across = attr(c, "number-columns-repeated").and_then(|v| v.parse::<usize>().ok()).unwrap_or(1).max(1);
                let mut text = String::new();
                ods_text(c, 0, &mut text);
                if !text.trim().is_empty() {
                    for k in 0..across.min(MAX_COLS) {
                        cells.push((col.saturating_add(k), text.clone()));
                    }
                }
                // Empty columns are dropped too: a run of empty cells counts as one.
                col = col.saturating_add(if text.trim().is_empty() { 1 } else { across });
            }
            // Empty rows are dropped from the table anyway: a run of them (often a million, to
            // the sheet's end) counts as one, so later rows stay within the limits.
            if cells.is_empty() {
                r = r.saturating_add(1);
                continue;
            }
            for _ in 0..repeat.min(MAX_ROWS) {
                for (c, text) in &cells {
                    if !budget.put(&mut grid, r, *c, text.clone()) {
                        grids.push(grid);
                        break 'tables;
                    }
                }
                r = r.saturating_add(1);
                if r >= MAX_ROWS {
                    break;
                }
            }
        }
        grids.push(grid);
    }
    Ok(blocks(grids, true, &budget))
}

// ── csv ─────────────────────────────────────────────────────────────────────────────────────

/// The delimiter of delimited text: the one of `,` `;` and tab that splits the first lines
/// most consistently (quoted text ignored).
pub(crate) fn detect_delimiter(text: &str) -> char {
    // The first records, split at line breaks outside quotes.
    let mut lines: Vec<&str> = Vec::new();
    let (mut quoted, mut start) = (false, 0usize);
    for (i, c) in text.char_indices() {
        if lines.len() >= 20 {
            break;
        }
        match c {
            '"' => quoted = !quoted,
            '\n' if !quoted => {
                lines.push(text.get(start..i).unwrap_or_default());
                start = i + 1;
            }
            _ => {}
        }
    }
    if lines.len() < 20 {
        lines.push(text.get(start..).unwrap_or_default());
    }
    lines.retain(|l| !l.trim().is_empty());
    let mut best = (',', 0usize);
    for d in [',', ';', '\t'] {
        let counts: Vec<usize> = lines
            .iter()
            .map(|l| {
                let mut quoted = false;
                l.chars()
                    .filter(|c| {
                        if *c == '"' {
                            quoted = !quoted;
                        }
                        !quoted && *c == d
                    })
                    .count()
            })
            .collect();
        let min = counts.iter().copied().min().unwrap_or(0);
        let consistent = counts.windows(2).all(|w| w.first() == w.last());
        let score = if min == 0 {
            0
        } else if consistent {
            10_000 + min
        } else {
            min
        };
        if score > best.1 {
            best = (d, score);
        }
    }
    best.0
}

/// RFC 4180 records: quoted fields may hold delimiters, line breaks and doubled quotes.
fn records(text: &str, delimiter: char, budget: &mut Budget, grid: &mut Grid) {
    let (mut row, mut col) = (0usize, 0usize);
    let mut field = String::new();
    let (mut quoted, mut was_quoted) = (false, false);
    let mut chars = text.chars().peekable();
    let mut row_has_content = false;
    while let Some(c) = chars.next() {
        if quoted {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    field.push('"');
                } else {
                    quoted = false;
                }
            } else {
                field.push(c);
            }
            continue;
        }
        match c {
            '"' if field.is_empty() && !was_quoted => {
                quoted = true;
                was_quoted = true;
            }
            _ if c == delimiter => {
                row_has_content |= !field.trim().is_empty();
                if !budget.put(grid, row, col, std::mem::take(&mut field)) {
                    return;
                }
                col = col.saturating_add(1);
                was_quoted = false;
            }
            '\r' | '\n' => {
                if c == '\r' && chars.peek() == Some(&'\n') {
                    chars.next();
                }
                row_has_content |= !field.trim().is_empty();
                if !budget.put(grid, row, col, std::mem::take(&mut field)) {
                    return;
                }
                if row_has_content {
                    row = row.saturating_add(1);
                }
                if row >= MAX_ROWS {
                    budget.truncated = true;
                    return;
                }
                col = 0;
                was_quoted = false;
                row_has_content = false;
            }
            _ => field.push(c),
        }
    }
    budget.put(grid, row, col, field);
}

pub(crate) fn csv(bytes: &[u8], delimiter: Option<char>) -> Result<Source, ConvertError> {
    let text = pdfcraft_create::decode_text(bytes);
    let delimiter = delimiter.unwrap_or_else(|| detect_delimiter(&text));
    let mut budget = Budget::new();
    let mut grid = Grid::default();
    records(&text, delimiter, &mut budget, &mut grid);
    Ok(blocks(vec![grid], false, &budget))
}
