//! Word's layout mode: a page whose artwork shows (a coloured background, shapes, pictures) keeps
//! its look. A picture of the page without its text fills the page behind everything, and every
//! paragraph is an editable text box where the PDF draws it: one Word paragraph per visual row,
//! each run in the PDF's font, size and colour.

use crate::{Block, Image, Page, esc, hex, layout_number};

/// One styled piece of a row (a line of the PDF in one font, size and colour).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Run {
    pub text: String,
    /// [x0, y0, x1, y1] on the displayed page in points (y up).
    pub rect: [f64; 4],
    pub size: f64,
    pub bold: bool,
    pub italic: bool,
    pub color: [f64; 3],
    /// The PDF font's `/BaseFont` (`ABCDEF+Montserrat-Bold`); [`family`] makes a Word font of it.
    pub font: String,
}

/// A paragraph with its visual rows, top to bottom; each row's runs in any order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Para {
    /// The paragraph as a flat block (its rotation, and the fallback when it has no rows).
    pub block: Block,
    pub rows: Vec<Vec<Run>>,
}

/// What layout mode needs for one page.
#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    /// The page drawn without its text, the size of the page.
    pub background: Image,
    pub paras: Vec<Para>,
}

/// A Word font name for a PDF `/BaseFont`: the subset tag, style suffix and PostScript endings
/// go, and words are spaced (`ABCDEF+TimesNewRomanPS-BoldMT` → `Times New Roman`). `None` when
/// nothing usable is left.
pub fn family(base_font: &str) -> Option<String> {
    let name = match base_font.split_once('+') {
        Some((tag, rest)) if tag.len() == 6 && tag.bytes().all(|b| b.is_ascii_uppercase()) => rest,
        _ => base_font,
    };
    let name = name.split([',', '-', '_']).next().unwrap_or_default();
    let mut name = name.trim();
    for suffix in ["PSMT", "MT", "PS"] {
        if let Some(stem) = name.strip_suffix(suffix)
            && stem.chars().last().is_some_and(|c| c.is_ascii_lowercase())
        {
            name = stem;
            break;
        }
    }
    let chars: Vec<char> = name.chars().filter(|c| !c.is_control()).take(64).collect();
    let mut out = String::new();
    for (i, &c) in chars.iter().enumerate() {
        let prev = i.checked_sub(1).and_then(|p| chars.get(p)).copied();
        let next = chars.get(i + 1).copied();
        // A space before a capital that starts a word: "OpenSans", and "DMSans" → "DM Sans".
        let starts_word = c.is_ascii_uppercase()
            && (prev.is_some_and(|p| p.is_ascii_lowercase())
                || prev.is_some_and(|p| p.is_ascii_uppercase()) && next.is_some_and(|n| n.is_ascii_lowercase()));
        if starts_word {
            out.push(' ');
        }
        out.push(c);
    }
    let out = out.trim().to_string();
    (out.chars().any(char::is_alphabetic)).then_some(out)
}

/// A font's kind for Word's font table, guessed from its name: its `w:family`, `w:pitch` and
/// the common font Word may use in its place.
fn font_kind(name: &str) -> (&'static str, &'static str, &'static str) {
    let n = name.to_ascii_lowercase();
    let any = |words: &[&str]| words.iter().any(|w| n.contains(w));
    if any(&["mono", "courier", "code", "consol", "menlo", "typewriter"]) {
        ("modern", "fixed", "Consolas")
    } else if (n.contains("serif") && !n.contains("sans"))
        || any(&[
            "times",
            "georgia",
            "garamond",
            "roman",
            "minion",
            "merriweather",
            "playfair",
            "lora",
            "cambria",
            "baskerville",
            "bodoni",
            "palatino",
            "caslon",
            "didot",
        ])
    {
        ("roman", "variable", "Georgia")
    } else {
        ("swiss", "variable", "Segoe UI")
    }
}

