use std::io::{Cursor, Write};
use std::sync::Arc;

use pdfcraft_cos::{Dict, Document, Object, SaveOptions, write_full};

use super::*;

// ── helpers ─────────────────────────────────────────────────────────────────────────────────

/// A zip archive of `files`, deflated.
fn zip_of(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for (name, data) in files {
        w.start_file(*name, opts).unwrap();
        w.write_all(data).unwrap();
    }
    w.finish().unwrap().into_inner()
}

/// Written and read back (also by hayro), so the output is a valid PDF.
fn reopen(doc: &Document) -> Document {
    let bytes = write_full(doc, &SaveOptions::default()).unwrap();
    hayro_syntax::Pdf::new(bytes.clone()).expect("parses");
    Document::open(Arc::new(bytes)).unwrap()
}

fn pages(doc: &Document) -> Vec<Dict> {
    let pages = doc.get(doc.root().unwrap()).as_dict().unwrap().reference(b"Pages").unwrap();
    let kids = doc.get(pages).as_dict().unwrap().get(b"Kids").unwrap().as_array().unwrap().clone();
    kids.iter().map(|k| doc.resolve(k).as_dict().cloned().unwrap()).collect()
}

fn content(doc: &Document, page: &Dict) -> Vec<u8> {
    let c = doc.resolve(page.get(b"Contents").unwrap());
    let Object::Stream(s) = &*c else { panic!("no content stream") };
    s.decoded().unwrap()
}

/// Every page's content stream.
fn contents(doc: &Document) -> Vec<Vec<u8>> {
    let doc = reopen(doc);
    pages(&doc).iter().map(|p| content(&doc, p)).collect()
}

fn has(hay: &[u8], needle: &str) -> bool {
    let needle = pdfcraft_fonts::win_ansi(needle);
    hay.windows(needle.len()).any(|w| w == needle.as_slice())
}

/// Whether `text` is shown with font `/Fn` somewhere in `hay`.
fn shown_in(hay: &[u8], font: usize, text: &str) -> bool {
    let mut lit = pdfcraft_fonts::literal(&pdfcraft_fonts::win_ansi(text));
    lit.extend_from_slice(b" Tj");
    hay.windows(lit.len()).enumerate().any(|(i, w)| {
        w == lit.as_slice() && {
            let before = &hay[..i];
            before.windows(2).rposition(|x| x == b"/F").is_some_and(|at| before.get(at + 2) == Some(&(b'0' + font as u8)))
        }
    })
}

fn media(d: &Dict) -> Vec<f64> {
    d.get(b"MediaBox").unwrap().as_array().unwrap().iter().map(|o| o.as_f64().unwrap()).collect()
}

/// A tiny opaque PNG.
fn png_bytes(w: u32, h: u32) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, w, h);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        let mut wr = enc.write_header().unwrap();
        wr.write_image_data(&vec![120u8; (w * h * 3) as usize]).unwrap();
    }
    out
}

const W: &str = r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main""#;

fn docx(body: &str, extra: &[(&str, &[u8])]) -> Vec<u8> {
    let document = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document {W}><w:body>{body}<w:sectPr><w:pgSz w:w="11906" w:h="16838"/></w:sectPr></w:body></w:document>"#
    );
    let styles = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><w:styles {W}>
        <w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/></w:style>
        <w:style w:type="paragraph" w:styleId="Titre1"><w:name w:val="heading 1"/><w:basedOn w:val="Normal"/></w:style>
        <w:style w:type="paragraph" w:styleId="Titre2"><w:name w:val="heading 2"/><w:basedOn w:val="Normal"/></w:style>
        <w:style w:type="paragraph" w:styleId="Titre"><w:name w:val="Title"/></w:style>
        <w:style w:type="character" w:styleId="Strong"><w:name w:val="Strong"/><w:rPr><w:b/></w:rPr></w:style>
        </w:styles>"#
    );
    let numbering = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><w:numbering {W}>
        <w:abstractNum w:abstractNumId="0"><w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="bullet"/><w:lvlText w:val="&#61623;"/></w:lvl></w:abstractNum>
        <w:abstractNum w:abstractNumId="1"><w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="decimal"/><w:lvlText w:val="%1."/></w:lvl>
          <w:lvl w:ilvl="1"><w:start w:val="1"/><w:numFmt w:val="lowerLetter"/><w:lvlText w:val="%2)"/></w:lvl></w:abstractNum>
        <w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num>
        <w:num w:numId="2"><w:abstractNumId w:val="1"/></w:num>
        </w:numbering>"#
    );
    let mut files: Vec<(&str, &[u8])> = vec![
        ("[Content_Types].xml", b"<Types/>"),
        ("word/document.xml", document.as_bytes()),
        ("word/styles.xml", styles.as_bytes()),
        ("word/numbering.xml", numbering.as_bytes()),
    ];
    files.extend_from_slice(extra);
    zip_of(&files)
}

