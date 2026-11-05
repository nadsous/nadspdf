//! pdfcraft-ai — NadsPDF's AI assistants, with the user's own API key ("bring your own key").
//!
//! NadsPDF itself never pays for, proxies or stores anything: the user pastes a key from
//! Anthropic, OpenRouter or Mistral, requests go from their computer straight to that provider,
//! and the provider bills them. This crate is headless and has no network code of its own unless
//! the `net` feature is on (only the desktop app turns it on):
//!
//! - [`Provider`] and [`Settings`]: where requests go and with which model.
//! - [`assistants`]: the five assistants and their working prompts.
//! - [`context`]: which pages of a document are sent, within a size budget.
//! - [`answer`]: what an assistant's answer holds: a table (Süleyman) or actions (Selim).
//! - [`Usage`] and [`Ledger`]: tokens and cost per request and per month, for the user to see.
//! - `net::send` (feature `net`): one HTTPS request to the provider.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod answer;
pub mod assistants;
pub mod context;
mod ledger;
#[cfg(feature = "net")]
pub mod net;
mod provider;

pub use ledger::{Ledger, MonthTotal, month_of};
pub use provider::{Message, Provider, Reply, Request, Role, Settings, Usage, body, error_message, parse_reply};