/// `word/fontTable.xml` for the fonts `document` names in `rFonts`, each with its kind, so Word
/// replaces a font that isn't installed by one of the same kind (a missing sans serif would
/// otherwise come out in a serif face). `None` when the document names no font.
pub(crate) fn font_table(document: &str) -> Option<String> {
    let mut names: Vec<&str> = Vec::new();
    for piece in document.split("<w:rFonts w:ascii=\"").skip(1) {
        let Some((name, _)) = piece.split_once('"') else { continue };
        if !names.contains(&name) && names.len() < 256 {
            names.push(name);
        }
    }
    if names.is_empty() {
        return None;
    }
    let mut s = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:fonts xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\">",
    );
    // The names are already escaped: they come out of the document's attributes.
    for name in names {
        let (family, pitch, alt) = font_kind(name);
        s.push_str(&format!(
            "<w:font w:name=\"{name}\"><w:altName w:val=\"{alt}\"/><w:family w:val=\"{family}\"/><w:pitch w:val=\"{pitch}\"/></w:font>"
        ));
    }
    s.push_str("</w:fonts>");
    Some(s)
}

/// A run with its font: `rFonts` first, as the schema orders `rPr`.
fn run(r: &Run) -> String {
    let mut rpr = String::new();
    if let Some(f) = family(&r.font) {
        let f = esc(&f);
        rpr.push_str(&format!("<w:rFonts w:ascii=\"{f}\" w:hAnsi=\"{f}\" w:cs=\"{f}\" w:eastAsia=\"{f}\"/>"));
    }
    if r.bold {
        rpr.push_str("<w:b/><w:bCs/>");
    }
    if r.italic {
        rpr.push_str("<w:i/><w:iCs/>");
    }
    if let Some(h) = hex(r.color) {
        rpr.push_str(&format!("<w:color w:val=\"{h}\"/>"));
    }
    let half_points = if r.size.is_finite() { (r.size * 2.0).round().clamp(2.0, 3276.0) as i64 } else { 24 };
    rpr.push_str(&format!("<w:sz w:val=\"{half_points}\"/><w:szCs w:val=\"{half_points}\"/>"));
    format!("<w:r><w:rPr>{rpr}</w:rPr><w:t xml:space=\"preserve\">{}</w:t></w:r>", esc(&r.text))
}

/// The box around some runs, or `None` when none has a finite box.
fn union<'a>(runs: impl IntoIterator<Item = &'a Run>) -> Option<[f64; 4]> {
    runs.into_iter()
        .map(|r| r.rect)
        .filter(|r| r.iter().all(|v| v.is_finite()))
        .reduce(|a, b| [a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])])
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Align {
    Left,
    Center,
    Right,
}

/// How a paragraph's rows line up: all on one left edge, centred, or flush right. Anything
/// else is left aligned with each row indented to where it starts.
fn alignment(rows: &[[f64; 4]], size: f64) -> Align {
    let spread = |f: &dyn Fn(&[f64; 4]) -> f64| {
        let v: Vec<f64> = rows.iter().map(f).collect();
        v.iter().copied().fold(f64::NEG_INFINITY, f64::max) - v.iter().copied().fold(f64::INFINITY, f64::min)
    };
    let tolerance = (size * 0.3).clamp(1.5, 6.0);
    if rows.len() < 2 || spread(&|r| r[0]) <= tolerance {
        Align::Left
    } else if spread(&|r| (r[0] + r[2]) / 2.0) <= tolerance {
        Align::Center
    } else if spread(&|r| r[2]) <= tolerance {
        Align::Right
    } else {
        Align::Left
    }
}

/// A row's text: its runs left to right, with a space where the PDF leaves a gap between two.
fn row_runs(runs: &[Run]) -> String {
    let mut sorted: Vec<&Run> = runs.iter().filter(|r| !r.text.is_empty()).collect();
    sorted.sort_by(|a, b| a.rect[0].total_cmp(&b.rect[0]));
    let mut s = String::new();
    let mut prev: Option<&Run> = None;
    for r in sorted {
        if let Some(p) = prev {
            let gap = r.rect[0] - p.rect[2];
            let spaced = p.text.ends_with(char::is_whitespace) || r.text.starts_with(char::is_whitespace);
            if !spaced && gap.is_finite() && gap > 0.15 * r.size.max(p.size) {
                s.push_str(&run(&Run { text: " ".into(), ..p.clone() }));
            }
        }
        s.push_str(&run(r));
        prev = Some(r);
    }
    s
}

