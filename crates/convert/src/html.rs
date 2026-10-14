//! HTML files, read leniently with a small tag scanner: headings, paragraphs, line breaks, bold
//! and italic, lists and tables. Scripts, styles and comments are left out; images and CSS
//! layout are not rendered.

use crate::model::{Align, Block, Cell, Kind, Para, Row, Run, Source, Style, Table};

/// Most tags read from one file.
const MAX_TAGS: usize = 2_000_000;
/// Deepest list or table nesting kept apart (deeper ones are read as the deepest).
const MAX_NESTING: usize = 32;

/// `&name;` and `&#n;` / `&#xh;` references in `s`, decoded. Unknown ones stay as written.
pub(crate) fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(at) = rest.find('&') {
        out.push_str(rest.get(..at).unwrap_or_default());
        let after = rest.get(at + 1..).unwrap_or_default();
        let end = after.char_indices().take(12).find(|(_, c)| *c == ';').map(|(i, _)| i);
        let decoded = end.and_then(|end| {
            let name = after.get(..end)?;
            let c = if let Some(num) = name.strip_prefix('#') {
                let v = match num.strip_prefix(['x', 'X']) {
                    Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                    None => num.parse::<u32>().ok()?,
                };
                // Windows-1252 code points written as numbers (&#150;) mean the 1252 characters.
                let v = match v {
                    0x80..=0x9f => cp1252(v).unwrap_or(v),
                    _ => v,
                };
                char::from_u32(v)?.to_string()
            } else {
                named(name)?.to_string()
            };
            Some((c, end))
        });
        match decoded {
            Some((c, end)) => {
                out.push_str(&c);
                rest = after.get(end + 1..).unwrap_or_default();
            }
            None => {
                out.push('&');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

fn cp1252(v: u32) -> Option<u32> {
    const HIGH: [u32; 32] = [
        0x20ac, 0, 0x201a, 0x0192, 0x201e, 0x2026, 0x2020, 0x2021, 0x02c6, 0x2030, 0x0160, 0x2039, 0x0152, 0, 0x017d, 0, 0, 0x2018, 0x2019, 0x201c,
        0x201d, 0x2022, 0x2013, 0x2014, 0x02dc, 0x2122, 0x0161, 0x203a, 0x0153, 0, 0x017e, 0x0178,
    ];
    HIGH.get(usize::try_from(v.checked_sub(0x80)?).ok()?).copied().filter(|c| *c != 0)
}

fn named(name: &str) -> Option<&'static str> {
    Some(match name {
        "amp" => "&",
        "lt" => "<",
        "gt" => ">",
        "quot" => "\"",
        "apos" => "'",
        "nbsp" => "\u{a0}",
        "ensp" | "emsp" | "thinsp" => " ",
        "shy" | "zwnj" | "zwj" | "lrm" | "rlm" => "",
        "laquo" => "«",
        "raquo" => "»",
        "lsaquo" => "‹",
        "rsaquo" => "›",
        "lsquo" => "‘",
        "rsquo" => "’",
        "sbquo" => "‚",
        "ldquo" => "“",
        "rdquo" => "”",
        "bdquo" => "„",
        "hellip" => "…",
        "ndash" => "–",
        "mdash" => "—",
        "bull" => "•",
        "middot" => "·",
        "copy" => "©",
        "reg" => "®",
        "trade" => "™",
        "euro" => "€",
        "pound" => "£",
        "yen" => "¥",
        "cent" => "¢",
        "deg" => "°",
        "times" => "×",
        "divide" => "÷",
        "plusmn" => "±",
        "para" => "¶",
        "sect" => "§",
        "iexcl" => "¡",
        "iquest" => "¿",
        "szlig" => "ß",
        "aelig" => "æ",
        "AElig" => "Æ",
        "oelig" => "œ",
        "OElig" => "Œ",
        "agrave" => "à",
        "aacute" => "á",
        "acirc" => "â",
        "atilde" => "ã",
        "auml" => "ä",
        "aring" => "å",
        "ccedil" => "ç",
        "egrave" => "è",
        "eacute" => "é",
        "ecirc" => "ê",
        "euml" => "ë",
        "igrave" => "ì",
        "iacute" => "í",
        "icirc" => "î",
        "iuml" => "ï",
        "ntilde" => "ñ",
        "ograve" => "ò",
        "oacute" => "ó",
        "ocirc" => "ô",
        "otilde" => "õ",
        "ouml" => "ö",
        "oslash" => "ø",
        "ugrave" => "ù",
        "uacute" => "ú",
        "ucirc" => "û",
        "uuml" => "ü",
        "yacute" => "ý",
        "yuml" => "ÿ",
        "Agrave" => "À",
        "Aacute" => "Á",
        "Acirc" => "Â",
        "Atilde" => "Ã",
        "Auml" => "Ä",
        "Aring" => "Å",
        "Ccedil" => "Ç",
        "Egrave" => "È",
        "Eacute" => "É",
        "Ecirc" => "Ê",
        "Euml" => "Ë",
        "Igrave" => "Ì",
        "Iacute" => "Í",
        "Icirc" => "Î",
        "Iuml" => "Ï",
        "Ntilde" => "Ñ",
        "Ograve" => "Ò",
        "Oacute" => "Ó",
        "Ocirc" => "Ô",
        "Otilde" => "Õ",
        "Ouml" => "Ö",
        "Oslash" => "Ø",
        "Ugrave" => "Ù",
        "Uacute" => "Ú",
        "Ucirc" => "Û",
        "Uuml" => "Ü",
        "Yacute" => "Ý",
        "Yuml" => "Ÿ",
        "larr" => "←",
        "rarr" => "→",
        "le" => "≤",
        "ge" => "≥",
        "ne" => "≠",
        "minus" => "−",
        "frac12" => "½",
        "frac14" => "¼",
        "frac34" => "¾",
        "sup2" => "²",
        "sup3" => "³",
        "micro" => "µ",
        _ => return None,
    })
}

#[derive(Debug, PartialEq)]
pub(crate) enum Tok {
    Text(String),
    Open { name: String, attrs: Vec<(String, String)> },
    Close(String),
}

/// Elements whose content is not markup.
const RAW: [&str; 5] = ["script", "style", "title", "textarea", "xmp"];

/// Case-insensitive search for `needle` (ASCII) in `hay` from byte `from`.
fn find_ci(hay: &str, needle: &str, from: usize) -> Option<usize> {
    let h = hay.as_bytes();
    let n = needle.as_bytes();
    (from..h.len().saturating_sub(n.len()).saturating_add(1)).find(|&i| h.get(i..i + n.len()).is_some_and(|w| w.eq_ignore_ascii_case(n)))
}

/// The tags and text of `s`, leniently: unclosed comments and tags run to the end, a `<` that
/// starts no tag is text.
pub(crate) fn scan(s: &str) -> Vec<Tok> {
    let mut out = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0usize;
    let mut text_start = 0usize;
    let flush_text = |out: &mut Vec<Tok>, from: usize, to: usize| {
        if to > from
            && let Some(t) = s.get(from..to)
        {
            out.push(Tok::Text(decode_entities(t)));
        }
    };
    while i < bytes.len() && out.len() < MAX_TAGS {
        if bytes.get(i) != Some(&b'<') {
            i += 1;
            continue;
        }
        let next = bytes.get(i + 1).copied().unwrap_or(0);
        if s.get(i..).is_some_and(|r| r.starts_with("<!--")) {
            flush_text(&mut out, text_start, i);
            i = s.get(i + 4..).and_then(|r| r.find("-->")).map_or(bytes.len(), |e| i + 4 + e + 3);
            text_start = i;
            continue;
        }
        if next == b'!' || next == b'?' {
            flush_text(&mut out, text_start, i);
            i = s.get(i..).and_then(|r| r.find('>')).map_or(bytes.len(), |e| i + e + 1);
            text_start = i;
            continue;
        }
        let closing = next == b'/';
        let name_start = if closing { i + 2 } else { i + 1 };
        let name_len =
            bytes.get(name_start..).map_or(0, |r| r.iter().take_while(|b| b.is_ascii_alphanumeric() || **b == b'-' || **b == b':').count());
        if name_len == 0 || !bytes.get(name_start).is_some_and(u8::is_ascii_alphabetic) {
            // Not a tag: a literal `<`.
            i += 1;
            continue;
        }
        flush_text(&mut out, text_start, i);
        let name = s.get(name_start..name_start + name_len).unwrap_or_default().to_ascii_lowercase();
        // The end of the tag: the first `>` outside quotes.
        let mut j = name_start + name_len;
        let mut quote: Option<u8> = None;
        while let Some(&b) = bytes.get(j) {
            match quote {
                Some(q) if b == q => quote = None,
                Some(_) => {}
                None if b == b'"' || b == b'\'' => quote = Some(b),
                None if b == b'>' => break,
                None => {}
            }
            j += 1;
        }
        let inner = s.get(name_start + name_len..j.min(bytes.len())).unwrap_or_default();
        i = (j + 1).min(bytes.len());
        text_start = i;
        if closing {
            out.push(Tok::Close(name));
            continue;
        }
        let attrs = attributes(inner);
        if RAW.contains(&name.as_str()) {
            let end = find_ci(s, &format!("</{name}"), i).unwrap_or(bytes.len());
            if name == "textarea" || name == "xmp" {
                out.push(Tok::Open { name: name.clone(), attrs });
                flush_text(&mut out, i, end);
                out.push(Tok::Close(name));
            }
            i = s.get(end..).and_then(|r| r.find('>')).map_or(bytes.len(), |e| end + e + 1);
            text_start = i;
            continue;
        }
        out.push(Tok::Open { name, attrs });
    }
    flush_text(&mut out, text_start, bytes.len());
    out
}

/// `name="value"`, `name='value'`, `name=value` and bare `name` attributes.
fn attributes(s: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut chars = s.char_indices().peekable();
    while let Some((start, c)) = chars.next() {
        if c.is_whitespace() || c == '/' {
            continue;
        }
        let mut end = start + c.len_utf8();
        while let Some(&(i, c)) = chars.peek() {
            if c.is_whitespace() || c == '=' || c == '/' {
                break;
            }
            end = i + c.len_utf8();
            chars.next();
        }
        let name = s.get(start..end).unwrap_or_default().to_ascii_lowercase();
        while chars.peek().is_some_and(|(_, c)| c.is_whitespace()) {
            chars.next();
        }
        let mut value = String::new();
        if chars.peek().is_some_and(|(_, c)| *c == '=') {
            chars.next();
            while chars.peek().is_some_and(|(_, c)| c.is_whitespace()) {
                chars.next();
            }
            match chars.peek().map(|(_, c)| *c) {
                Some(q @ ('"' | '\'')) => {
                    chars.next();
                    for (_, c) in chars.by_ref() {
                        if c == q {
                            break;
                        }
                        value.push(c);
                    }
                }
                _ => {
                    while let Some(&(_, c)) = chars.peek() {
                        if c.is_whitespace() {
                            break;
                        }
                        value.push(c);
                        chars.next();
                    }
                }
            }
        }
        if out.len() < 64 {
            out.push((name, decode_entities(&value)));
        }
    }
    out
}

#[derive(Default)]
struct TableBuild {
    rows: Vec<Row>,
    row: Option<Row>,
    cell: Option<(Cell, bool)>,
    head: bool,
}

#[derive(Default)]
struct Builder {
    source: Source,
    runs: Vec<Run>,
    kind: Option<Kind>,
    align: Align,
    bold: usize,
    italic: usize,
    underline: usize,
    pre: usize,
    quote: u8,
    lists: Vec<(bool, u32)>,
    tables: Vec<TableBuild>,
    images: bool,
}

impl Builder {
    fn style(&self) -> Style {
        Style { bold: self.bold > 0, italic: self.italic > 0, underline: self.underline > 0 }
    }

    fn push(&mut self, text: &str) {
        let style = self.style();
        let mut out = String::new();
        if self.pre > 0 {
            out.push_str(text);
        } else {
            // Whitespace collapses to one space, and none at the start of a line.
            let mut space = self.runs.last().is_none_or(|r| r.text.ends_with([' ', '\n']));
            for c in text.chars() {
                if c.is_whitespace() && c != '\u{a0}' {
                    if !space {
                        out.push(' ');
                        space = true;
                    }
                } else {
                    out.push(c);
                    space = false;
                }
            }
        }
        if out.is_empty() {
            return;
        }
        match self.runs.last_mut() {
            Some(r) if r.style == style => r.text.push_str(&out),
            _ => self.runs.push(Run::styled(out, style)),
        }
    }

    /// End the current paragraph, if it has text.
    fn flush(&mut self) {
        let mut runs = std::mem::take(&mut self.runs);
        let kind = self.kind.take();
        if let Some(last) = runs.last_mut() {
            let trimmed = last.text.trim_end_matches([' ', '\n']).len();
            last.text.truncate(trimmed);
        }
        if runs.iter().all(|r| r.text.trim().is_empty()) {
            return;
        }
        if let Some(t) = self.tables.last_mut()
            && let Some((cell, _)) = t.cell.as_mut()
        {
            if let Some(Kind::ListItem { marker, .. }) = &kind {
                runs.insert(0, Run::plain(format!("{marker} ")));
            }
            cell.paras.push(runs);
            return;
        }
        let mut p = Para::new(kind.unwrap_or(Kind::Body), runs);
        p.align = self.align;
        p.indent = self.quote;
        self.source.blocks.push(Block::Para(p));
    }

    fn end_cell(&mut self) {
        self.flush();
        if let Some(t) = self.tables.last_mut()
            && let Some((mut cell, header)) = t.cell.take()
        {
            // Header cells are bold, also where their row isn't a header row.
            if header {
                cell.paras.iter_mut().flatten().for_each(|r| r.style.bold = true);
            }
            let row = t.row.get_or_insert_with(Row::default);
            row.header = if row.cells.is_empty() { header } else { row.header && header };
            row.cells.push(cell);
        }
    }

    fn end_row(&mut self) {
        self.end_cell();
        if let Some(t) = self.tables.last_mut()
            && let Some(mut row) = t.row.take()
        {
            row.header |= t.head;
            if !row.cells.is_empty() {
                t.rows.push(row);
            }
        }
    }

    fn end_table(&mut self) {
        self.end_row();
        let Some(t) = self.tables.pop() else { return };
        if t.rows.is_empty() {
            return;
        }
        // Header rows only count at the top.
        let mut rows = t.rows;
        let lead = rows.iter().take_while(|r| r.header).count();
        for r in rows.iter_mut().skip(lead) {
            if r.header {
                r.header = false;
                for c in &mut r.cells {
                    for p in &mut c.paras {
                        for run in p.iter_mut() {
                            run.style.bold = true;
                        }
                    }
                }
            }
        }
        if let Some(outer) = self.tables.last_mut()
            && let Some((cell, _)) = outer.cell.as_mut()
        {
            // A table inside a cell: one line per row, cells separated by " | ".
            for row in rows {
                let mut line = Vec::new();
                for (i, c) in row.cells.into_iter().enumerate() {
                    if i > 0 {
                        line.push(Run::plain(" | "));
                    }
                    for (j, p) in c.paras.into_iter().enumerate() {
                        if j > 0 {
                            line.push(Run::plain(" "));
                        }
                        line.extend(p);
                    }
                }
                cell.paras.push(line);
            }
            return;
        }
        self.source.blocks.push(Block::Table(Table { rows, widths: None, size: 10.0, fill: false, split_columns: false }));
    }

    fn open(&mut self, name: &str, attrs: &[(String, String)]) {
        let get = |k: &str| attrs.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
        match name {
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                self.flush();
                let level = name.get(1..).and_then(|n| n.parse::<u8>().ok()).unwrap_or(1);
                self.kind = Some(Kind::Heading(level));
                self.align = align(get("align"), get("style"));
            }
            "p" | "div" | "section" | "article" | "header" | "footer" | "main" | "nav" | "aside" | "figure" | "figcaption" | "address" | "dd"
            | "dt" | "dl" | "form" | "fieldset" | "legend" | "caption" | "summary" | "details" => {
                self.flush();
                self.align = align(get("align"), get("style"));
            }
            "center" => {
                self.flush();
                self.align = Align::Center;
            }
            "blockquote" => {
                self.flush();
                self.quote = self.quote.saturating_add(1).min(6);
            }
            "br" => self.push_raw("\n"),
            "hr" => self.flush(),
            "b" | "strong" => self.bold += 1,
            "i" | "em" | "cite" | "var" | "dfn" => self.italic += 1,
            "u" | "ins" => self.underline += 1,
            "pre" | "listing" | "plaintext" => {
                self.flush();
                self.pre += 1;
            }
            "ul" | "ol" | "menu" => {
                self.flush();
                if self.lists.len() < MAX_NESTING {
                    let start = get("start").and_then(|s| s.trim().parse::<u32>().ok()).unwrap_or(1);
                    self.lists.push((name == "ol", start.saturating_sub(1)));
                }
            }
            "li" => {
                self.flush();
                let level = u8::try_from(self.lists.len().saturating_sub(1)).unwrap_or(0);
                let marker = match self.lists.last_mut() {
                    Some((true, n)) => {
                        *n = n.saturating_add(1);
                        format!("{n}.")
                    }
                    Some((false, _)) if level % 2 == 1 => "\u{2013}".to_string(),
                    _ => "\u{2022}".to_string(),
                };
                self.kind = Some(Kind::ListItem { level, marker });
            }
            "table" => {
                self.flush();
                if self.tables.len() < MAX_NESTING {
                    self.tables.push(TableBuild::default());
                }
            }
            "thead" => {
                if let Some(t) = self.tables.last_mut() {
                    t.head = true;
                }
            }
            "tbody" | "tfoot" => {
                self.end_row();
                if let Some(t) = self.tables.last_mut() {
                    t.head = false;
                }
            }
            "tr" => {
                self.end_row();
                if let Some(t) = self.tables.last_mut() {
                    t.row = Some(Row::default());
                }
            }
            "td" | "th" => {
                self.end_cell();
                if let Some(t) = self.tables.last_mut() {
                    let span = get("colspan").and_then(|s| s.trim().parse::<usize>().ok()).unwrap_or(1).clamp(1, 64);
                    t.cell = Some((Cell { paras: Vec::new(), span }, name == "th"));
                }
            }
            "img" => self.images = true,
            _ => {}
        }
    }

    fn close(&mut self, name: &str) {
        match name {
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "p" | "div" | "section" | "article" | "header" | "footer" | "main" | "nav" | "aside"
            | "figure" | "figcaption" | "address" | "dd" | "dt" | "dl" | "form" | "fieldset" | "legend" | "caption" | "summary" | "details"
            | "li" => {
                self.flush();
                self.align = Align::Left;
            }
            "center" => {
                self.flush();
                self.align = Align::Left;
            }
            "blockquote" => {
                self.flush();
                self.quote = self.quote.saturating_sub(1);
            }
            "b" | "strong" => self.bold = self.bold.saturating_sub(1),
            "i" | "em" | "cite" | "var" | "dfn" => self.italic = self.italic.saturating_sub(1),
            "u" | "ins" => self.underline = self.underline.saturating_sub(1),
            "pre" | "listing" | "plaintext" => {
                self.flush();
                self.pre = self.pre.saturating_sub(1);
            }
            "ul" | "ol" | "menu" => {
                self.flush();
                self.lists.pop();
            }
            "table" => self.end_table(),
            "tr" => self.end_row(),
            "td" | "th" => self.end_cell(),
            "thead" => {
                self.end_row();
                if let Some(t) = self.tables.last_mut() {
                    t.head = false;
                }
            }
            _ => {}
        }
    }

    /// Text that is kept as is (a line break), whatever the whitespace rules.
    fn push_raw(&mut self, text: &str) {
        let style = self.style();
        match self.runs.last_mut() {
            Some(r) if r.style == style => r.text.push_str(text),
            _ => self.runs.push(Run::styled(text, style)),
        }
    }
}

fn align(attr: Option<&str>, style: Option<&str>) -> Align {
    let style = style.unwrap_or_default().to_ascii_lowercase().replace(' ', "");
    match attr.map(str::to_ascii_lowercase).as_deref() {
        Some("center") => Align::Center,
        Some("right") => Align::Right,
        _ if style.contains("text-align:center") => Align::Center,
        _ if style.contains("text-align:right") => Align::Right,
        _ => Align::Left,
    }
}

pub(crate) fn read(text: &str) -> Source {
    let mut b = Builder::default();
    for tok in scan(text) {
        match tok {
            Tok::Text(t) => b.push(&t),
            Tok::Open { name, attrs } => b.open(&name, &attrs),
            Tok::Close(name) => b.close(&name),
        }
    }
    while !b.tables.is_empty() {
        b.end_table();
    }
    b.flush();
    if b.images {
        b.source.notes.push("images in HTML files are not included".into());
    }
    b.source
}
