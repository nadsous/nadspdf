//! Word documents (Office Open XML, `.docx`): `word/document.xml` with its styles, numbering
//! and image relationships.

use std::collections::HashMap;

use roxmltree::Node;

use crate::ConvertError;
use crate::model::{Align, Block, Cell, Kind, Para, Row, Run, Source, Style, Table};
use crate::xml::{MAX_DEPTH, attr, child, children, descendant, is, parse};
use crate::zipfile::{Archive, resolve};

/// Most images one document embeds; later ones are left out with a note.
const MAX_IMAGES: usize = 500;
/// Longest `basedOn` chain followed.
const MAX_STYLE_CHAIN: usize = 16;
/// EMU per point (DrawingML sizes).
const EMU_PER_PT: f64 = 12_700.0;

/// A `w:b`-style on/off property: present means on unless its value says off.
fn on_off(n: Node) -> bool {
    !matches!(attr(n, "val"), Some("0" | "false" | "off" | "none"))
}

/// Run properties as set (or not) at one level: paragraph style, character style, run.
#[derive(Clone, Copy, Debug, Default)]
struct RunProps {
    bold: Option<bool>,
    italic: Option<bool>,
    underline: Option<bool>,
    hidden: Option<bool>,
}

impl RunProps {
    fn read(rpr: Option<Node>) -> Self {
        let Some(rpr) = rpr else { return Self::default() };
        let get = |name: &str| child(rpr, name).map(on_off);
        Self { bold: get("b"), italic: get("i"), underline: child(rpr, "u").map(on_off), hidden: get("vanish") }
    }

    /// `self` where set, else `under`.
    fn over(self, under: Self) -> Self {
        Self {
            bold: self.bold.or(under.bold),
            italic: self.italic.or(under.italic),
            underline: self.underline.or(under.underline),
            hidden: self.hidden.or(under.hidden),
        }
    }

    fn style(self) -> Style {
        Style { bold: self.bold.unwrap_or(false), italic: self.italic.unwrap_or(false), underline: self.underline.unwrap_or(false) }
    }
}

#[derive(Debug, Default)]
struct StyleDef {
    name: String,
    based_on: Option<String>,
    outline: Option<u8>,
    run: RunProps,
    num: Option<(String, u8)>,
}

#[derive(Debug, Default)]
struct Styles {
    map: HashMap<String, StyleDef>,
    default_para: Option<String>,
}

fn num_pr(n: Node) -> Option<(String, u8)> {
    let id = child(n, "numId").and_then(|x| attr(x, "val"))?.to_string();
    let level = child(n, "ilvl").and_then(|x| attr(x, "val")).and_then(|v| v.parse::<u8>().ok()).unwrap_or(0).min(8);
    Some((id, level))
}

impl Styles {
    fn read(text: &str) -> Result<Self, ConvertError> {
        let xml = parse(text, "word/styles.xml")?;
        let mut styles = Styles::default();
        for s in children(xml.root_element(), "style") {
            let Some(id) = attr(s, "styleId") else { continue };
            let ppr = child(s, "pPr");
            let def = StyleDef {
                name: child(s, "name").and_then(|n| attr(n, "val")).unwrap_or_default().to_ascii_lowercase(),
                based_on: child(s, "basedOn").and_then(|n| attr(n, "val")).map(str::to_string),
                outline: ppr.and_then(|p| child(p, "outlineLvl")).and_then(|n| attr(n, "val")).and_then(|v| v.parse().ok()),
                run: RunProps::read(child(s, "rPr")),
                num: ppr.and_then(|p| child(p, "numPr")).and_then(num_pr),
            };
            if attr(s, "type") == Some("paragraph") && attr(s, "default").is_some_and(|v| matches!(v, "1" | "true" | "on")) {
                styles.default_para = Some(id.to_string());
            }
            styles.map.insert(id.to_string(), def);
        }
        Ok(styles)
    }

    /// The style `id` and the styles it is based on, nearest first.
    fn chain(&self, id: Option<&str>) -> Vec<&StyleDef> {
        let mut out = Vec::new();
        let mut cur = id.map(str::to_string).or_else(|| self.default_para.clone());
        while let Some(id) = cur {
            if out.len() >= MAX_STYLE_CHAIN {
                break;
            }
            let Some(def) = self.map.get(&id) else { break };
            out.push(def);
            cur = def.based_on.clone();
        }
        out
    }

