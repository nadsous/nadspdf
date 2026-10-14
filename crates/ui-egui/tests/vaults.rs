//! Vaults on Home: creating one, adding files (picker, drop, current document), opening, moving
//! a file between vaults, missing files, deleting, and the setting surviving a restart. Files are
//! never moved, copied or deleted by any of it.

use std::path::PathBuf;

use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use pdfcraft_ui_egui::PdfCraftApp;

/// A one-page PDF.
const FIXTURE: &[u8] = b"%PDF-1.7
1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj
2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj
3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 300 400] >> endobj
trailer << /Root 1 0 R >>
%%EOF";

/// A fresh folder under the temp dir, removed when dropped.
struct TempFolder(PathBuf);

impl TempFolder {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("pdfcraft-vaults-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn file(&self, name: &str) -> String {
        let path = self.0.join(name);
        std::fs::write(&path, FIXTURE).unwrap();
        path.to_string_lossy().into_owned()
    }
}

impl Drop for TempFolder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn harness(setup: impl FnOnce(&mut PdfCraftApp) + 'static) -> Harness<'static, PdfCraftApp> {
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 1400.0)).build_eframe(move |_cc| {
        let mut app = PdfCraftApp::new();
        app.set_option("language", "en").unwrap();
        app.run_inline = true;
        setup(&mut app);
        app
    });
    h.run_steps(4);
    h
}

#[test]
fn creating_a_vault_and_adding_a_file_through_the_picker() {
    let dir = TempFolder::new("create");
    let file = dir.file("contract.pdf");
    let pick = file.clone();
    let mut h = harness(move |app| app.pick_override = Some(vec![pick]));
    // New vault ▸ name it ▸ Create vault.
    h.get_by_label("New vault").click();
    h.run_steps(2);
    let name = h.get_by_role_and_label(Role::TextInput, "Vault name");
    name.focus();
    name.type_text("Work");
    h.run_steps(1);
    h.get_by_label("Create vault").click();
    h.run_steps(2);
    let id = {
        let app = h.state();
        assert_eq!(app.vaults.data.list.len(), 1);
        assert_eq!(app.vaults.data.list[0].name, "Work");
        assert!(app.vaults.dialog.is_none());
        app.vaults.data.list[0].id
    };
    assert_eq!(h.state().vaults.selected, Some(id), "the new vault shows");
    // Add files… on the open vault uses the picker.
    h.get_by_label("Add files…").click();
    h.run_steps(4);
    assert_eq!(h.state().vaults.data.list[0].files, std::slice::from_ref(&file));
    // The file is listed and opens on click; it never moved.
    h.get_by_label("contract.pdf").click();
    h.run_steps(3);
    assert_eq!(h.state().views.len(), 1, "the file opens in a tab");
    assert!(std::path::Path::new(&file).exists());
}

#[test]
fn files_dropped_on_a_vault_chip_join_it_instead_of_opening() {
    let dir = TempFolder::new("drop");
    let file = dir.file("scan.pdf");
    let mut h = harness(|app| {
        app.set_option("vault", "Scans").unwrap();
        app.vaults.selected = None;
    });
    h.run_steps(2);
    // The Home chip (the All tools panel lists vaults as well, in shorter rows).
    let chip = h.get_all_by_label("Scans").map(|n| n.rect()).find(|r| r.height() > 50.0).unwrap().center();
    h.hover_at(chip);
    h.run_steps(2);
    #[derive(Debug)]
    struct Dropped(PathBuf);
    impl egui::DroppedFile for Dropped {
        fn path(&self) -> &std::path::Path {
            &self.0
        }
        fn bytes(&self) -> Result<Vec<u8>, String> {
            std::fs::read(&self.0).map_err(|e| e.to_string())
        }
    }
    h.input_mut().dropped_files.push(std::sync::Arc::new(Dropped(PathBuf::from(&file))));
    h.run_steps(3);
    assert!(h.state().views.is_empty(), "the dropped file is not opened");
    assert_eq!(h.state().vaults.data.list[0].files, [file]);
}

#[test]
fn a_file_moves_between_vaults_and_a_missing_one_is_shown() {
    let dir = TempFolder::new("move");
    let a = dir.file("a.pdf");
    let gone = dir.0.join("gone.pdf").to_string_lossy().into_owned();
    let (a2, gone2) = (a.clone(), gone.clone());
    let mut h = harness(move |app| {
        app.set_option("vault", "Taxes").unwrap();
        app.set_option("vault", "Home").unwrap();
        let taxes = app.vaults.data.find_by_name("Taxes").unwrap();
        app.add_to_vault(taxes, &[a2, gone2]);
        app.show_vault(taxes);
    });
    h.run_steps(3);
    h.get_by_label("Locate…");
    let (taxes, home) = {
        let v = &h.state().vaults.data;
        (v.find_by_name("Taxes").unwrap(), v.find_by_name("Home").unwrap())
    };
    assert!(h.state_mut().move_to_vault(taxes, home, &a));
    assert_eq!(h.state().vaults.data.get(home).unwrap().files, std::slice::from_ref(&a));
    assert_eq!(h.state().vaults.data.get(taxes).unwrap().files, [gone]);
    assert!(std::path::Path::new(&a).exists(), "moving between vaults never touches the file");
}

#[test]
fn deleting_a_vault_asks_first_and_keeps_its_files() {
    let dir = TempFolder::new("delete");
    let file = dir.file("keep.pdf");
    let f = file.clone();
    let mut h = harness(move |app| app.set_option("vault", &format!("Old={f}")).unwrap());
    h.run_steps(2);
    h.get_by_label("Delete").click();
    h.run_steps(2);
    assert_eq!(h.state().vaults.data.list.len(), 1, "nothing is deleted before confirming");
    h.get_by_label("Delete vault").click();
    h.run_steps(2);
    assert!(h.state().vaults.data.list.is_empty());
    assert!(std::path::Path::new(&file).exists(), "deleting a vault never deletes its files");
}

#[test]
fn the_current_document_joins_a_vault_from_the_command() {
    let dir = TempFolder::new("current");
    let file = dir.file("open.pdf");
    let f = file.clone();
    let mut h = harness(move |app| {
        app.set_option("vault", "Inbox").unwrap();
        app.open_path(&f);
    });
    h.run_steps(2);
    assert!(h.state_mut().execute("vault.add_current"));
    h.run_steps(2);
    // The panel's vault list shows too; the dialog's row is drawn last, above it.
    h.get_all_by_label("Inbox").last().unwrap().click();
    h.run_steps(2);
    assert_eq!(h.state().vaults.data.list[0].files, [file]);
}

#[test]
fn vaults_survive_a_restart_and_bad_settings_are_ignored() {
    let root = if cfg!(windows) { "C:\\" } else { "/" };
    let mut a = PdfCraftApp::new();
    a.set_option("vault", &format!("Work={root}w.pdf|relative.pdf")).unwrap();
    let mut b = PdfCraftApp::new();
    b.restore(&a.persist());
    assert_eq!(b.vaults.data.list.len(), 1);
    assert_eq!(b.vaults.data.list[0].name, "Work");
    assert_eq!(b.vaults.data.list[0].files, [format!("{root}w.pdf")], "relative paths are never kept");
    b.restore(r#"{"vaults": [{"name": 5}, "x", {"name": "ok", "files": "nope", "color": [1]}]}"#);
    assert_eq!(b.vaults.data.list.len(), 1);
    assert!(b.vaults.data.list[0].files.is_empty());
}
