use super::*;

/// A 4 × 4 red PNG.
fn png() -> Vec<u8> {
    // Signature, IHDR (4×4, RGB8), one IDAT of zlib-stored rows, IEND.
    fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        out.extend_from_slice(kind);
        out.extend_from_slice(data);
        let mut crc = 0xffff_ffffu32;
        for &b in kind.iter().chain(data) {
            crc ^= u32::from(b);
            for _ in 0..8 {
                crc = if crc & 1 != 0 { (crc >> 1) ^ 0xedb8_8320 } else { crc >> 1 };
            }
        }
        out.extend_from_slice(&(!crc).to_be_bytes());
    }
    // Four rows: filter byte 0, then four red pixels.
    let row: Vec<u8> = std::iter::once(0).chain([255, 0, 0].repeat(4)).collect();
    let raw = row.repeat(4);
    let mut z = vec![0x78, 0x01, 1];
    z.extend_from_slice(&(raw.len() as u16).to_le_bytes());
    z.extend_from_slice(&(!(raw.len() as u16)).to_le_bytes());
    z.extend_from_slice(&raw);
    let (mut a, mut b) = (1u32, 0u32);
    for &x in &raw {
        a = (a + u32::from(x)) % 65521;
        b = (b + a) % 65521;
    }
    z.extend_from_slice(&((b << 16) | a).to_be_bytes());
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    chunk(&mut out, b"IHDR", &[0, 0, 0, 4, 0, 0, 0, 4, 8, 2, 0, 0, 0]);
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    out
}

fn a4() -> Layout {
    Layout::new(Paper::A4, 1).unwrap_or_else(|e| panic!("{e}"))
}

#[test]
fn a_new_document_has_its_pages_at_the_paper_size() {
    let l = Layout::new(Paper::A4, 3).unwrap();
    assert_eq!(l.page_count(), 3);
    let (w, h) = l.page_size(0).unwrap();
    assert!((w - 595.28).abs() < 1.0 && (h - 841.89).abs() < 1.0, "{w} × {h}");
    let l = Layout::new(Paper::Letter, 0).unwrap();
    assert_eq!(l.page_count(), 1, "at least one page");
    assert_eq!(l.page_size(0).map(|(w, h)| (w.round(), h.round())), Some((612.0, 792.0)));
    assert!(l.page_size(1).is_none());
}

#[test]
fn a_text_frame_is_placed_where_asked_and_holds_its_text() {
    let mut l = a4();
    let id = l.add_text_frame(0, Rect::new(300.0, 200.0, 72.0, 100.0), "Bonjour NadsPDF").unwrap();
    let it = l.item(0, id).unwrap();
    assert_eq!(it.kind, ItemKind::Text);
    let r = it.rect;
    assert!((r.x0 - 72.0).abs() < 0.5 && (r.y0 - 100.0).abs() < 0.5 && (r.x1 - 300.0).abs() < 0.5 && (r.y1 - 200.0).abs() < 0.5, "{r:?}");
    assert_eq!(l.text(id).as_deref(), Some("Bonjour NadsPDF"));
    assert_eq!(l.item_at(0, 100.0, 150.0).map(|i| i.id), Some(id));
    assert!(l.item_at(0, 10.0, 10.0).is_none());
}

#[test]
fn text_is_replaced_and_formatted() {
    let mut l = a4();
    let id = l.add_text_frame(0, Rect::new(72.0, 72.0, 400.0, 300.0), "avant").unwrap();
    l.set_text(id, "après, sur deux\nparagraphes").unwrap();
    assert_eq!(l.text(id).as_deref().map(|t| t.replace(['\u{2029}', '\r'], "\n")).as_deref(), Some("après, sur deux\nparagraphes"));
    l.set_font_size(id, 24.0).unwrap();
    l.set_font_family(id, "Inter").unwrap();
    assert!(l.set_font_size(id, 0.0).is_err());
    assert!(l.set_font_size(id, f64::NAN).is_err());
    assert!(l.set_font_size(id, 5000.0).is_err());
}