    /// The heading a paragraph style makes: by its built-in name (`heading 2`, `Title`), which
    /// Word keeps in English whatever the interface language, else its outline level, else
    /// common localized style ids (`Titre1`).
    fn heading(&self, id: Option<&str>) -> Option<Kind> {
        for def in self.chain(id) {
            if let Some(k) = heading_name(&def.name) {
                return Some(k);
            }
            if let Some(l) = def.outline.filter(|l| *l < 9) {
                return Some(Kind::Heading(l.saturating_add(1).min(6)));
            }
        }
        id.and_then(heading_name)
    }

    fn run(&self, id: Option<&str>) -> RunProps {
        self.chain(id).iter().fold(RunProps::default(), |acc, d| acc.over(d.run))
    }

    fn numbering(&self, id: Option<&str>) -> Option<(String, u8)> {
        self.chain(id).iter().find_map(|d| d.num.clone())
    }
}

/// `heading 1`, `Heading1`, `Titre1`, `Title`, `Titre`, … → the heading kind.
fn heading_name(name: &str) -> Option<Kind> {
    let flat: String = name.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_lowercase();
    if matches!(flat.as_str(), "title" | "titre" | "titel" | "título" | "titolo") {
        return Some(Kind::Title);
    }
    if matches!(flat.as_str(), "subtitle" | "sous-titre" | "soustitre" | "untertitel") {
        return Some(Kind::Heading(2));
    }
    for prefix in ["heading", "titre", "überschrift", "berschrift", "título", "titulo", "titolo", "kop"] {
        if let Some(rest) = flat.strip_prefix(prefix)
            && let Ok(n) = rest.parse::<u8>()
            && (1..=9).contains(&n)
        {
            return Some(Kind::Heading(n.min(6)));
        }
    }
    None
}

#[derive(Debug, Clone)]
struct Level {
    fmt: String,
    text: String,
    start: u32,
}

#[derive(Debug, Default)]
struct Numbering {
    nums: HashMap<String, String>,
    abstracts: HashMap<String, HashMap<u8, Level>>,
}

impl Numbering {
    fn read(text: &str) -> Result<Self, ConvertError> {
        let xml = parse(text, "word/numbering.xml")?;
        let mut n = Numbering::default();
        for a in children(xml.root_element(), "abstractNum") {
            let Some(id) = attr(a, "abstractNumId") else { continue };
            let mut levels = HashMap::new();
            for l in children(a, "lvl") {
                let Some(ilvl) = attr(l, "ilvl").and_then(|v| v.parse::<u8>().ok()).filter(|l| *l < 9) else { continue };
                levels.insert(
                    ilvl,
                    Level {
                        fmt: child(l, "numFmt").and_then(|x| attr(x, "val")).unwrap_or("decimal").to_string(),
                        text: child(l, "lvlText").and_then(|x| attr(x, "val")).unwrap_or_default().to_string(),
                        start: child(l, "start").and_then(|x| attr(x, "val")).and_then(|v| v.parse().ok()).unwrap_or(1).min(1_000_000),
                    },
                );
            }
            n.abstracts.insert(id.to_string(), levels);
        }
        for num in children(xml.root_element(), "num") {
            if let (Some(id), Some(a)) = (attr(num, "numId"), child(num, "abstractNumId").and_then(|x| attr(x, "val"))) {
                n.nums.insert(id.to_string(), a.to_string());
            }
        }
        Ok(n)
    }

    fn levels(&self, num: &str) -> Option<&HashMap<u8, Level>> {
        self.abstracts.get(self.nums.get(num)?)
    }
}

fn roman(mut n: u32) -> String {
    const TABLE: [(u32, &str); 13] = [
        (1000, "m"),
        (900, "cm"),
        (500, "d"),
        (400, "cd"),
        (100, "c"),
        (90, "xc"),
        (50, "l"),
        (40, "xl"),
        (10, "x"),
        (9, "ix"),
        (5, "v"),
        (4, "iv"),
        (1, "i"),
    ];
    let mut out = String::new();
    for (v, s) in TABLE {
        while n >= v && out.len() < 64 {
            out.push_str(s);
            n -= v;
        }
    }
    out
}

