//! Running extension programs.
//!
//! Each program gets its stdin (lines from NadsPDF) and stdout (lines from the extension). A
//! reader thread per program turns its lines into [`FromExtension`] messages on one channel,
//! which the app drains once a frame; `wake` asks the app for a frame when one arrives. A
//! program that exits, or closes its stdout, is reported once with [`Message::Exited`] and is
//! not restarted. Its stderr goes to the log.
//!
//! Dropping the [`Host`] closes every program's stdin (the polite way to ask it to stop) and
//! then ends the ones still running.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

use crate::discovery::Installed;
use crate::manifest::Permission;
use crate::protocol::{self, Incoming, MAX_LINE_BYTES};

/// Messages handled per [`Host::poll`], so a chatty extension can't stall a frame.
pub const MAX_MESSAGES_PER_POLL: usize = 256;
/// How long a program has to exit after its stdin closes before it is ended.
const STOP_GRACE: Duration = Duration::from_millis(500);
/// A stderr line longer than this is cut in the log.
const MAX_LOG_LINE: usize = 500;

pub type Wake = Arc<dyn Fn() + Send + Sync>;

#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    Line(Incoming),
    /// The program ended or closed its stdout: why, in a sentence.
    Exited(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct FromExtension {
    pub ext: String,
    pub message: Message,
}

struct Running {
    child: Child,
    stdin: Option<ChildStdin>,
    permissions: Vec<Permission>,
}

pub struct Host {
    running: HashMap<String, Running>,
    tx: Sender<FromExtension>,
    rx: Receiver<FromExtension>,
    wake: Wake,
}

impl Host {
    pub fn new(wake: Wake) -> Host {
        let (tx, rx) = channel();
        Host { running: HashMap::new(), tx, rx, wake }
    }

    pub fn is_running(&self, id: &str) -> bool {
        self.running.contains_key(id)
    }

    /// Start an approved extension and send it `host.started`. Approval is the caller's check.
    pub fn start(&mut self, ext: &Installed, hello: serde_json::Value) -> Result<(), String> {
        let id = ext.manifest.id.clone();
        if self.running.contains_key(&id) {
            return Ok(());
        }
        let mut command = Command::new(&ext.executable);
        command.current_dir(&ext.dir).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // CREATE_NO_WINDOW: a console program must not flash a window over NadsPDF.
            command.creation_flags(0x0800_0000);
        }
        let mut child = command.spawn().map_err(|e| format!("{} could not start: {e}", ext.manifest.name))?;
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let stdin = child.stdin.take();
        if let Some(stdout) = stdout {
            let (tx, wake, ext_id) = (self.tx.clone(), self.wake.clone(), id.clone());
            let spawned = std::thread::Builder::new().name(format!("ext-{id}")).spawn(move || read_lines(stdout, &ext_id, &tx, &wake));
            if let Err(e) = spawned {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("{} could not start: {e}", ext.manifest.name));
            }
        }
        if let Some(stderr) = stderr {
            let ext_id = id.clone();
            let _ = std::thread::Builder::new().name(format!("ext-{id}-log")).spawn(move || log_lines(stderr, &ext_id));
        }
        self.running.insert(id.clone(), Running { child, stdin, permissions: ext.manifest.permissions.clone() });
        self.send(&id, &protocol::event("host.started", hello));
        Ok(())
    }

    /// Write one line to an extension. A program that can't take it any more is stopped.
    pub fn send(&mut self, id: &str, line: &str) -> bool {
        let Some(run) = self.running.get_mut(id) else { return false };
        let ok = run
            .stdin
            .as_mut()
            .is_some_and(|stdin| stdin.write_all(line.as_bytes()).and_then(|()| stdin.write_all(b"\n")).and_then(|()| stdin.flush()).is_ok());
        if !ok {
            self.stop(id);
        }
        ok
    }

    /// Send an event to every running extension that holds `permission`.
    pub fn broadcast(&mut self, permission: Permission, line: &str) {
        let ids: Vec<String> = self.running.iter().filter(|(_, r)| r.permissions.contains(&permission)).map(|(id, _)| id.clone()).collect();
        for id in ids {
            self.send(&id, line);
        }
    }

    /// Messages that arrived since the last poll (at most [`MAX_MESSAGES_PER_POLL`]). An
    /// `Exited` message also forgets the program.
    pub fn poll(&mut self) -> Vec<FromExtension> {
        let got: Vec<FromExtension> = self.rx.try_iter().take(MAX_MESSAGES_PER_POLL).collect();
        for m in &got {
            if matches!(m.message, Message::Exited(_)) {
                self.stop(&m.ext);
            }
        }
        if got.len() == MAX_MESSAGES_PER_POLL {
            (self.wake)();
        }
        got
    }

    /// Close the program's stdin, give it a moment to exit, then end it.
    pub fn stop(&mut self, id: &str) {
        if let Some(mut run) = self.running.remove(id) {
            drop(run.stdin.take());
            let deadline = Instant::now() + STOP_GRACE;
            while Instant::now() < deadline {
                if !matches!(run.child.try_wait(), Ok(None)) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            let _ = run.child.kill();
            let _ = run.child.wait();
        }
    }

    pub fn stop_all(&mut self) {
        let ids: Vec<String> = self.running.keys().cloned().collect();
        for id in ids {
            self.stop(&id);
        }
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        self.stop_all();
    }
}