/// One paragraph as a text box: its rows are Word paragraphs with the PDF's line pitch, and the
/// box sits where the first row's text starts.
fn para_shape(page: &Page, p: &Para, shape_id: usize) -> Option<String> {
    let rows: Vec<([f64; 4], &[Run])> = p.rows.iter().filter_map(|r| Some((union(r)?, r.as_slice()))).collect();
    let first = rows.first()?;
    let size_of = |runs: &[Run]| runs.iter().map(|r| r.size).filter(|s| s.is_finite() && *s > 0.0).fold(0.0f64, f64::max).max(1.0);
    let size = rows.iter().map(|r| size_of(r.1)).fold(1.0f64, f64::max);
    let rects: Vec<[f64; 4]> = rows.iter().map(|r| r.0).collect();
    let align = alignment(&rects, size);
    let left = rects.iter().map(|r| r[0]).fold(f64::INFINITY, f64::min);
    let right = rects.iter().map(|r| r[2]).fold(f64::NEG_INFINITY, f64::max);
    // Each row's line height is the distance to the next row's top; the last row repeats the
    // paragraph's usual pitch. Never less than the text, so Word doesn't clip the glyphs.
    let mut pitches: Vec<f64> = rows.windows(2).map(|w| w[0].0[3] - w[1].0[3]).collect();
    let usual = {
        let mut v: Vec<f64> = pitches.iter().copied().filter(|v| v.is_finite() && *v > 0.0).collect();
        v.sort_by(f64::total_cmp);
        v.get(v.len() / 2).copied().unwrap_or(size * 1.2)
    };
    pitches.push(usual);
    let lines: Vec<f64> = rows.iter().zip(&pitches).map(|(r, pitch)| layout_number(pitch.clamp(size_of(r.1) * 1.05, size_of(r.1) * 4.0))).collect();
    let height = layout_number(lines.iter().sum::<f64>()).max(1.0);
    // Word sets a row's glyphs in the lower part of an exact line: centre the PDF row in it.
    let first_line = lines.first().copied().unwrap_or(size * 1.2);
    let first_height = first.0[3] - first.0[1];
    let y = layout_number(page.height - first.0[3] - (first_line - first_height).max(0.0) / 2.0);
    // The box is the text's own extent. Word's fonts are rarely the PDF's and often wider, so
    // rows never wrap (`wrap="none"`): a longer row runs past the box the way its alignment
    // says, instead of wrapping and pushing the next rows down.
    let (x, width) = (layout_number(left), layout_number(right - left + 2.0).max(1.0));
    let mut paras = String::new();
    for ((rect, runs), line) in rows.iter().zip(&lines) {
        let jc = match align {
            Align::Left => {
                let indent = ((rect[0] - left) * 20.0).round();
                if indent >= 20.0 { format!("<w:ind w:left=\"{}\"/>", layout_number(indent / 20.0) as i64 * 20) } else { String::new() }
            }
            Align::Center => "<w:jc w:val=\"center\"/>".into(),
            Align::Right => "<w:jc w:val=\"right\"/>".into(),
        };
        let twips = (line * 20.0).round().max(20.0) as i64;
        paras.push_str(&format!(
            "<w:p><w:pPr><w:spacing w:before=\"0\" w:after=\"0\" w:line=\"{twips}\" w:lineRule=\"exact\"/>{jc}</w:pPr>{}</w:p>",
            row_runs(runs)
        ));
    }
    Some(text_box(shape_id, [x, y, width, height], &paras))
}

