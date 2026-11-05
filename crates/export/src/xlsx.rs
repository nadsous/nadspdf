//! A one-sheet Excel workbook (.xlsx, Office Open XML SpreadsheetML): a bold header row, then
//! text and number cells. Enough for a table an assistant extracted, opened in Excel,
//! LibreOffice or Google Sheets.

use crate::Zip;
use crate::esc;

/// One cell.
#[derive(Clone, Debug, PartialEq)]
pub enum XlsxCell {
    Empty,
    Text(String),
    Number(f64),
}

/// Excel's limits: rows, columns, and characters in a cell.
const MAX_ROWS: usize = 1_048_575;
const MAX_COLUMNS: usize = 16_384;
const MAX_CELL_CHARS: usize = 32_767;

/// The column letters of 0-based column `i` (0 → A, 26 → AA).
fn column(mut i: usize) -> String {
    let mut out = Vec::new();
    loop {
        out.push(b'A' + (i % 26) as u8);
        if i < 26 {
            break;
        }
        i = i / 26 - 1;
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

/// A sheet name Excel accepts: at most 31 characters, none of `[]:*?/\`, not empty.
fn sheet_name(name: &str) -> String {
    let clean: String = name.chars().filter(|c| !"[]:*?/\\".contains(*c) && !c.is_control()).take(31).collect();
    let clean = clean.trim().trim_matches('\'').to_string();
    if clean.is_empty() { "Sheet1".into() } else { clean }
}

fn text_cell(reference: &str, text: &str, style: u8) -> String {
    let text: String = text.chars().filter(|c| !c.is_control() || matches!(c, '\n' | '\t')).take(MAX_CELL_CHARS).collect();
    format!("<c r=\"{reference}\" t=\"inlineStr\" s=\"{style}\"><is><t xml:space=\"preserve\">{}</t></is></c>", esc(&text))
}

/// The workbook: `header` in bold, then `rows`, on a sheet named `name`.
pub fn xlsx(name: &str, header: &[String], rows: &[Vec<XlsxCell>]) -> Vec<u8> {
    let cols = header.len().max(rows.iter().map(Vec::len).max().unwrap_or(0)).min(MAX_COLUMNS);
    let mut sheet = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<worksheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\">",
    );
    if cols > 0 {
        // Wide enough to read, without measuring.
        sheet.push_str(&format!("<cols><col min=\"1\" max=\"{cols}\" width=\"22\" customWidth=\"1\"/></cols>"));
    }
    sheet.push_str("<sheetData>");
    sheet.push_str("<row r=\"1\">");
    for (c, h) in header.iter().take(cols).enumerate() {
        sheet.push_str(&text_cell(&format!("{}1", column(c)), h, 1));
    }
    sheet.push_str("</row>");
    for (r, row) in rows.iter().take(MAX_ROWS).enumerate() {
        let n = r + 2;
        sheet.push_str(&format!("<row r=\"{n}\">"));
        for (c, cell) in row.iter().take(cols).enumerate() {
            let reference = format!("{}{n}", column(c));
            match cell {
                XlsxCell::Empty => {}
                XlsxCell::Text(t) => sheet.push_str(&text_cell(&reference, t, 0)),
                XlsxCell::Number(v) if v.is_finite() => sheet.push_str(&format!("<c r=\"{reference}\"><v>{v}</v></c>")),
                XlsxCell::Number(_) => {}
            }
        }
        sheet.push_str("</row>");
    }
    sheet.push_str("</sheetData>");
    sheet.push_str("</worksheet>");

    let types = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
<Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/>\
<Override PartName=\"/xl/workbook.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml\"/>\
<Override PartName=\"/xl/worksheets/sheet1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml\"/>\
<Override PartName=\"/xl/styles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml\"/></Types>";
    let rels = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
<Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"xl/workbook.xml\"/></Relationships>";
    let workbook = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<workbook xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\">\
<sheets><sheet name=\"{}\" sheetId=\"1\" r:id=\"rId1\"/></sheets></workbook>",
        esc(&sheet_name(name))
    );
    let workbook_rels = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
<Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet\" Target=\"worksheets/sheet1.xml\"/>\
<Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles\" Target=\"styles.xml\"/></Relationships>";
    // Style 0: normal; style 1: bold (the header).
    let styles = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<styleSheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\">\
<fonts count=\"2\"><font><sz val=\"11\"/><name val=\"Calibri\"/></font><font><b/><sz val=\"11\"/><name val=\"Calibri\"/></font></fonts>\
<fills count=\"2\"><fill><patternFill patternType=\"none\"/></fill><fill><patternFill patternType=\"gray125\"/></fill></fills>\
<borders count=\"1\"><border><left/><right/><top/><bottom/><diagonal/></border></borders>\
<cellStyleXfs count=\"1\"><xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"0\"/></cellStyleXfs>\
<cellXfs count=\"2\"><xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"0\" xfId=\"0\"/><xf numFmtId=\"0\" fontId=\"1\" fillId=\"0\" borderId=\"0\" xfId=\"0\" applyFont=\"1\"/></cellXfs>\
</styleSheet>";
    let mut z = Zip::default();
    z.add("[Content_Types].xml", types.as_bytes(), true);
    z.add("_rels/.rels", rels.as_bytes(), true);
    z.add("xl/workbook.xml", workbook.as_bytes(), true);
    z.add("xl/_rels/workbook.xml.rels", workbook_rels.as_bytes(), true);
    z.add("xl/styles.xml", styles.as_bytes(), true);
    z.add("xl/worksheets/sheet1.xml", sheet.as_bytes(), true);
    z.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columns_are_lettered_like_excel() {
        assert_eq!([column(0), column(25), column(26), column(27), column(701), column(702)], ["A", "Z", "AA", "AB", "ZZ", "AAA"]);
        assert_eq!(sheet_name("Factures [2026]: Q1?"), "Factures 2026 Q1");
        assert_eq!(sheet_name("///"), "Sheet1");
    }

    #[test]
    fn a_workbook_holds_its_header_text_and_numbers() {
        let bytes = xlsx(
            "Factures",
            &["N°".into(), "Montant (EUR)".into()],
            &[vec![XlsxCell::Text("F-1 <A&B>".into()), XlsxCell::Number(1234.5)], vec![XlsxCell::Empty, XlsxCell::Number(f64::NAN)]],
        );
        assert!(bytes.starts_with(b"PK"));
        let sheet = crate::tests::part(&bytes, "xl/worksheets/sheet1.xml");
        assert!(sheet.contains("<c r=\"A1\" t=\"inlineStr\" s=\"1\">"), "{sheet}");
        assert!(sheet.contains("F-1 &lt;A&amp;B&gt;") && sheet.contains("<c r=\"B2\"><v>1234.5</v></c>"), "{sheet}");
        assert!(!sheet.contains("NaN"), "a number that isn't one is left empty");
        assert!(crate::tests::part(&bytes, "xl/workbook.xml").contains("name=\"Factures\""));
    }
}