fn p(style: Option<&str>, runs: &str) -> String {
    let ppr = style.map(|s| format!(r#"<w:pPr><w:pStyle w:val="{s}"/></w:pPr>"#)).unwrap_or_default();
    format!("<w:p>{ppr}{runs}</w:p>")
}

fn r(text: &str) -> String {
    format!(r#"<w:r><w:t xml:space="preserve">{text}</w:t></w:r>"#)
}

fn li(num: u32, level: u8, text: &str) -> String {
    format!(r#"<w:p><w:pPr><w:numPr><w:ilvl w:val="{level}"/><w:numId w:val="{num}"/></w:numPr></w:pPr>{}</w:p>"#, r(text))
}

// ── characters ──────────────────────────────────────────────────────────────────────────────

#[test]
fn french_typography_is_drawn_and_other_scripts_are_escaped() {
    for c in "éèàçœŒ«»’…–—€êâîôûëïüÿÉÈÀÇ".chars() {
        assert_eq!(map_char(c), Mapped::Char(c), "{c} must be drawn as itself");
    }
    // A narrow no-break space (before « ; : ! ? ») keeps its no-break meaning.
    assert_eq!(map_char('\u{202f}'), Mapped::Char('\u{a0}'));
    assert_eq!(map_char('\u{fb01}'), Mapped::Str("fi"));
    assert_eq!(map_char('\u{f0b7}'), Mapped::Char('•'), "Word's Symbol-font bullet");
    assert_eq!(map_char('\u{200b}'), Mapped::Drop);
    assert_eq!(map_char('漢'), Mapped::Escape);
    let (doc, report) = convert_with_report("notes.txt", "Café « déjà vu » — 漢字\n".as_bytes()).unwrap();
    assert_eq!(report.escaped, 2);
    let c = contents(&doc);
    assert!(has(&c[0], "Café « déjà vu » — [U+6F22][U+5B57]"), "{}", String::from_utf8_lossy(&c[0]));
}

#[test]
fn words_wrap_and_long_words_break_by_character() {
    let lines = layout::wrap_text("Le petit chat est très content aujourd'hui", 11.0, 100.0);
    assert!(lines.len() >= 2, "{lines:?}");
    assert!(lines.iter().all(|l| pdfcraft_fonts::Std14::Helvetica.text_width(l, 11.0) <= 100.0 + 1e-6), "{lines:?}");
    let long = "x".repeat(200);
    let lines = layout::wrap_text(&long, 11.0, 100.0);
    assert!(lines.len() > 5 && lines.concat().len() == 200, "{lines:?}");
}

// ── docx ────────────────────────────────────────────────────────────────────────────────────

#[test]
fn docx_headings_runs_lists_and_tables() {
    let body = [
        p(Some("Titre"), &r("Rapport annuel")),
        p(Some("Titre1"), &r("Résumé exécutif")),
        p(
            None,
            &format!(
                r#"{}<w:r><w:rPr><w:b/></w:rPr><w:t>gras</w:t></w:r>{}<w:r><w:rPr><w:i/></w:rPr><w:t>italique</w:t></w:r><w:r><w:rPr><w:rStyle w:val="Strong"/></w:rPr><w:t xml:space="preserve"> fort</w:t></w:r><w:r><w:tab/><w:t>tab</w:t></w:r>"#,
                r("Du texte "),
                r(" et ")
            ),
        ),
        li(1, 0, "Premier point"),
        li(1, 0, "Second point"),
        li(2, 0, "Étape un"),
        li(2, 1, "Sous-étape"),
        li(2, 0, "Étape deux"),
        r#"<w:tbl><w:tblGrid><w:gridCol w:w="3000"/><w:gridCol w:w="6000"/></w:tblGrid>
            <w:tr><w:trPr><w:tblHeader/></w:trPr><w:tc><w:p><w:r><w:t>Nom</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>Valeur</w:t></w:r></w:p></w:tc></w:tr>
            <w:tr><w:tc><w:p><w:r><w:t>Coût</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>12 €</w:t></w:r></w:p></w:tc></w:tr>
            <w:tr><w:tc><w:tcPr><w:gridSpan w:val="2"/></w:tcPr><w:p><w:r><w:t>Fusionnée</w:t></w:r></w:p></w:tc></w:tr>
          </w:tbl>"#
            .to_string(),
        p(None, r#"<w:r><w:br w:type="page"/></w:r>"#),
        p(Some("Titre2"), &r("Deuxième page")),
        // Field codes and deleted text are not shown.
        p(None, r#"<w:r><w:instrText>PAGE</w:instrText></w:r><w:del><w:r><w:delText>supprimé</w:delText></w:r></w:del><w:r><w:t>Fin</w:t></w:r>"#),
    ]
    .concat();
    let (doc, report) = convert_with_report("Rapport.docx", &docx(&body, &[])).unwrap();
    assert_eq!(report.pages, 2);
    assert_eq!(report.escaped, 0);
    let c = contents(&doc);
    let one = &c[0];
    assert!(shown_in(one, 1, "Rapport annuel"), "title in bold");
    assert!(has(one, "/F1 24.00 Tf"), "the title is the largest text");
    assert!(shown_in(one, 1, "Résumé exécutif") && has(one, "/F1 18.00 Tf"), "heading 1 by its built-in name, whatever the style id");
    assert!(shown_in(one, 0, "Du texte") && shown_in(one, 1, "gras") && shown_in(one, 2, "italique") && shown_in(one, 1, "fort"));
    assert!(has(one, "(•)") && has(one, "(Premier point)"), "bullet list");
    assert!(has(one, "(1.)") && has(one, "(2.)") && has(one, "(a\\))"), "numbered list with a lettered sub-level");
    assert!(shown_in(one, 1, "Nom") && shown_in(one, 0, "Coût") && has(one, "(Fusionnée)"), "table with a bold header row");
    assert!(has(one, " re S Q"), "cell borders");
    assert!(!has(one, "PAGE") && !has(one, "supprimé"));
    assert!(has(&c[1], "(Deuxième page)") && has(&c[1], "(Fin)"), "the page break");
    let doc = reopen(&doc);
    let p = pages(&doc);
    assert_eq!(media(&p[0]), [0.0, 0.0, 595.276, 841.89], "A4 portrait");
}

#[test]
fn docx_images_are_embedded_at_their_size() {
    let png = png_bytes(40, 20);
    let rels = br#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
        <Relationship Id="rId5" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/image1.png"/>
        <Relationship Id="rId6" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/missing.png"/>
        </Relationships>"#;
    let drawing = |rid: &str| {
        format!(
            r#"<w:p><w:r><w:drawing><wp:inline><wp:extent cx="1270000" cy="635000"/><a:graphic><a:graphicData><a:blip r:embed="{rid}"/></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>"#
        )
    };
    let body = [p(None, &r("Avant")), drawing("rId5"), drawing("rId6"), p(None, &r("Après"))].concat();
    let (doc, report) =
        convert_with_report("img.docx", &docx(&body, &[("word/_rels/document.xml.rels", rels), ("word/media/image1.png", &png)])).unwrap();
    assert_eq!(report.images_skipped, 0);
    assert!(report.notes.iter().any(|n| n.contains("missing")), "{:?}", report.notes);
    let c = contents(&doc);
    assert!(has(&c[0], "q 100.000 0 0 50.000"), "100 x 50 pt from the drawing's extent: {}", String::from_utf8_lossy(&c[0]));
    let doc = reopen(&doc);
    let res = pages(&doc)[0].get(b"Resources").unwrap().as_dict().unwrap().clone();
    assert!(res.get(b"XObject").is_some());
}

#[test]
fn docx_landscape_section_turns_the_page() {
    let document = format!(
        r#"<w:document {W}><w:body>{}<w:sectPr><w:pgSz w:w="16838" w:h="11906" w:orient="landscape"/></w:sectPr></w:body></w:document>"#,
        p(None, &r("Large"))
    );
    let bytes = zip_of(&[("word/document.xml", document.as_bytes())]);
    let doc = reopen(&convert("wide.docx", &bytes).unwrap());
    assert_eq!(media(&pages(&doc)[0]), [0.0, 0.0, 841.89, 595.276]);
}

#[test]
fn malformed_docx_is_an_error_not_a_panic() {
    assert!(matches!(convert("a.docx", b"not a zip at all"), Err(ConvertError::Malformed(_))));
    assert!(matches!(convert("a.docx", &zip_of(&[("hello.txt", b"hi")])), Err(ConvertError::Malformed(m)) if m.contains("word/document.xml")));
    assert!(matches!(convert("a.docx", &zip_of(&[("word/document.xml", b"<w:document><w:body>")])), Err(ConvertError::Malformed(_))));
    // A DTD (billion laughs and the like) is refused.
    let dtd = br#"<?xml version="1.0"?><!DOCTYPE d [<!ENTITY a "aaaa">]><d>&a;</d>"#;
    assert!(convert("a.docx", &zip_of(&[("word/document.xml", dtd)])).is_err());
    assert!(convert("a.docx", &[]).is_err());
}

/// Truncated and bit-flipped Office files never panic.
#[test]
fn damaged_office_files_never_panic() {
    let body = [p(Some("Titre1"), &r("Titre")), li(2, 0, "un"), p(None, &r("texte"))].concat();
    let samples = [("a.docx", docx(&body, &[])), ("a.xlsx", xlsx_sample()), ("a.ods", ods_sample())];
    let mut seed = 0x2545_f491_u32;
    for (name, good) in &samples {
        for cut in (0..good.len()).step_by(good.len() / 40 + 1) {
            let _ = convert(name, &good[..cut]);
        }
        for _ in 0..200 {
            let mut bad = good.clone();
            for _ in 0..4 {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                let i = seed as usize % bad.len();
                bad[i] ^= (seed >> 8) as u8 | 1;
            }
            let _ = convert(name, &bad);
        }
    }
}

// ── spreadsheets ────────────────────────────────────────────────────────────────────────────

const X: &str = r#"xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships""#;

fn xlsx_sample() -> Vec<u8> {
    let workbook = format!(
        r#"<workbook {X}><sheets><sheet name="Ventes" sheetId="1" r:id="rId1"/><sheet name="Caché" sheetId="2" state="hidden" r:id="rId2"/><sheet name="Notes" sheetId="3" r:id="rId3"/></sheets></workbook>"#
    );
    let rels = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
        <Relationship Id="rId1" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Target="worksheets/sheet2.xml"/><Relationship Id="rId3" Target="/xl/worksheets/sheet3.xml"/></Relationships>"#;
    let shared =
        format!(r#"<sst {X}><si><t>Produit</t></si><si><t>Prix</t></si><si><r><t>Crème </t></r><r><t>brûlée</t></r></si><si><t>Date</t></si></sst>"#);
    let styles = format!(
        r#"<styleSheet {X}><numFmts><numFmt numFmtId="164" formatCode="dd/mm/yyyy"/></numFmts><cellXfs><xf numFmtId="0"/><xf numFmtId="164"/><xf numFmtId="9"/></cellXfs></styleSheet>"#
    );
    let sheet1 = format!(
        r#"<worksheet {X}><sheetData>
        <row r="1"><c r="A1" t="s"><v>0</v></c><c r="B1" t="s"><v>1</v></c><c r="D1" t="s"><v>3</v></c></row>
        <row r="2"><c r="A2" t="s"><v>2</v></c><c r="B2"><v>4.5</v></c><c r="C2" t="b"><v>1</v></c><c r="D2" s="1"><v>45292</v></c><c r="E2" s="2"><v>0.25</v></c></row>
        <row r="4"><c r="A4" t="inlineStr"><is><t>Tarte</t></is></c><c r="B4"><v>0.30000000000000004</v></c></row>
        </sheetData></worksheet>"#
    );
    let sheet2 = format!(r#"<worksheet {X}><sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>secret</t></is></c></row></sheetData></worksheet>"#);
    let sheet3 = format!(r#"<worksheet {X}><sheetData><row r="1"><c r="A1" t="str"><v>Remarque</v></c></row></sheetData></worksheet>"#);
    zip_of(&[
        ("xl/workbook.xml", workbook.as_bytes()),
        ("xl/_rels/workbook.xml.rels", rels),
        ("xl/sharedStrings.xml", shared.as_bytes()),
        ("xl/styles.xml", styles.as_bytes()),
        ("xl/worksheets/sheet1.xml", sheet1.as_bytes()),
        ("xl/worksheets/sheet2.xml", sheet2.as_bytes()),
        ("xl/worksheets/sheet3.xml", sheet3.as_bytes()),
    ])
}

#[test]
fn xlsx_sheets_become_headed_tables() {
    let (doc, report) = convert_with_report("Budget.xlsx", &xlsx_sample()).unwrap();
    assert_eq!(report.pages, 2, "one page per visible sheet");
    let c = contents(&doc);
    let one = &c[0];
    assert!(shown_in(one, 1, "Ventes"), "the sheet name heads its table");
    assert!(shown_in(one, 1, "Produit") && shown_in(one, 1, "Prix"), "first row as the header");
    assert!(has(one, "(Crème brûlée)"), "rich shared strings");
    for v in ["(4.5)", "(TRUE)", "(2024-01-01)", "(25%)", "(Tarte)", "(0.3)"] {
        assert!(has(one, v), "{v} in {}", String::from_utf8_lossy(one));
    }
    assert!(!c.iter().any(|p| has(p, "secret")), "hidden sheets are left out");
    assert!(has(&c[1], "(Remarque)"), "an absolute relationship target");
}

#[test]
fn wide_sheets_turn_landscape_and_split_columns() {
    let header: Vec<String> = (0..12).map(|i| format!("Colonne{i}")).collect();
    let mut csv = header.join(",") + "\n";
    for r in 0..5 {
        csv += &(0..12).map(|c| format!("valeur{r}x{c}")).collect::<Vec<_>>().join(",");
        csv += "\n";
    }
    let doc = reopen(&convert("wide.csv", csv.as_bytes()).unwrap());
    assert_eq!(media(&pages(&doc)[0]), [0.0, 0.0, 841.89, 595.276], "landscape for many columns");
    // Far too many columns for one page: they are printed in groups, never squeezed to nothing.
    let header: Vec<String> = (0..80).map(|i| format!("UnTitreDeColonneAssezLong{i}")).collect();
    let csv = header.join(";") + "\n" + &vec!["x"; 80].join(";") + "\n";
    let c = contents(&convert("huge.csv", csv.as_bytes()).unwrap());
    let all: Vec<u8> = c.concat();
    assert!(has(&all, "(UnTitreDeColonneAssezLong0)") && has(&all, "(UnTitreDeColonneAssezLong79)"), "every column is shown whole");
}

fn ods_sample() -> Vec<u8> {
    let content = r#"<?xml version="1.0" encoding="UTF-8"?>
    <office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0">
    <office:body><office:spreadsheet>
      <table:table table:name="Feuille1">
        <table:table-header-rows><table:table-row><table:table-cell><text:p>Ville</text:p></table:table-cell><table:table-cell><text:p>Pays</text:p></table:table-cell></table:table-row></table:table-header-rows>
        <table:table-row table:number-rows-repeated="2"><table:table-cell table:number-columns-repeated="2"><text:p>Même<text:s text:c="2"/>texte</text:p></table:table-cell><table:table-cell table:number-columns-repeated="16380"/></table:table-row>
        <table:table-row table:number-rows-repeated="1048570"><table:table-cell table:number-columns-repeated="16384"/></table:table-row>
        <table:table-row><table:table-cell><text:p>Lyon</text:p></table:table-cell><table:table-cell office:value-type="float" office:value="3"><text:p>3,0</text:p></table:table-cell></table:table-row>
      </table:table>
    </office:spreadsheet></office:body></office:document-content>"#;
    zip_of(&[("mimetype", b"application/vnd.oasis.opendocument.spreadsheet"), ("content.xml", content.as_bytes())])
}

#[test]
fn ods_tables_expand_repeats_and_skip_empty_space() {
    let (doc, report) = convert_with_report("villes.ods", &ods_sample()).unwrap();
    assert_eq!(report.pages, 1);
    assert!(report.notes.is_empty(), "{:?}", report.notes);
    let c = contents(&doc);
    assert!(shown_in(&c[0], 1, "Feuille1") && shown_in(&c[0], 1, "Ville"));
    let count = c[0].windows(6).filter(|w| *w == b"(M\xeame)").count();
    assert_eq!(count, 4, "two repeated rows of two repeated cells");
    assert!(has(&c[0], "(Lyon)") && has(&c[0], "(3,0)"), "the displayed text, after a million empty rows");
}

#[test]
fn csv_delimiters_and_quotes() {
    use sheet::detect_delimiter;
    assert_eq!(detect_delimiter("a;b;c\n1;2,5;3\n"), ';');
    assert_eq!(detect_delimiter("a,b\n1,2\n"), ',');
    assert_eq!(detect_delimiter("a\tb\n1\t2\n"), '\t');
    assert_eq!(detect_delimiter("\"x;y\",b\n1,2\n"), ',', "delimiters inside quotes don't count");
    let csv = "Nom;Remarque\r\nDupont;\"Il a dit \"\"oui\"\"; puis\nest parti\"\r\nMartin;ok\r\n";
    let c = contents(&convert("liste.csv", csv.as_bytes()).unwrap());
    assert!(
        shown_in(&c[0], 1, "Nom") && has(&c[0], "(Il a dit \"oui\"; puis)") && has(&c[0], "(est parti)") && has(&c[0], "(Martin)"),
        "{}",
        String::from_utf8_lossy(&c[0])
    );
    // Windows-1252 text (no BOM, not UTF-8) still reads as French.
    let c = contents(&convert("latin.csv", b"Pr\xe9nom,Ville\nH\xe9l\xe8ne,S\xe8te\n").unwrap());
    assert!(has(&c[0], "(Hélène)") && has(&c[0], "(Sète)"));
}

#[test]
fn long_tables_continue_on_new_pages_under_their_header() {
    let mut csv = String::from("Numéro,Libellé\n");
    for i in 0..400 {
        csv += &format!("{i},Ligne numéro {i}\n");
    }
    let c = contents(&convert("long.csv", csv.as_bytes()).unwrap());
    assert!(c.len() >= 5, "{} pages", c.len());
    assert!(c.iter().all(|p| shown_in(p, 1, "Numéro")), "the header row repeats on every page");
    assert!(has(c.last().unwrap(), "(399)"));
}

#[test]
fn sheet_number_formats() {
    use sheet::{NumKind, cell_ref, classify_code, date_text, general};
    assert_eq!(cell_ref("A1"), Some((0, 0)));
    assert_eq!(cell_ref("AB12"), Some((11, 27)));
    assert_eq!(cell_ref("A0"), None);
    assert_eq!(cell_ref("ABCD1"), None);
    assert_eq!(cell_ref("12"), None);
    assert_eq!(general(0.1 + 0.2), "0.3");
    assert_eq!(general(1234.0), "1234");
    assert_eq!(general(-2.5), "-2.5");
    assert_eq!(general(f64::NAN), "NaN");
    assert_eq!(date_text(45292.0, NumKind::Date).as_deref(), Some("2024-01-01"));
    assert_eq!(date_text(45292.5, NumKind::DateTime).as_deref(), Some("2024-01-01 12:00"));
    assert_eq!(date_text(1e300, NumKind::Date), None);
    assert_eq!(classify_code("dd/mm/yyyy"), NumKind::Date);
    assert_eq!(classify_code("[h]:mm"), NumKind::Time);
    assert_eq!(classify_code("0.00%"), NumKind::Percent);
    assert_eq!(classify_code("#,##0.00\" days\""), NumKind::General, "quoted text doesn't count");
}

// ── html ────────────────────────────────────────────────────────────────────────────────────

#[test]
fn html_structure_entities_and_scripts() {
    let html = r#"<!DOCTYPE html><html><head><title>Ignoré</title><style>p { color: red }</style>
        <script>document.write("<p>pas affiché</p>")</script></head><body>
        <h1>Bienvenue &amp; bonjour</h1>
        <!-- un commentaire <p>caché</p> -->
        <p>Un <b>texte</b> en <i>italique</i>&nbsp;: l&#39;été&hellip; <br>nouvelle ligne &unknown; 3 &lt; 4</p>
        <ul><li>pomme<li>poire<ul><li>imbriqué</li></ul></li></ul>
        <ol start="3"><li>trois</li><li>quatre</li></ol>
        <table><thead><tr><th>Clé<th>Valeur</thead><tr><td>a<td>1<tr><td colspan=2>large</table>
        <p>a < b et fin
        </body></html>"#;
    let c = contents(&convert("page.html", html.as_bytes()).unwrap());
    let one = &c[0];
    assert!(shown_in(one, 1, "Bienvenue & bonjour") && has(one, "/F1 18.00 Tf"));
    assert!(!has(one, "Ignoré") && !has(one, "color") && !has(one, "affiché") && !has(one, "caché"));
    assert!(shown_in(one, 1, "texte") && shown_in(one, 2, "italique") && has(one, "l'été…") && has(one, "&unknown; 3 < 4"));
    assert!(has(one, "(nouvelle ligne"));
    assert!(has(one, "(pomme)") && has(one, "(poire)") && has(one, "(imbriqué)") && has(one, "(–)"));
    assert!(has(one, "(3.)") && has(one, "(4.)"), "ordered list with start");
    assert!(shown_in(one, 1, "Clé") && has(one, "(large)") && has(one, " re S Q"));
    assert!(has(one, "(a < b et fin)"), "a stray < is text");
}

#[test]
fn html_scanner_is_lenient() {
    use html::{Tok, scan};
    assert_eq!(
        scan("<p class='x>y'>a</p>"),
        [Tok::Open { name: "p".into(), attrs: vec![("class".into(), "x>y".into())] }, Tok::Text("a".into()), Tok::Close("p".into())]
    );
    // Unclosed constructs run to the end instead of failing.
    for s in ["<p", "<!--", "<script>", "<a href=\"", "&#xFFFFFFFF;", "&#55296;", "<<>>", "</>", "<table><tr><td><table><td>x"] {
        let _ = scan(s);
        let _ = convert("x.html", s.as_bytes()).unwrap();
    }
    assert_eq!(html::decode_entities("&#150;&#x41;&eacute;&amp"), "–Aé&amp");
}

// ── text, dispatch ──────────────────────────────────────────────────────────────────────────

#[test]
fn plain_text_and_form_feeds() {
    let doc = convert("notes.txt", "Ligne 1\n\nLigne 3\u{c}Page 2".as_bytes()).unwrap();
    let c = contents(&doc);
    assert_eq!(c.len(), 2);
    assert!(has(&c[0], "(Ligne 1)") && has(&c[0], "(Ligne 3)") && has(&c[1], "(Page 2)"));
}

#[test]
fn unsupported_formats_say_what_to_do() {
    let Err(ConvertError::Unsupported(m)) = convert("vieux.xls", b"\xd0\xcf\x11\xe0") else { panic!() };
    assert!(m.contains(".xlsx"), "{m}");
    let Err(ConvertError::Unsupported(m)) = convert("vieux.doc", b"") else { panic!() };
    assert!(m.contains(".docx"), "{m}");
    let Err(ConvertError::Unsupported(m)) = convert("a.pdf", b"%PDF-1.7") else { panic!() };
    assert!(m.contains(".docx") && m.contains(".csv"), "{m}");
    assert!(handles("Rapport.DOCX") && handles("a.xls") && !handles("a.pdf") && !handles("noext"));
    assert!(supported_extensions().contains(&"xlsx"));
}

#[test]
fn empty_inputs_still_make_a_page() {
    for (name, bytes) in [("e.txt", Vec::new()), ("e.csv", Vec::new()), ("e.html", Vec::new()), ("e.docx", docx("", &[]))] {
        let doc = convert(name, &bytes).unwrap();
        assert_eq!(pages(&reopen(&doc)).len(), 1, "{name}");
    }
}

#[test]
fn info_title_comes_from_the_file_name() {
    let doc = reopen(&convert("C:\\dossier\\Mon rapport.txt", b"x").unwrap());
    let info = doc.trailer().reference(b"Info").unwrap();
    let title = doc.get(info).as_dict().unwrap().get(b"Title").unwrap().as_string().unwrap().to_text();
    assert_eq!(title, "Mon rapport");
}