/// A DrawingML text box anchored to the page at `[x, y, width, height]` (points, from the top
/// left), with no fill, no outline, no insets and no wrapping; it grows to fit its text.
fn text_box(shape_id: usize, [x, y, width, height]: [f64; 4], content: &str) -> String {
    let emu = |pt: f64| (layout_number(pt) * 12700.0).round() as i64;
    let (x, y, cx, cy) = (emu(x), emu(y), emu(width.max(1.0)), emu(height.max(1.0)));
    // docPr ids are unique in the document; inline pictures number theirs from 1.
    let id = shape_id.saturating_add(100_000);
    format!(
        "<w:r><w:drawing><wp:anchor distT=\"0\" distB=\"0\" distL=\"0\" distR=\"0\" simplePos=\"0\" relativeHeight=\"{id}\" behindDoc=\"0\" locked=\"0\" layoutInCell=\"1\" allowOverlap=\"1\"><wp:simplePos x=\"0\" y=\"0\"/><wp:positionH relativeFrom=\"page\"><wp:posOffset>{x}</wp:posOffset></wp:positionH><wp:positionV relativeFrom=\"page\"><wp:posOffset>{y}</wp:posOffset></wp:positionV><wp:extent cx=\"{cx}\" cy=\"{cy}\"/><wp:effectExtent l=\"0\" t=\"0\" r=\"0\" b=\"0\"/><wp:wrapNone/><wp:docPr id=\"{id}\" name=\"Text {shape_id}\"/><wp:cNvGraphicFramePr/><a:graphic xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\"><a:graphicData uri=\"http://schemas.microsoft.com/office/word/2010/wordprocessingShape\"><wps:wsp><wps:cNvSpPr txBox=\"1\"/><wps:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"{cx}\" cy=\"{cy}\"/></a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom><a:noFill/><a:ln><a:noFill/></a:ln></wps:spPr><wps:txbx><w:txbxContent>{content}</w:txbxContent></wps:txbx><wps:bodyPr rot=\"0\" wrap=\"none\" lIns=\"0\" tIns=\"0\" rIns=\"0\" bIns=\"0\" anchor=\"t\" anchorCtr=\"0\"><a:spAutoFit/></wps:bodyPr></wps:wsp></a:graphicData></a:graphic></wp:anchor></w:drawing></w:r>"
    )
}

