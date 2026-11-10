//! NadsPDF's Mise en page tab in the real shell: opened from the mode bar, a frame drawn on the
//! page, its text changed, undone, and the document exported into a PDF tab.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use pdfcraft_ui_egui::PdfCraftApp;
use pdfcraft_ui_egui::layout_ui::Tool;

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
fn the_mode_bar_opens_the_tab_and_a_new_document_starts_empty() {
    let mut h = harness();
    h.get_by_label("Page design").click();
    h.run_steps(3);
    assert!(h.state().layout_showing());
    h.get_by_label("New A4 document").click();
    h.run_steps(3);
    assert_eq!(h.state().layout_tab.page_count(), 1);
    assert!(h.state().layout_tab.items().is_empty());
    // Its tab is in the strip; Home leaves it open but not shown.
    h.get_by_label("Home").click();
    h.run_steps(2);
    assert!(!h.state().layout_showing() && h.state().layout_tab.open);
}

#[test]
fn a_frame_is_drawn_typed_in_undone_and_exported() {
    let mut h = harness();
    h.state_mut().new_layout(pdfcraft_layout::Paper::A4);
    h.run_steps(3);
    h.get_by_label("Text frame").click();
    h.run_steps(2);
    assert_eq!(h.state().layout_tab.tool, Tool::Text);
    // Drag across the middle of the page area.
    let (from, to) = (egui::pos2(400.0, 300.0), egui::pos2(650.0, 420.0));
    h.hover_at(from);
    h.run_steps(1);
    h.drag_at(from);
    h.run_steps(1);
    for i in 1..=6 {
        h.hover_at(from + (to - from) * (i as f32 / 6.0));
        h.run_steps(1);
    }
    h.drop_at(to);
    h.run_steps(3);
    let items = h.state().layout_tab.items();
    assert_eq!(items.len(), 1, "one frame drawn");
    let id = items[0].id;
    assert!(items[0].rect.width() > 50.0 && items[0].rect.height() > 20.0, "{:?}", items[0].rect);
    assert_eq!(h.state().layout_tab.selected, Some(id), "the new frame is selected");
    assert_eq!(h.state().layout_tab.tool, Tool::Select, "back to Select");

    let text = h.state_mut().layout_tab.document_mut().and_then(|d| d.text(id));
    assert_eq!(text.as_deref(), Some("Type your text here"));
    h.state_mut().layout_tab.document_mut().map(|d| d.set_text(id, "Bonjour")).unwrap().unwrap();
    assert!(h.state_mut().execute("edit.undo"));
    h.run_steps(2);
    let text = h.state_mut().layout_tab.document_mut().and_then(|d| d.text(id));
    assert_eq!(text.as_deref(), Some("Type your text here"), "Undo goes to the Mise en page document");

    // Export opens the PDF in a NadsPDF tab.
    h.get_by_label("Export as PDF").click();
    h.run_steps(4);
    assert_eq!(h.state().views.len(), 1);
    assert!(h.state().active.is_some() && !h.state().layout_showing());
    assert!(h.state().layout_tab.open, "the Mise en page tab stays open");
}

#[test]
fn the_sample_command_opens_the_magazine_and_delete_removes_a_frame() {
    let mut h = harness();
    assert!(h.state_mut().execute("layout.sample"));
    h.run_steps(3);
    assert_eq!(h.state().layout_tab.page_count(), 4);
    let first = h.state().layout_tab.items().first().map(|i| i.id).unwrap();
    let before = h.state().layout_tab.items().len();
    h.state_mut().layout_tab.selected = Some(first);
    h.run_steps(2);
    h.get_by_label("Delete").click();
    h.run_steps(3);
    assert_eq!(h.state().layout_tab.items().len(), before - 1);
    assert_eq!(h.state().layout_tab.selected, None);
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
    h.run_steps(6);
    h.render().unwrap().save(dir.join("1-vide.png")).unwrap();
    assert!(h.state_mut().execute("layout.sample"));
    h.run_steps(6);
    h.state_mut().layout_tab.page = 1;
    h.run_steps(6);
    let pick = h.state().layout_tab.items().into_iter().find(|i| i.kind == pdfcraft_layout::ItemKind::Text).map(|i| i.id);
    h.state_mut().layout_tab.selected = pick;
    h.run_steps(6);
    h.render().unwrap().save(dir.join("2-magazine.png")).unwrap();
}
