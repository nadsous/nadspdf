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