/// A page in layout mode: the background picture behind everything, then a text box per
/// paragraph (rotated ones keep [`crate::block_shape`]).
pub(crate) fn layout_page<'a>(page: &'a Page, layout: &'a Layout, media: &mut Vec<(String, &'a Image)>, shape_id: &mut usize) -> String {
    let mut s = String::from(crate::ANCHOR_OPEN);
    *shape_id = shape_id.saturating_add(1);
    let n = media.len().saturating_add(1);
    let (w, h) = (layout_number(page.width).max(1.0), layout_number(page.height).max(1.0));
    s.push_str(&format!(
        "<w:r><w:pict><v:rect id=\"Background{shape_id}\" style=\"position:absolute;margin-left:0;margin-top:0;width:{w:.3}pt;height:{h:.3}pt;z-index:-3;mso-position-horizontal-relative:page;mso-position-vertical-relative:page\" filled=\"f\" stroked=\"f\"><v:imagedata r:id=\"rIdImg{n}\"/><w10:wrap type=\"none\"/></v:rect></w:pict></w:r>"
    ));
    media.push((format!("image{n}.{}", layout.background.ext), &layout.background));
    let body = crate::body_size(std::slice::from_ref(page)).max(1.0);
    for p in &layout.paras {
        *shape_id = shape_id.saturating_add(1);
        let shape = if p.block.rotation.abs() > 0.1 { None } else { para_shape(page, p, *shape_id) };
        s.push_str(&shape.unwrap_or_else(|| crate::block_shape(page, &p.block, *shape_id, body)));
    }
    s.push_str("</w:p>");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_families_lose_tags_styles_and_postscript_endings() {
        assert_eq!(family("ABCDEF+TimesNewRomanPS-BoldMT").as_deref(), Some("Times New Roman"));
        assert_eq!(family("ArialMT").as_deref(), Some("Arial"));
        assert_eq!(family("QWERTY+Montserrat-SemiBold").as_deref(), Some("Montserrat"));
        assert_eq!(family("DMSans-Regular").as_deref(), Some("DM Sans"));
        assert_eq!(family("Arial,Bold").as_deref(), Some("Arial"));
        assert_eq!(family("Onest").as_deref(), Some("Onest"));
        // Not a subset tag: kept.
        assert_eq!(family("Abc+Def").as_deref(), Some("Abc+Def"));
        assert_eq!(family(""), None);
        assert_eq!(family("ABCDEF+123"), None);
    }

    #[test]
    fn the_font_table_names_each_font_once_with_its_kind() {
        let doc = "<w:rFonts w:ascii=\"Onest\" w:hAnsi=\"Onest\"/><w:rFonts w:ascii=\"JetBrains Mono\"/><w:rFonts w:ascii=\"Onest\"/><w:rFonts w:ascii=\"Times New Roman\"/>";
        let t = font_table(doc).unwrap();
        assert_eq!(t.matches("<w:font ").count(), 3);
        assert!(t.contains("w:name=\"Onest\"><w:altName w:val=\"Segoe UI\"/><w:family w:val=\"swiss\"/>"));
        assert!(t.contains("w:name=\"JetBrains Mono\"><w:altName w:val=\"Consolas\"/><w:family w:val=\"modern\"/><w:pitch w:val=\"fixed\"/>"));
        assert!(t.contains("w:name=\"Times New Roman\"><w:altName w:val=\"Georgia\"/><w:family w:val=\"roman\"/>"));
        assert!(font_table("<w:p/>").is_none());
        // A cut-off attribute is skipped.
        assert!(font_table("<w:rFonts w:ascii=\"Unterminated").is_none());
    }

    fn run_at(text: &str, x: f64, top: f64, w: f64) -> Run {
        Run { text: text.into(), rect: [x, top - 10.0, x + w, top], size: 10.0, font: "Helvetica".into(), ..Run::default() }
    }

    #[test]
    fn rows_line_up_as_the_pdf_draws_them() {
        assert_eq!(alignment(&[[10.0, 0.0, 200.0, 10.0], [10.5, 0.0, 150.0, 10.0]], 10.0), Align::Left);
        assert_eq!(alignment(&[[10.0, 0.0, 210.0, 10.0], [60.0, 0.0, 160.0, 10.0]], 10.0), Align::Center);
        assert_eq!(alignment(&[[10.0, 0.0, 210.0, 10.0], [110.0, 0.0, 210.5, 10.0]], 10.0), Align::Right);
        assert_eq!(alignment(&[[f64::NAN; 4], [1.0; 4]], f64::NAN), Align::Left);
    }

    #[test]
    fn runs_join_left_to_right_with_spaces_at_gaps() {
        let row = [run_at("world", 60.0, 700.0, 30.0), run_at("Hello", 10.0, 700.0, 40.0), run_at("!", 90.0, 700.0, 3.0)];
        let xml = row_runs(&row);
        let texts: Vec<&str> = xml.split("<w:t xml:space=\"preserve\">").skip(1).filter_map(|t| t.split("</w:t>").next()).collect();
        assert_eq!(texts, ["Hello", " ", "world", "!"]);
    }

    #[test]
    fn a_paragraph_box_keeps_rows_pitch_and_fonts() {
        let page = Page { width: 612.0, height: 792.0, ..Page::default() };
        let p = Para {
            block: Block { text: "a b".into(), rect: [72.0, 670.0, 300.0, 700.0], size: 10.0, ..Block::default() },
            rows: vec![vec![run_at("First row", 72.0, 700.0, 100.0)], vec![run_at("Second", 72.0, 686.0, 60.0)]],
        };
        let xml = para_shape(&page, &p, 7).unwrap();
        assert_eq!(xml.matches("<w:p>").count(), 2);
        // 14 pt between the rows' tops: an exact 280-twip line.
        assert!(xml.contains("w:line=\"280\" w:lineRule=\"exact\""), "{xml}");
        assert!(xml.contains("w:ascii=\"Helvetica\""));
        assert!(xml.contains("wrap=\"none\""));
        // 72 pt from the left edge, in EMU.
        assert!(xml.contains("<wp:posOffset>914400</wp:posOffset>"));
        // No rows: nothing to place this way.
        assert!(para_shape(&page, &Para::default(), 1).is_none());
    }
}