fn letters(n: u32) -> String {
    // a … z, aa … zz, as Word counts.
    if n == 0 {
        return String::new();
    }
    let i = (n - 1) % 26;
    let times = ((n - 1) / 26 + 1).min(10) as usize;
    char::from_u32(u32::from(b'a') + i).map(|c| c.to_string().repeat(times)).unwrap_or_default()
}

fn format_number(n: u32, fmt: &str) -> String {
    match fmt {
        "lowerLetter" => letters(n),
        "upperLetter" => letters(n).to_uppercase(),
        "lowerRoman" => roman(n),
        "upperRoman" => roman(n).to_uppercase(),
        "decimalZero" => format!("{n:02}"),
        "none" => String::new(),
        _ => n.to_string(),
    }
}

struct Reader<'z> {
    zip: Archive<'z>,
    styles: Styles,
    numbering: Numbering,
    rels: HashMap<String, (String, bool)>,
    counters: HashMap<String, [u32; 9]>,
    images: usize,
    source: Source,
    notes: Vec<String>,
}

/// What a paragraph's content yields, in order.
enum Seg {
    Run(Run),
    PageBreak,
    Image(Block),
}

impl Reader<'_> {
    /// The list marker for item `level` of list `num`, advancing its counters.
    fn marker(&mut self, num: &str, level: u8) -> Option<String> {
        let levels = self.numbering.levels(num)?.clone();
        let lvl = levels.get(&level)?;
        if lvl.fmt == "bullet" {
            return Some(if level.is_multiple_of(2) { "\u{2022}" } else { "\u{2013}" }.to_string());
        }
        let start = |k: usize| levels.get(&u8::try_from(k).unwrap_or(0)).map_or(1, |l| l.start);
        let counters = self.counters.entry(num.to_string()).or_insert([0; 9]);
        let li = usize::from(level.min(8));
        // An item restarts the levels below it, and counts at its own (from the level's start).
        for c in counters.iter_mut().skip(li + 1) {
            *c = 0;
        }
        if let Some(c) = counters.get_mut(li) {
            *c = if *c == 0 { start(li) } else { c.saturating_add(1) };
        }
        let mut text = if lvl.text.is_empty() { format!("%{}.", li + 1) } else { lvl.text.clone() };
        for k in (0..9usize).rev() {
            let pat = format!("%{}", k + 1);
            if text.contains(&pat) {
                let fmt = levels.get(&u8::try_from(k).unwrap_or(0)).map_or("decimal", |l| l.fmt.as_str());
                let n = counters.get(k).copied().filter(|n| *n > 0).unwrap_or_else(|| start(k));
                text = text.replace(&pat, &format_number(n, fmt));
            }
        }
        Some(text)
    }

    fn image(&mut self, rid: &str, size: Option<(f64, f64)>) -> Option<Block> {
        let (target, external) = self.rels.get(rid)?.clone();
        if external {
            self.note("linked (not embedded) images are left out");
            return None;
        }
        if self.images >= MAX_IMAGES {
            self.note("images beyond the first 500 are left out");
            return None;
        }
        let path = resolve("word/", &target);
        match self.zip.read(&path) {
            Ok(Some(bytes)) => {
                self.images += 1;
                let name = path.rsplit('/').next().unwrap_or(&path).to_string();
                Some(Block::Image { name, bytes, size })
            }
            _ => {
                self.note("some images are missing from the file and are left out");
                None
            }
        }
    }

    fn note(&mut self, n: &str) {
        if !self.notes.iter().any(|x| x == n) {
            self.notes.push(n.to_string());
        }
    }

    /// The content of a run or a container of runs (hyperlinks, insertions, content controls).
    fn inline(&mut self, n: Node, base: RunProps, out: &mut Vec<Seg>, depth: usize) {
        if depth > MAX_DEPTH {
            return;
        }
        for c in n.children().filter(Node::is_element) {
            match c.tag_name().name() {
                "r" => self.run(c, base, out),
                "hyperlink" | "ins" | "smartTag" | "fldSimple" | "customXml" | "moveTo" | "sdtContent" | "dir" | "bdo" => {
                    self.inline(c, base, out, depth + 1)
                }
                "sdt" => {
                    if let Some(content) = child(c, "sdtContent") {
                        self.inline(content, base, out, depth + 1);
                    }
                }
                // Office Math: its text, plainly.
                "oMath" | "oMathPara" => {
                    let text: String = c.descendants().filter(|d| is(*d, "t")).filter_map(|d| d.text()).collect();
                    out.push(Seg::Run(Run::styled(text, base.style())));
                }
                _ => {}
            }
        }
    }

    fn run(&mut self, r: Node, base: RunProps, out: &mut Vec<Seg>) {
        let rpr = child(r, "rPr");
        let char_style = rpr.and_then(|p| child(p, "rStyle")).and_then(|s| attr(s, "val"));
        let props = RunProps::read(rpr).over(self.styles.run(char_style).over(base));
        if props.hidden == Some(true) {
            return;
        }
        let style = props.style();
        let push = |out: &mut Vec<Seg>, s: &str| match out.last_mut() {
            Some(Seg::Run(last)) if last.style == style => last.text.push_str(s),
            _ => out.push(Seg::Run(Run::styled(s, style))),
        };
        for c in r.children().filter(Node::is_element) {
            match c.tag_name().name() {
                "t" => push(out, c.text().unwrap_or_default()),
                "tab" | "ptab" => push(out, "\t"),
                "br" => match attr(c, "type") {
                    Some("page") => out.push(Seg::PageBreak),
                    _ => push(out, "\n"),
                },
                "cr" => push(out, "\n"),
                "noBreakHyphen" => push(out, "\u{2011}"),
                "sym" => {
                    let ch = attr(c, "char").and_then(|h| u32::from_str_radix(h, 16).ok()).and_then(char::from_u32);
                    if let Some(ch) = ch {
                        push(out, ch.encode_utf8(&mut [0; 4]));
                    }
                }
                "drawing" => {
                    let size = descendant(c, "extent").and_then(|e| {
                        let cx = attr(e, "cx")?.parse::<f64>().ok()?;
                        let cy = attr(e, "cy")?.parse::<f64>().ok()?;
                        Some((cx / EMU_PER_PT, cy / EMU_PER_PT))
                    });
                    if let Some(rid) = descendant(c, "blip").and_then(|b| attr(b, "embed")).map(str::to_string)
                        && let Some(img) = self.image(&rid, size)
                    {
                        out.push(Seg::Image(img));
                    }
                }
                "pict" | "object" => {
                    if let Some(rid) = descendant(c, "imagedata").and_then(|b| attr(b, "id")).map(str::to_string)
                        && let Some(img) = self.image(&rid, None)
                    {
                        out.push(Seg::Image(img));
                    }
                }
                _ => {}
            }
        }
    }

    /// A `w:p`: its paragraph, and the page breaks and images inside it, as blocks.
    fn paragraph(&mut self, p: Node) -> Vec<Block> {
        let ppr = child(p, "pPr");
        let style_id = ppr.and_then(|x| child(x, "pStyle")).and_then(|s| attr(s, "val"));
        let mut kind = self.styles.heading(style_id).unwrap_or(Kind::Body);
        if kind == Kind::Body
            && let Some(l) = ppr.and_then(|x| child(x, "outlineLvl")).and_then(|n| attr(n, "val")).and_then(|v| v.parse::<u8>().ok())
            && l < 9
        {
            kind = Kind::Heading(l.saturating_add(1).min(6));
        }
        let num = match ppr.and_then(|x| child(x, "numPr")) {
            Some(n) => num_pr(n).or_else(|| self.styles.numbering(style_id)),
            None => self.styles.numbering(style_id),
        }
        .filter(|(id, _)| id != "0");
        let marker = num.and_then(|(id, level)| self.marker(&id, level).map(|m| (m, level)));
        let align = match ppr.and_then(|x| child(x, "jc")).and_then(|j| attr(j, "val")) {
            Some("center") => Align::Center,
            Some("right" | "end") => Align::Right,
            _ => Align::Left,
        };
        let base = self.styles.run(style_id);
        let mut segs = Vec::new();
        self.inline(p, base, &mut segs, 0);
        let mut blocks = Vec::new();
        let mut runs: Vec<Run> = Vec::new();
        let mut first = true;
        let make = |runs: Vec<Run>, first: bool| {
            let mut runs = runs;
            let kind = match (&kind, &marker) {
                (Kind::Body, Some((m, level))) if first => Kind::ListItem { level: *level, marker: m.clone() },
                (Kind::Body, Some((_, level))) => Kind::ListItem { level: *level, marker: String::new() },
                (k, Some((m, _))) if first && !m.is_empty() => {
                    runs.insert(0, Run::styled(format!("{m} "), runs.first().map(|r| r.style).unwrap_or_default()));
                    k.clone()
                }
                (k, _) => k.clone(),
            };
            Block::Para(Para { kind, runs, align, indent: 0 })
        };
        let mut had_image = false;
        for seg in segs {
            match seg {
                Seg::Run(r) => runs.push(r),
                Seg::PageBreak => {
                    if runs.iter().any(|r| !r.text.is_empty()) {
                        blocks.push(make(std::mem::take(&mut runs), first));
                        first = false;
                    }
                    blocks.push(Block::PageBreak);
                }
                Seg::Image(img) => {
                    if runs.iter().any(|r| !r.text.trim().is_empty()) {
                        blocks.push(make(std::mem::take(&mut runs), first));
                        first = false;
                    }
                    runs.clear();
                    blocks.push(img);
                    had_image = true;
                }
            }
        }
        if !(had_image && runs.iter().all(|r| r.text.trim().is_empty())) && (first || runs.iter().any(|r| !r.text.is_empty())) {
            blocks.push(make(runs, first));
        }
        // A section break that starts a new page.
        if let Some(sect) = ppr.and_then(|x| child(x, "sectPr"))
            && !matches!(child(sect, "type").and_then(|t| attr(t, "val")), Some("continuous"))
        {
            blocks.push(Block::PageBreak);
        }
        blocks
    }

    /// A cell's or nested table's content as paragraphs of runs.
    fn cell_paras(&mut self, n: Node, depth: usize, out: &mut Vec<Vec<Run>>) {
        if depth > MAX_DEPTH {
            return;
        }
        for c in n.children().filter(Node::is_element) {
            match c.tag_name().name() {
                "p" => {
                    for b in self.paragraph(c) {
                        match b {
                            Block::Para(p) => {
                                let mut runs = p.runs;
                                if let Kind::ListItem { marker, .. } = p.kind
                                    && !marker.is_empty()
                                {
                                    runs.insert(0, Run::plain(format!("{marker} ")));
                                }
                                out.push(runs);
                            }
                            Block::Image { .. } => {
                                self.note("images inside tables are left out");
                            }
                            _ => {}
                        }
                    }
                }
                // A nested table: each row on its own line, cells separated by " | ".
                "tbl" => {
                    for tr in children(c, "tr") {
                        let mut line: Vec<Run> = Vec::new();
                        for (i, tc) in children(tr, "tc").enumerate() {
                            let mut paras = Vec::new();
                            self.cell_paras(tc, depth + 1, &mut paras);
                            if i > 0 {
                                line.push(Run::plain(" | "));
                            }
                            for (j, p) in paras.into_iter().enumerate() {
                                if j > 0 {
                                    line.push(Run::plain(" "));
                                }
                                line.extend(p);
                            }
                        }
                        out.push(line);
                    }
                }
                "sdt" => {
                    if let Some(content) = child(c, "sdtContent") {
                        self.cell_paras(content, depth + 1, out);
                    }
                }
                "customXml" => self.cell_paras(c, depth + 1, out),
                _ => {}
            }
        }
    }

    fn table(&mut self, tbl: Node, depth: usize) -> Table {
        let grid: Vec<f64> = child(tbl, "tblGrid")
            .map(|g| children(g, "gridCol").map(|c| attr(c, "w").and_then(|w| w.parse::<f64>().ok()).unwrap_or(0.0)).collect())
            .unwrap_or_default();
        let mut rows = Vec::new();
        for tr in children(tbl, "tr") {
            let header = child(tr, "trPr").and_then(|p| child(p, "tblHeader")).is_some_and(on_off);
            let mut cells = Vec::new();
            for tc in children(tr, "tc") {
                let tcpr = child(tc, "tcPr");
                let span = tcpr
                    .and_then(|p| child(p, "gridSpan"))
                    .and_then(|s| attr(s, "val"))
                    .and_then(|v| v.parse::<usize>().ok())
                    .unwrap_or(1)
                    .clamp(1, 64);
                // A vertically merged continuation cell shows nothing of its own.
                let continued = tcpr.and_then(|p| child(p, "vMerge")).is_some_and(|m| attr(m, "val").is_none_or(|v| v == "continue"));
                let mut paras = Vec::new();
                if !continued {
                    self.cell_paras(tc, depth + 1, &mut paras);
                }
                cells.push(Cell { paras, span });
            }
            rows.push(Row { cells, header });
        }
        // Only leading header rows repeat; a row marked header later on is just bold.
        let table = Table { rows, widths: None, size: 10.0, fill: true, split_columns: false };
        let columns = table.columns();
        let widths = (grid.len() == columns && grid.iter().all(|w| *w > 0.0)).then_some(grid);
        Table { widths, ..table }
    }

    fn body(&mut self, n: Node, depth: usize) {
        if depth > MAX_DEPTH {
            return;
        }
        for c in n.children().filter(Node::is_element) {
            match c.tag_name().name() {
                "p" => {
                    let blocks = self.paragraph(c);
                    self.source.blocks.extend(blocks);
                }
                "tbl" => {
                    let t = self.table(c, depth);
                    self.source.blocks.push(Block::Table(t));
                }
                "sdt" => {
                    if let Some(content) = child(c, "sdtContent") {
                        self.body(content, depth + 1);
                    }
                }
                "customXml" | "ins" | "moveTo" => self.body(c, depth + 1),
                _ => {}
            }
        }
    }
}

