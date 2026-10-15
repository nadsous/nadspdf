//! `pdfcraft-cli extensions`: listing an extensions folder (usable and broken folders, approval
//! state) and checking one extension folder, without starting any program.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

const BIN: &str = env!("CARGO_BIN_EXE_pdfcraft-cli");

fn run(args: &[&str]) -> Output {
    Command::new(BIN).args(args).output().unwrap()
}

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("pdfcraft-cli-extensions-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn extension(root: &Path, folder: &str, manifest: &str) -> PathBuf {
    let dir = root.join(folder);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("ext.exe"), b"not run by these tests").unwrap();
    std::fs::write(dir.join("manifest.json"), manifest).unwrap();
    dir
}

const GOOD: &str =
    r#"{"protocol":1,"id":"com.example.erp","name":"ERP","version":"1.2.0","executable":"ext.exe","permissions":["read_text","banner"]}"#;

fn json(out: &Output) -> Value {
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    serde_json::from_slice(&out.stdout).unwrap()
}

#[test]
fn check_describes_a_usable_extension() {
    let scratch = Scratch::new("check");
    let dir = extension(&scratch.0, "erp", GOOD);
    let v = json(&run(&["extensions", "check", dir.to_str().unwrap()]));
    assert_eq!(v["ok"], true);
    assert_eq!(v["id"], "com.example.erp");
    assert_eq!(v["permissions"], serde_json::json!(["read_text", "banner"]));
    assert_eq!(v["fingerprint"].as_str().map(str::len), Some(64));
    assert!(v.get("approved").is_none(), "check doesn't read approvals");
}

#[test]
fn check_fails_with_the_reason() {
    let scratch = Scratch::new("broken");
    let dir = extension(&scratch.0, "erp", &GOOD.replace("ext.exe", "../outside.exe"));
    let out = run(&["extensions", "check", dir.to_str().unwrap()]);
    assert!(!out.status.success());
    assert!(out.stdout.is_empty());
    assert!(String::from_utf8_lossy(&out.stderr).contains("relative path inside the extension folder"));
}

#[test]
fn list_shows_usable_and_broken_folders_and_their_approval() {
    let scratch = Scratch::new("list");
    let folder = scratch.0.join("Extensions");
    extension(&folder, "a-erp", GOOD);
    extension(&folder, "b-typo", &GOOD.replace("\"permissions\"", "\"permisions\""));
    let v = json(&run(&["extensions", "list", "--dir", folder.to_str().unwrap()]));
    let list = v["extensions"].as_array().unwrap();
    assert_eq!(list.len(), 2);
    assert_eq!(list[0]["id"], "com.example.erp");
    assert_eq!(list[0]["approved"], false);
    assert_eq!(list[1]["ok"], false);
    assert!(list[1]["error"].as_str().unwrap().contains("permisions"));
    assert!(v["approvals"].as_str().unwrap().ends_with("extension-approvals.json"));
}

#[test]
fn a_missing_folder_lists_nothing() {
    let scratch = Scratch::new("missing");
    let v = json(&run(&["extensions", "list", "--dir", scratch.0.join("nope").to_str().unwrap()]));
    assert_eq!(v["extensions"], serde_json::json!([]));
}

#[test]
fn bad_usage_fails() {
    for args in [&["extensions"][..], &["extensions", "start"], &["extensions", "check"], &["extensions", "list", "--force"]] {
        let out = run(args);
        assert!(!out.status.success(), "{args:?}");
    }
}