fn read_lines(stdout: impl Read, ext: &str, tx: &Sender<FromExtension>, wake: &Wake) {
    let mut reader = BufReader::new(stdout);
    let reason = loop {
        match read_bounded_line(&mut reader, MAX_LINE_BYTES) {
            Ok(None) => break "the extension closed its output".to_string(),
            Ok(Some(Line::TooLong)) => {
                let message = Message::Line(Incoming::Invalid {
                    id: None,
                    code: protocol::INVALID_REQUEST,
                    message: format!("a message is limited to {MAX_LINE_BYTES} bytes"),
                });
                if tx.send(FromExtension { ext: ext.to_string(), message }).is_err() {
                    return;
                }
            }
            Ok(Some(Line::Text(text))) => {
                if text.trim().is_empty() {
                    continue;
                }
                let message = Message::Line(protocol::parse_line(&text));
                if tx.send(FromExtension { ext: ext.to_string(), message }).is_err() {
                    return;
                }
            }
            Err(e) => break format!("the extension's output could not be read: {e}"),
        }
        wake();
    };
    let _ = tx.send(FromExtension { ext: ext.to_string(), message: Message::Exited(reason) });
    wake();
}

fn log_lines(stderr: impl Read, ext: &str) {
    let mut reader = BufReader::new(stderr);
    while let Ok(Some(line)) = read_bounded_line(&mut reader, 64 * 1024) {
        if let Line::Text(text) = line {
            let cut: String = text.chars().take(MAX_LOG_LINE).collect();
            log::info!("extension {ext}: {cut}");
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Line {
    Text(String),
    TooLong,
}

/// One line without its `\n` (and `\r`), or `TooLong` after skipping the rest of an oversized
/// one. `None` at the end of the stream. Invalid UTF-8 is replaced, not refused: the JSON parser
/// then reports the line.
fn read_bounded_line(reader: &mut impl BufRead, max: usize) -> std::io::Result<Option<Line>> {
    let mut buf = Vec::new();
    let mut too_long = false;
    loop {
        let chunk = reader.fill_buf()?;
        if chunk.is_empty() {
            if buf.is_empty() && !too_long {
                return Ok(None);
            }
            break;
        }
        let (take, done) = match chunk.iter().position(|&b| b == b'\n') {
            Some(i) => (i, true),
            None => (chunk.len(), false),
        };
        if !too_long {
            let room = max.saturating_sub(buf.len());
            if take > room {
                too_long = true;
                buf.clear();
            } else {
                buf.extend_from_slice(chunk.get(..take).unwrap_or_default());
            }
        }
        reader.consume(if done { take.saturating_add(1) } else { take });
        if done {
            break;
        }
    }
    if too_long {
        return Ok(Some(Line::TooLong));
    }
    if buf.last() == Some(&b'\r') {
        buf.pop();
    }
    Ok(Some(Line::Text(String::from_utf8_lossy(&buf).into_owned())))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(input: &[u8], max: usize) -> Vec<Line> {
        let mut r = BufReader::with_capacity(4, input);
        let mut out = Vec::new();
        while let Some(l) = read_bounded_line(&mut r, max).unwrap() {
            out.push(l);
        }
        out
    }

    #[test]
    fn splits_lines_across_small_buffers() {
        assert_eq!(lines(b"hello\r\nworld\nlast", 100), vec![Line::Text("hello".into()), Line::Text("world".into()), Line::Text("last".into())]);
    }

    #[test]
    fn an_oversized_line_is_skipped_to_its_end() {
        assert_eq!(lines(b"0123456789\nok\n", 5), vec![Line::TooLong, Line::Text("ok".into())]);
        assert_eq!(lines(b"0123456789", 5), vec![Line::TooLong]);
    }

    #[test]
    fn empty_input_has_no_lines() {
        assert!(lines(b"", 5).is_empty());
        assert_eq!(lines(b"\n", 5), vec![Line::Text(String::new())]);
    }

    #[test]
    fn invalid_utf8_is_replaced() {
        assert_eq!(lines(b"\xff{}\n", 10), vec![Line::Text("\u{fffd}{}".into())]);
    }
}
