//! NadsPDF's own extensions, written against protocol 1 (`docs/extensions.md`) like anyone's:
//! separate programs NadsPDF starts only once the user imported and turned them on.
//!
//! - `nads-factures` (**Contrôle des factures**): checks the IBAN, BIC, SIREN/SIRET and VAT
//!   numbers of an opened invoice offline, and warns when a supplier's IBAN changed.
//! - `nads-echeances` (**Échéances**): finds the dates a document asks something for and puts
//!   them in the user's calendar as an `.ics` file.
//!
//! Both ask only for `document_events`, `read_text` and `banner`: no file paths, no web page.
//! `cargo xtask extensions` builds them into `dist/extensions/<id>/`, ready for Home ▸
//! Extensions ▸ Import an extension.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable, clippy::indexing_slicing)]

pub mod echeances;
pub mod factures;
pub mod rpc;
pub mod store;
pub mod text;
