//! Échéances: the extension program. See `nads_extensions::echeances` for how dates are found.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable, clippy::indexing_slicing)]

use std::collections::HashMap;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use nads_extensions::echeances::{Deadline, date_fr, find, ics, safe_file_stem};
use nads_extensions::rpc::{self, Button, Extension, Out};
use nads_extensions::store;

const ID: &str = "site.nads.echeances";
const BANNER: &str = "echeances";

const ADD_ALL: Button = Button { id: "add_all", label: "Ajouter au calendrier", primary: true };
const ADD_ALL_MANY: Button = Button { id: "add_all", label: "Tout ajouter au calendrier", primary: true };
const ADD_FIRST: Button = Button { id: "add_first", label: "Seulement la plus proche", primary: false };

struct Echeances {
    dir: PathBuf,
    /// Each open document's name and dates.
    docs: HashMap<u64, (String, Vec<Deadline>)>,
}

/// The banner for the dates of a document (at least one).
fn banner(found: &[Deadline]) -> Option<(String, Vec<Button>)> {
    let first = found.first()?;
    let when = format!("{} le {}", first.kind.label(), date_fr(first.date));
    Some(match found.len() {
        1 => (format!("{when} : « {} »", first.context), vec![ADD_ALL]),
        n => (format!("{n} dates à retenir, la plus proche : {when} : « {} »", first.context), vec![ADD_ALL_MANY, ADD_FIRST]),
    })
}

/// Opens `path` with the program the system uses for it (the calendar for an `.ics`). The
/// program's output is discarded: our stdout is NadsPDF's channel.
fn open_with_system(path: &Path) -> io::Result<()> {
    let mut command = if cfg!(windows) {
        Command::new("explorer.exe")
    } else if cfg!(target_os = "macos") {
        Command::new("open")
    } else {
        Command::new("xdg-open")
    };
    let mut child = command.arg(path).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn()?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

impl Extension for Echeances {
    fn on_text<W: Write>(&mut self, out: &mut Out<W>, doc: u64, name: &str, text: &str) -> io::Result<()> {
        let found = find(text, chrono::Local::now().date_naive());
        if let Some((text, buttons)) = banner(&found) {
            out.banner(doc, BANNER, &text, &buttons)?;
            self.docs.insert(doc, (name.to_string(), found));
        }
        Ok(())
    }

    fn on_action<W: Write>(&mut self, out: &mut Out<W>, doc: u64, banner: &str, action: &str) -> io::Result<()> {
        if banner != BANNER {
            return Ok(());
        }
        let Some((name, found)) = self.docs.get(&doc) else { return Ok(()) };
        let chosen = match action {
            "add_all" => found.as_slice(),
            "add_first" => found.get(..1).unwrap_or_default(),
            _ => return Ok(()),
        };
        let Some(first) = chosen.first() else { return Ok(()) };
        let file = self.dir.join(format!("{}-{}.ics", safe_file_stem(name), first.date.format("%Y%m%d")));
        let text = match std::fs::write(&file, ics(chosen, name, chrono::Utc::now())).and_then(|()| open_with_system(&file)) {
            Ok(()) => {
                let what = if chosen.len() == 1 { "1 date envoyée".to_string() } else { format!("{} dates envoyées", chosen.len()) };
                format!("{what} à votre agenda. Rien ne s'ouvre ? Importez le fichier {}", file.display())
            }
            Err(err) => format!("Impossible d'écrire ou d'ouvrir le fichier calendrier ({err})."),
        };
        out.banner(doc, BANNER, &text, &[])?;
        Ok(())
    }

    fn on_closed(&mut self, doc: u64) {
        self.docs.remove(&doc);
    }
}

fn main() {
    let mut ext = Echeances { dir: store::data_dir(ID), docs: HashMap::new() };
    if let Err(err) = rpc::run(&mut ext, io::stdin().lock(), io::stdout().lock()) {
        eprintln!("stopped: {err}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    #[test]
    fn banners() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 10).unwrap_or_default();
        assert!(banner(&[]).is_none());
        let one = find("À régler avant le 15/12/2026", today);
        let (text, buttons) = banner(&one).unwrap_or_default();
        assert_eq!(text, "Paiement le 15 décembre 2026 : « À régler avant le 15/12/2026 »");
        assert_eq!(buttons, [ADD_ALL]);
        let two = find("Rendez-vous le 20/01/2027. Paiement avant le 05/12/2026.", today);
        let (text, buttons) = banner(&two).unwrap_or_default();
        assert!(text.starts_with("2 dates à retenir, la plus proche : Paiement le 5 décembre 2026"), "{text}");
        assert_eq!(buttons, [ADD_ALL_MANY, ADD_FIRST]);
    }
}
