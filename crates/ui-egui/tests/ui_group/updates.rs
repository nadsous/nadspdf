//! Help ▸ Check for updates (issue #28), with stand-in release sources (no network).

use std::sync::Arc;

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use pdfcraft_ui_egui::PdfCraftApp;
use std::sync::atomic::{AtomicUsize, Ordering};

use pdfcraft_ui_egui::updates::{Installer, Release, UpdateInstaller, UpdateSource, is_newer};

fn release(v: String) -> Release {
    Release { url: format!("https://github.com/nadsous/nadspdf/releases/tag/{v}"), version: v, installer: None }
}

fn source(answer: Result<&str, &str>) -> UpdateSource {
    let answer = answer.map(str::to_string).map_err(str::to_string);
    Arc::new(move || answer.clone().map(release))
}

/// A source answering `v99.0.0` that counts its calls.
fn counted_source(calls: Arc<AtomicUsize>, installer: bool) -> UpdateSource {
    Arc::new(move || {
        calls.fetch_add(1, Ordering::SeqCst);
        let mut r = release("v99.0.0".into());
        if installer {
            r.installer = Some(Installer {
                name: "nadspdf-99.0.0-windows-x64.msi".into(),
                url: "https://github.com/nadsous/nadspdf/releases/download/v99.0.0/nadspdf-99.0.0-windows-x64.msi".into(),
                sha256: "ab".repeat(32),
            });
        }
        Ok(r)
    })
}

/// An app whose launch check is off, so only the command asks.
fn harness(answer: Result<&str, &str>) -> Harness<'static, PdfCraftApp> {
    Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_eframe(move |_cc| {
        let mut app = PdfCraftApp::new();
        app.set_option("language", "en").unwrap();
        app.restore(r#"{"updates": {"auto": false}}"#);
        app.update_source = Some(source(answer));
        app
    })
}