#[test]
fn frames_move_resize_and_go() {
    let mut l = a4();
    let id = l.add_text_frame(0, Rect::new(72.0, 72.0, 272.0, 172.0), "x").unwrap();
    l.move_item(id, 10.0, 20.0).unwrap();
    let r = l.item(0, id).unwrap().rect;
    assert!((r.x0 - 82.0).abs() < 0.5 && (r.y0 - 92.0).abs() < 0.5, "{r:?}");
    l.resize_item(0, id, Rect::new(82.0, 92.0, 382.0, 292.0)).unwrap();
    let r = l.item(0, id).unwrap().rect;
    assert!((r.width() - 300.0).abs() < 0.5 && (r.height() - 200.0).abs() < 0.5, "{r:?}");
    assert!(l.move_item(id, f64::INFINITY, 0.0).is_err());
    assert!(l.resize_item(0, id, Rect::new(0.0, 0.0, 1.0, 1.0)).is_err(), "too small");
    l.delete(id).unwrap();
    assert!(l.item(0, id).is_none());
}

#[test]
fn every_change_undoes_and_redoes() {
    let mut l = a4();
    let id = l.add_text_frame(0, Rect::new(72.0, 72.0, 272.0, 172.0), "x").unwrap();
    l.move_item(id, 50.0, 0.0).unwrap();
    assert!(l.undo());
    assert!((l.item(0, id).unwrap().rect.x0 - 72.0).abs() < 0.5);
    assert!(l.redo());
    assert!((l.item(0, id).unwrap().rect.x0 - 122.0).abs() < 0.5);
    assert!(l.undo() && l.undo());
    assert!(l.item(0, id).is_none(), "the frame's creation undone too");
}

#[test]
fn an_image_fills_its_frame_and_bad_bytes_leave_nothing() {
    let mut l = a4();
    let id = l.add_image(0, Rect::new(72.0, 72.0, 272.0, 272.0), &png(), "red.png").unwrap();
    assert_eq!(l.item(0, id).map(|i| i.kind), Some(ItemKind::Image));
    let before = l.items(0).len();
    assert!(l.add_image(0, Rect::new(72.0, 300.0, 272.0, 500.0), b"not an image", "x.png").is_err());
    assert!(l.add_image(0, Rect::new(72.0, 300.0, 272.0, 500.0), &[], "x.png").is_err());
    assert_eq!(l.items(0).len(), before, "no empty frame left behind");
}

#[test]
fn pages_render_at_the_asked_scale_and_refuse_absurd_ones() {
    let mut l = a4();
    l.add_text_frame(0, Rect::new(72.0, 72.0, 400.0, 200.0), "Texte").unwrap();
    let img = l.render_page(0, 0.5).unwrap();
    assert_eq!((img.width, img.height), (298, 421));
    assert_eq!(img.rgba.len(), img.width as usize * img.height as usize * 4);
    assert_eq!(img.rgba.get(..4), Some(&[255u8, 255, 255, 255][..]), "white paper");
    for scale in [0.0, -1.0, f64::NAN, f64::INFINITY, 1000.0] {
        assert!(l.render_page(0, scale).is_none(), "{scale}");
    }
    assert!(l.render_page(9, 1.0).is_none());
}

#[test]
fn pages_are_added_and_bad_pages_and_rects_are_refused() {
    let mut l = a4();
    assert_eq!(l.add_page().unwrap(), 1);
    assert_eq!(l.page_count(), 2);
    let id = l.add_text_frame(1, Rect::new(72.0, 72.0, 200.0, 200.0), "page 2").unwrap();
    assert!(l.item(1, id).is_some() && l.item(0, id).is_none());
    assert!(l.add_text_frame(7, Rect::new(72.0, 72.0, 200.0, 200.0), "x").is_err());
    assert!(l.add_text_frame(0, Rect::new(f64::NAN, 0.0, 10.0, 10.0), "x").is_err());
    assert!(l.add_text_frame(0, Rect::new(0.0, 0.0, 1e12, 10.0), "x").is_err());
    assert!(l.add_text_frame(0, Rect::new(10.0, 10.0, 11.0, 11.0), "x").is_err());
    assert!(l.text(424242).is_none() && l.set_text(424242, "x").is_err());
}