/// Relationships of a part: id → (target, external).
fn relationships(text: &str) -> Result<HashMap<String, (String, bool)>, ConvertError> {
    let xml = parse(text, "word/_rels/document.xml.rels")?;
    Ok(children(xml.root_element(), "Relationship")
        .filter_map(|r| Some((attr(r, "Id")?.to_string(), (attr(r, "Target")?.to_string(), attr(r, "TargetMode") == Some("External")))))
        .collect())
}

pub(crate) fn read(bytes: &[u8]) -> Result<Source, ConvertError> {
    let mut zip = Archive::open(bytes, "Word (.docx)")?;
    let text = zip
        .text("word/document.xml")?
        .ok_or_else(|| ConvertError::Malformed("this .docx file has no word/document.xml: it isn't a Word document".into()))?;
    let styles = match zip.text("word/styles.xml")? {
        Some(t) => Styles::read(&t)?,
        None => Styles::default(),
    };
    let numbering = match zip.text("word/numbering.xml")? {
        Some(t) => Numbering::read(&t)?,
        None => Numbering::default(),
    };
    let rels = match zip.text("word/_rels/document.xml.rels")? {
        Some(t) => relationships(&t)?,
        None => HashMap::new(),
    };
    let xml = parse(&text, "word/document.xml")?;
    let body = descendant(xml.root_element(), "body").ok_or_else(|| ConvertError::Malformed("word/document.xml has no body".into()))?;
    let mut reader = Reader { zip, styles, numbering, rels, counters: HashMap::new(), images: 0, source: Source::default(), notes: Vec::new() };
    // The page orientation of the (last) section.
    if let Some(size) = child(body, "sectPr").and_then(|s| child(s, "pgSz")) {
        let dim = |k: &str| attr(size, k).and_then(|v| v.parse::<f64>().ok());
        let landscape = attr(size, "orient") == Some("landscape") || matches!((dim("w"), dim("h")), (Some(w), Some(h)) if w > h);
        if landscape {
            reader.source.blocks.push(Block::Section { landscape: true });
        }
    }
    reader.body(body, 0);
    let mut source = reader.source;
    source.notes = reader.notes;
    // Trailing empty paragraphs would only add blank space at the end.
    while matches!(source.blocks.last(), Some(Block::Para(p)) if p.is_blank() && !matches!(p.kind, Kind::ListItem { .. })) {
        source.blocks.pop();
    }
    Ok(source)
}