/// An app with the launch check as saved in `settings`.
fn launching(settings: &'static str, source: UpdateSource, installer: Option<UpdateInstaller>, store: bool) -> Harness<'static, PdfCraftApp> {
    Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_eframe(move |_cc| {
        let mut app = PdfCraftApp::new();
        app.set_option("language", "en").unwrap();
        app.restore(settings);
        app.update_source = Some(source);
        app.update_installer = installer;
        app.updates_from_store = store;
        app
    })
}

/// Run frames until the background check has reported (or give up).
fn settle(h: &mut Harness<'static, PdfCraftApp>) {
    for _ in 0..200 {
        h.run_steps(2);
        if h.query_by_label_contains("Checking for a newer version").is_none() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    h.run_steps(2);
}

#[test]
fn versions_compare_by_number() {
    assert!(is_newer("v0.2.0", "0.1.1"));
    assert!(is_newer("v0.1.10", "0.1.9"));
    assert!(is_newer("1", "0.9.9"));
    assert!(!is_newer("v0.1.1", "0.1.1"));
    assert!(!is_newer("v0.1.0", "0.1.1"));
    assert!(!is_newer("v0.1.1-beta.2", "0.1.1"), "suffixes are ignored");
    assert!(!is_newer("nightly", "0.1.1"), "a tag that isn't a version is never newer");
    assert!(!is_newer("v1.2.3.4", "0.1.1"));
    assert!(!is_newer("v99999999999999999999.0.0", "0.1.1"), "out of range");
}

#[test]
fn a_newer_release_is_offered_for_download() {
    let mut h = harness(Ok("v99.0.0"));
    h.state_mut().execute("help.check_updates");
    settle(&mut h);
    h.get_by_label_contains("NadsPDF 99.0.0 is available");
    h.get_by_label("Download");
    h.get_by_label("Later").click();
    h.run_steps(3);
    assert!(h.query_by_label_contains("is available").is_none(), "Later closes the dialog");
}

#[test]
fn an_up_to_date_or_failed_check_says_so() {
    let mut h = harness(Ok(concat!("v", env!("CARGO_PKG_VERSION"))));
    h.state_mut().execute("help.check_updates");
    settle(&mut h);
    h.get_by_label_contains("is up to date");
    assert!(h.query_by_label("Download").is_none());

    let mut h = harness(Err("couldn't reach GitHub"));
    h.state_mut().execute("help.check_updates");
    settle(&mut h);
    h.get_by_label_contains("Couldn't check for updates: couldn't reach GitHub");
}

#[test]
fn a_launch_check_runs_once_a_day_and_speaks_only_for_a_new_version() {
    // Never checked: the launch check finds v99 and shows the dialog by itself.
    let calls = Arc::new(AtomicUsize::new(0));
    let mut h = launching("{}", counted_source(calls.clone(), false), None, false);
    settle(&mut h);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    h.get_by_label_contains("NadsPDF 99.0.0 is available");
    // Its time is saved: the next launch the same day doesn't ask.
    let saved: serde_json::Value = serde_json::from_str(&h.state().persist()).unwrap();
    let at = saved["updates"]["checked_at"].as_u64().unwrap();
    assert!(at > 0);
    let settings: &'static str = Box::leak(saved.to_string().into_boxed_str());
    let again = Arc::new(AtomicUsize::new(0));
    let mut h = launching(settings, counted_source(again.clone(), false), None, false);
    settle(&mut h);
    assert_eq!(again.load(Ordering::SeqCst), 0);
    assert!(h.query_by_label_contains("is available").is_none());

    // Up to date or unreachable: nothing shows.
    for answer in [Ok(concat!("v", env!("CARGO_PKG_VERSION"))), Err("couldn't reach GitHub")] {
        let mut h = launching("{}", source(answer), None, false);
        settle(&mut h);
        assert!(h.query_by_label_contains("is up to date").is_none(), "{answer:?}");
        assert!(h.query_by_label_contains("Couldn't check").is_none(), "{answer:?}");
    }
}

#[test]
fn the_launch_check_can_be_turned_off_and_store_copies_never_check() {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut h = launching(r#"{"updates": {"auto": false}}"#, counted_source(calls.clone(), false), None, false);
    settle(&mut h);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    h.state_mut().execute("help.check_updates");
    settle(&mut h);
    assert_eq!(calls.load(Ordering::SeqCst), 1, "the command still asks");

    let mut h = launching("{}", counted_source(calls.clone(), false), None, true);
    settle(&mut h);
    h.state_mut().execute("help.check_updates");
    settle(&mut h);
    assert_eq!(calls.load(Ordering::SeqCst), 1, "a Store copy never asks");
    h.get_by_label_contains("comes from the Microsoft Store");
}

#[test]
fn update_installs_the_release_and_quits() {
    let calls = Arc::new(AtomicUsize::new(0));
    let installed = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let log = installed.clone();
    let installer: UpdateInstaller = Arc::new(move |i: &Installer| {
        log.lock().map_err(|e| e.to_string())?.push(i.name.clone());
        Ok(())
    });
    let mut h = launching(r#"{"updates": {"auto": false}}"#, counted_source(calls, true), Some(installer), false);
    h.state_mut().execute("help.check_updates");
    settle(&mut h);
    h.get_by_label_contains("install the new version in place of this one");
    assert!(h.query_by_label("Download").is_none());
    h.get_by_label("Update").click();
    for _ in 0..100 {
        h.run_steps(2);
        if h.query_by_label_contains("is available").is_none() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(*installed.lock().unwrap(), ["nadspdf-99.0.0-windows-x64.msi"]);
    assert!(h.query_by_label_contains("is available").is_none(), "the dialog closes for the quit");
}

#[test]
fn a_failed_download_says_so_and_offers_the_release_page() {
    let calls = Arc::new(AtomicUsize::new(0));
    let installer: UpdateInstaller = Arc::new(|_: &Installer| Err("the download stopped".into()));
    let mut h = launching(r#"{"updates": {"auto": false}}"#, counted_source(calls, true), Some(installer), false);
    h.state_mut().execute("help.check_updates");
    settle(&mut h);
    h.get_by_label("Update").click();
    for _ in 0..100 {
        h.run_steps(2);
        if h.query_by_label_contains("Couldn't install the update").is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    h.get_by_label_contains("Couldn't install the update: the download stopped");
    h.get_by_label("Download");
}