#[test]
fn the_document_exports_as_a_pdf_that_says_nadspdf_made_it() {
    let mut l = a4();
    l.add_text_frame(0, Rect::new(72.0, 72.0, 400.0, 200.0), "Exporté par NadsPDF").unwrap();
    let pdf = l.export_pdf().unwrap();
    assert!(pdf.starts_with(b"%PDF-"));
    let text = String::from_utf8_lossy(&pdf);
    assert!(text.contains("NadsPDF") && !text.contains("DesignCraft"), "producer");
}

#[test]
fn the_sample_lays_out_and_exports() {
    let mut l = Layout::sample().unwrap();
    assert_eq!(l.page_count(), 4);
    assert!(!l.items(1).is_empty());
    let pdf = l.export_pdf().unwrap();
    assert!(pdf.starts_with(b"%PDF-"), "{} bytes", pdf.len());
    // To look at it: NADSPDF_LAYOUT_SAMPLE=out.pdf cargo test -p pdfcraft-layout
    if let Some(path) = std::env::var_os("NADSPDF_LAYOUT_SAMPLE") {
        std::fs::write(path, &pdf).unwrap_or_default();
    }
}

#[test]
fn base64_matches_the_rfc_vectors() {
    for (i, o) in [("", ""), ("f", "Zg=="), ("fo", "Zm8="), ("foo", "Zm9v"), ("foob", "Zm9vYg=="), ("fooba", "Zm9vYmE="), ("foobar", "Zm9vYmFy")] {
        assert_eq!(base64(i.as_bytes()), o);
    }
}

#[test]
fn a_saved_document_reopens_the_same_and_is_no_longer_unsaved() {
    let mut l = a4();
    assert!(!l.is_dirty(), "a new document has nothing to save");
    let id = l.add_text_frame(0, Rect::new(72.0, 72.0, 300.0, 200.0), "Gardé à l'enregistrement").unwrap();
    assert!(l.is_dirty());
    let revision = l.revision();
    let bytes = l.save_bytes().unwrap();
    assert!(l.is_dirty(), "not saved until the file is written");
    l.mark_saved(revision);
    assert!(!l.is_dirty());
    assert!(is_layout_file("anything", &bytes), "recognised by its content");
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(&bytes)).unwrap();
    let mut meta = String::new();
    std::io::Read::read_to_string(&mut z.by_name("meta.json").unwrap(), &mut meta).unwrap();
    assert!(meta.contains("NadsPDF") && !meta.contains("DesignCraft"), "the file names NadsPDF as its maker: {meta}");
    let (mut back, warnings) = Layout::open(&format!("Affiche.{EXTENSION}"), &bytes).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    assert!(!back.is_dirty());
    assert_eq!(back.page_count(), 1);
    let it = back.items(0);
    assert_eq!(it.len(), 1);
    assert_eq!(back.text(it[0].id).as_deref(), Some("Gardé à l'enregistrement"));
    assert_eq!(it[0].rect, l.item(0, id).unwrap().rect);
}

#[test]
fn an_idml_package_opens_as_a_layout() {
    let mut l = a4();
    l.add_text_frame(0, Rect::new(72.0, 72.0, 300.0, 200.0), "Depuis InDesign").unwrap();
    let v = l.session.execute("file.exportIdml", &json!({})).unwrap();
    let idml = designcraft_engine_base64_decode(v["base64"].as_str().unwrap());
    assert!(is_layout_file("Brochure.idml", &idml) && is_layout_file("Brochure.IDML", b""));
    let (mut back, _) = Layout::open("Brochure.idml", &idml).unwrap();
    let texts: Vec<String> = back
        .items(0)
        .iter()
        .filter_map(|i| i.kind.eq(&ItemKind::Text).then_some(i.id))
        .collect::<Vec<_>>()
        .into_iter()
        .filter_map(|id| back.text(id))
        .collect();
    assert!(texts.iter().any(|t| t.contains("Depuis InDesign")), "{texts:?}");
}

#[test]
fn files_that_are_not_layouts_are_refused_in_words() {
    assert!(!is_layout_file("report.pdf", b"%PDF-1.7"));
    for (name, bytes) in [("x.nadsmep", &b"not a zip"[..]), ("x.idml", b"PK\x03\x04broken"), ("x.nadsmep", b"")] {
        match Layout::open(name, bytes) {
            Err(e) => assert!(!e.to_string().contains("DesignCraft"), "{e}"),
            Ok(_) => panic!("{name} opened"),
        }
    }
}

