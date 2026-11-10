//! NadsPDF's Mise en page tab in the real shell: the page layout interface opened from the mode
//! bar, page layouts opened from NadsPDF's Open, and unsaved work kept when the tab closes.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use pdfcraft_ui_egui::PdfCraftApp;
use serde_json::json;

fn harness() -> Harness<'static, PdfCraftApp> {
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 900.0)).build_eframe(|_cc| {
        let mut app = PdfCraftApp::new();
        app.set_option("language", "en").unwrap();
        app
    });
    h.run_steps(4);
    h
}

#[test]
fn the_mode_bar_opens_the_tab_and_home_leaves_it_open() {
    let mut h = harness();
    h.get_by_label("Page design").click();
    h.run_steps(6);
    assert!(h.state().layout_showing());
    assert!(!h.state().layout_tab.has_document());
    h.get_by_label("Home").click();
    h.run_steps(2);
    assert!(!h.state().layout_showing() && h.state().layout_tab.open);
}

#[test]
fn the_sample_opens_in_the_interface_and_its_menus_run() {
    let mut h = harness();
    assert!(h.state_mut().execute("layout.sample"));
    h.run_steps(8);
    assert_eq!(h.state().layout_tab.document_count(), 1);
    // The engine's commands run through the interface.
    let pages = h.state_mut().layout_tab.run("document.inspect", json!({})).unwrap();
    assert!(pages.to_string().contains("pages"), "{pages}");
    // NadsPDF's look stays: the interface's text is in NadsPDF's interface font.
    let font = h.ctx.fonts(|f| f.definitions().families.get(&egui::FontFamily::Proportional).and_then(|s| s.first().cloned()));
    assert_eq!(font.as_deref(), Some("Onest"));
}

#[test]
fn a_page_layout_opens_from_nadspdf_open_and_unsaved_work_is_kept_on_close() {
    let dir = std::env::temp_dir().join(format!("nadspdf-page-design-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("Affiche.nadsmep");
    let mut doc = pdfcraft_layout::Layout::new(pdfcraft_layout::Paper::A4, 1).unwrap();
    doc.add_text_frame(0, pdfcraft_layout::Rect::new(72.0, 72.0, 300.0, 200.0), "Affiche").unwrap();
    std::fs::write(&file, doc.save_bytes().unwrap()).unwrap();

    let mut h = harness();
    h.state_mut().open_path(file.to_str().unwrap());
    h.run_steps(8);
    assert!(h.state().layout_showing());
    assert!(h.state().views.is_empty(), "no PDF tab");
    assert_eq!(h.state().layout_tab.document_count(), 1);
    assert_eq!(h.state().layout_tab.title(), "Affiche.nadsmep");
    assert!(!h.state().layout_tab.is_dirty());

    // A change makes it unsaved: closing once keeps it, twice discards it.
    h.state_mut().layout_tab.run("frame.create", json!({"rect": [72, 300, 300, 400], "content": "text", "text": "x"})).unwrap();
    h.run_steps(2);
    assert!(h.state().layout_tab.is_dirty());
    h.state_mut().request_close_layout_tab();
    h.run_steps(2);
    assert!(h.state().layout_tab.open && h.state().layout_tab.has_document(), "kept");
    h.state_mut().request_close_layout_tab();
    h.run_steps(2);
    assert!(!h.state().layout_tab.open);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_file_that_is_not_a_layout_is_refused_in_words() {
    let mut h = harness();
    let e = h.state_mut().open_bytes("broken.nadsmep", None, b"not a layout".to_vec()).unwrap_err();
    assert!(!e.contains("DesignCraft"), "{e}");
    assert!(!h.state().layout_tab.has_document());
}

/// Screenshots for review: `NADSPDF_PAGE_DESIGN_SHOTS=<folder> cargo test -p pdfcraft-ui-egui --test it page_design`.
#[test]
fn screenshots_when_asked() {
    let Some(dir) = std::env::var_os("NADSPDF_PAGE_DESIGN_SHOTS").map(std::path::PathBuf::from) else { return };
    let mut h = Harness::builder().with_size(egui::vec2(1600.0, 1000.0)).with_pixels_per_point(1.5).build_eframe(|_cc| {
        let mut app = PdfCraftApp::new();
        app.set_option("language", "fr").unwrap();
        app
    });
    h.run_steps(4);
    h.state_mut().open_layout_tab();
    h.run_steps(10);
    h.render().unwrap().save(dir.join("1-vide.png")).unwrap();
    assert!(h.state_mut().execute("layout.sample"));
    h.run_steps(30);
    h.render().unwrap().save(dir.join("2-magazine.png")).unwrap();
}

#[test]
fn quitting_with_an_unsaved_page_layout_asks_first() {
    let mut h = harness();
    assert!(h.state_mut().execute("layout.sample"));
    h.run_steps(6);
    h.state_mut().layout_tab.run("frame.create", json!({"rect": [72, 72, 200, 200], "content": "text", "text": "x"})).unwrap();
    // Home: the tab is not shown when the window is closed.
    h.state_mut().active = None;
    h.state_mut().layout_tab.focused = false;
    h.run_steps(2);
    h.input_mut().viewports.entry(egui::ViewportId::ROOT).or_default().events.push(egui::ViewportEvent::Close);
    h.run_steps(1);
    assert!(h.state().layout_showing(), "the window stays open on the unsaved page layout");
    assert!(h.state().layout_tab.has_document());
}
