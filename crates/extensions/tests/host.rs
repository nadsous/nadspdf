//! A real program round trip. The test binary is its own extension: copied into an extension
//! folder as `demo-child`, it sees its new name and plays the extension (libtest's own report on
//! stdout reaches the host too, as invalid lines it must tolerate).

#![cfg(not(target_arch = "wasm32"))]

use std::io::{BufRead, Write};
use std::sync::Arc;
use std::time::{Duration, Instant};

use pdfcraft_extensions::host::{FromExtension, Host, Message};
use pdfcraft_extensions::protocol::{self, Incoming, Method};
use serde_json::json;

const CHILD: &str = "demo-child";

fn child() {
    let stdin = std::io::stdin();
    let mut lines = stdin.lock().lines();
    let hello = lines.next().and_then(Result::ok).unwrap_or_default();
    let started = hello.contains("host.started");
    let mut out = std::io::stdout();
    let request = json!({
        "jsonrpc": "2.0", "id": 1, "method": "ui.banner.show",
        "params": { "doc": 1, "id": "di", "text": if started { "started" } else { "no hello" }, "actions": [] }
    });
    writeln!(out, "{request}").unwrap();
    out.flush().unwrap();
    let _answer = lines.next();
}

fn wait_for(host: &mut Host, what: impl Fn(&FromExtension) -> bool) -> FromExtension {
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        if let Some(found) = host.poll().into_iter().find(|m| what(m)) {
            return found;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("timed out");
}

#[test]
fn a_program_round_trip() {
    let me = std::env::current_exe().unwrap();
    if me.file_stem().and_then(|s| s.to_str()) == Some(CHILD) {
        child();
        return;
    }
    let root = std::env::temp_dir().join(format!("nadspdf-ext-host-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let dir = root.join("demo");
    std::fs::create_dir_all(&dir).unwrap();
    let exe = format!("{CHILD}{}", std::env::consts::EXE_SUFFIX);
    std::fs::copy(&me, dir.join(&exe)).unwrap();
    std::fs::write(
        dir.join("manifest.json"),
        format!(r#"{{"protocol":1,"id":"com.example.demo","name":"Demo","version":"1","executable":"{exe}","permissions":["banner"]}}"#),
    )
    .unwrap();
    let installed = pdfcraft_extensions::discovery::load(&dir).unwrap();

    let mut host = Host::new(Arc::new(|| {}));
    host.start(&installed, json!({ "host": "test", "protocol": 1 })).unwrap();
    assert!(host.is_running("com.example.demo"));

    let request = wait_for(&mut host, |m| matches!(&m.message, Message::Line(Incoming::Request { .. })));
    let Message::Line(Incoming::Request { id, method, params }) = request.message else { unreachable!() };
    assert_eq!(request.ext, "com.example.demo");
    assert_eq!(method, Method::BannerShow);
    assert_eq!(params["text"], "started", "the program got host.started first");

    assert!(host.send("com.example.demo", &protocol::result(&id, json!({ "shown": true }))));
    wait_for(&mut host, |m| matches!(m.message, Message::Exited(_)));
    assert!(!host.is_running("com.example.demo"));
    drop(host);
    let _ = std::fs::remove_dir_all(&root);
}