/// Base64 back to bytes (tests only: the engine returns IDML as base64).
fn designcraft_engine_base64_decode(s: &str) -> Vec<u8> {
    let val = |c: u8| match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    };
    let bits: Vec<u8> = s.bytes().filter_map(val).collect();
    let mut out = Vec::new();
    for c in bits.chunks(4) {
        let n = c.iter().enumerate().fold(0u32, |n, (i, v)| n | (u32::from(*v) << (18 - 6 * i)));
        out.push((n >> 16) as u8);
        if c.len() > 2 {
            out.push((n >> 8) as u8);
        }
        if c.len() > 3 {
            out.push(n as u8);
        }
    }
    out
}

/// A one-page 300 × 200 pt PDF drawing a red rectangle (the artwork behind the words).
fn artwork_pdf() -> Vec<u8> {
    let content = b"1 0 0 rg 20 20 100 50 re f";
    let objs = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] /Contents 4 0 R >>".to_string(),
        format!("<< /Length {} >>\nstream\n{}\nendstream", content.len(), String::from_utf8_lossy(content)),
    ];
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offs = Vec::new();
    for (i, o) in objs.iter().enumerate() {
        offs.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{o}\nendobj\n", i + 1).as_bytes());
    }
    let x = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).as_bytes());
    for o in offs {
        out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{x}\n%%EOF\n", objs.len() + 1).as_bytes());
    out
}

#[test]
fn a_pdf_becomes_artwork_behind_editable_words() {
    let block = |text: &str, rect: [f64; 4], bold: bool, color: [f64; 3]| PdfBlock {
        text: text.into(),
        rect,
        style: TextStyle { size: 14.0, bold, color },
        rotation: 0.0,
    };
    let pages = [PdfPage {
        width: 300.0,
        height: 200.0,
        blocks: vec![
            block("Titre en gras", [20.0, 160.0, 160.0, 176.0], true, [0.8, 0.1, 0.1]),
            block("Un paragraphe ordinaire", [20.0, 120.0, 220.0, 134.0], false, [0.0, 0.0, 0.0]),
            block("   ", [20.0, 100.0, 60.0, 110.0], false, [0.0; 3]),
            block("hors champ", [f64::NAN, 0.0, 1.0, 1.0], false, [0.0; 3]),
        ],
    }];
    let mut l = Layout::from_pdf(&artwork_pdf(), "Affiche.pdf", &pages).unwrap();
    assert_eq!(l.page_count(), 1);
    assert_eq!(l.page_size(0).map(|(w, h)| (w.round(), h.round())), Some((300.0, 200.0)));
    let items = l.items(0);
    assert_eq!(items.iter().filter(|i| i.kind == ItemKind::Image).count(), 1, "the artwork: {items:?}");
    let texts: Vec<Item> = items.iter().filter(|i| i.kind == ItemKind::Text).cloned().collect();
    assert_eq!(texts.len(), 2, "blank and broken blocks are left out: {texts:?}");
    let title = texts.iter().find(|i| l.text(i.id).as_deref() == Some("Titre en gras")).cloned().unwrap();
    // Placed where the PDF drew it, the page being y-down: 200 - 176 = 24 pt from the top.
    assert!((title.rect.x0 - 20.0).abs() < 0.5 && (title.rect.y0 - 22.6).abs() < 1.5, "{:?}", title.rect);
    // Editable: the words change like any frame's.
    l.set_text(title.id, "Nouveau titre").unwrap();
    assert_eq!(l.text(title.id).as_deref(), Some("Nouveau titre"));
    // The artwork is locked: a click on it finds the words, not the picture.
    let pdf = l.export_pdf().unwrap();
    assert!(pdf.starts_with(b"%PDF-"));
    assert!(Layout::from_pdf(b"", "x.pdf", &pages).is_err());
    assert!(Layout::from_pdf(&artwork_pdf(), "x.pdf", &[]).is_err());
}
