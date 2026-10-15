//! pdfcraft-extensions — L7 out-of-process extensions.
//!
//! An extension is a folder holding a `manifest.json` and a program. NadsPDF starts the program
//! and talks to it with newline-delimited JSON-RPC 2.0 on its stdin and stdout, the same framing
//! as the MCP server (`docs/extensions.md` is the contract for extension authors).
//!
//! **Opt-in only.** Nothing runs unless the user copied an extension into the extensions folder
//! *and* approved it. An approval is bound to a fingerprint of the manifest and the program, so
//! a changed program or a new permission asks again. An extension only gets the requests its
//! manifest declares, and opens only the web origins it lists.
//!
//! This crate is headless: [`manifest`] parses and checks a manifest, [`discovery`] lists the
//! installed extensions, [`approvals`] keeps what the user allowed, [`protocol`] reads and writes
//! the messages and [`host`] runs the programs. The desktop app draws the banners and answers the
//! requests (`ui-egui`'s `extensions_ui`).

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod approvals;
pub mod discovery;
pub mod folders;
#[cfg(not(target_arch = "wasm32"))]
pub mod host;
pub mod manifest;
pub mod protocol;

pub use approvals::Approvals;
pub use discovery::{Entry, Installed, discover};
pub use manifest::{Manifest, Permission};
