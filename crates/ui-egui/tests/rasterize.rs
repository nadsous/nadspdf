//! Compress ▸ Rasterize pages (strong compression) in the real shell.

use egui_kittest::kittest::Queryable;
use pdfcraft_engine::rasterize::RasterLevel;
use pdfcraft_ui_egui::PdfCraftApp;

/// A 600 × 400 px PNG of noise: PNG can't compress it, JPEG can.
fn noise_png() -> Vec<u8> {
    let mut seed = 0x9e37_79b9_u32;
    let px: Vec<u8> = (0..600 * 400 * 3)
        .map(|_| {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            (seed >> 24) as u8
        })
        .collect();
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, 600, 400);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().unwrap().write_image_data(&px).unwrap();
    out
}

#[test]
fn rasterize_dialog_picks_a_level_and_saves_a_smaller_copy() {
    let dir = std::env::temp_dir().join(format!("pdfcraft-rasterize-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let out = dir.join("compressed.pdf");
    let _ = std::fs::remove_file(&out);
    let target = out.to_string_lossy().into_owned();
    let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_eframe(move |_cc| {
        let mut app = PdfCraftApp::new();
        app.set_option("language", "en").unwrap();
        app.open_bytes("scan.png", None, noise_png()).unwrap();
        app.save_override = Some(target);
        app
    });
    h.run_steps(2);
    assert!(h.state_mut().execute("optimize.rasterize"));
    h.run_steps(3);
    if let Ok(path) = std::env::var("PDFCRAFT_RASTERIZE_SHOT") {
        h.render().unwrap().save(path).unwrap();
    }
    h.get_by_label_contains("Text is no longer selectable");
    h.get_by_label("Strong").click();
    h.run_steps(1);
    h.get_by_label("Compress").click();
    h.run_steps(1);
    let start = std::time::Instant::now();
    while h.state().optimize_run.is_some() && start.elapsed() < std::time::Duration::from_secs(60) {
        h.run_steps(1);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    h.run_steps(2);
    assert_eq!(h.state().raster_level, RasterLevel::Strong, "the choice is remembered");
    let bytes = std::fs::read(&out).unwrap();
    assert!(bytes.starts_with(b"%PDF-"));
    assert!(bytes.len() * 2 < h.state().session.docs()[0].bytes.len(), "{} bytes", bytes.len());
    assert!(h.state().session.docs()[0].dirty, "the open document is unchanged");
    h.get_by_label_contains("% smaller");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn rasterizing_a_small_text_file_saves_nothing() {
    let mut app = PdfCraftApp::new();
    app.set_option("language", "en").unwrap();
    app.open_bytes("notes.txt", None, b"hello".to_vec()).unwrap();
    let dir = std::env::temp_dir().join(format!("pdfcraft-rasterize-small-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let out = dir.join("x.pdf");
    app.save_override = Some(out.to_string_lossy().into_owned());
    app.run_inline = true;
    app.start_rasterize(RasterLevel::Light);
    assert!(app.optimize_run.is_none());
    assert!(!out.exists());
    assert!(
        app.toast.as_ref().is_some_and(|t| t.0.starts_with("Rasterizing wouldn't make this file smaller")),
        "{:?}",
        app.toast.as_ref().map(|t| &t.0)
    );
}
