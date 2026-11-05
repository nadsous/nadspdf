//! The assistant's API keys in the system's credential store: Windows Credential Manager, the
//! macOS Keychain. Never in the settings file. One entry per provider, under the service
//! `NadsPDF`.

use pdfcraft_ai::Provider;
use pdfcraft_ui_egui::assistant_ui::KeyStore;

/// The longest key accepted (providers' keys are far shorter).
const MAX_KEY: usize = 512;

pub struct SystemKeys;

fn entry(provider: Provider) -> Result<keyring::Entry, String> {
    keyring::Entry::new("NadsPDF", &format!("assistant-{}", provider.id())).map_err(|e| format!("the credential store can't be used: {e}"))
}

impl KeyStore for SystemKeys {
    fn get(&self, provider: Provider) -> Option<String> {
        entry(provider).ok()?.get_password().ok().filter(|k| !k.is_empty() && k.len() <= MAX_KEY)
    }

    fn set(&self, provider: Provider, key: &str) -> Result<(), String> {
        let key = key.trim();
        if key.is_empty() || key.len() > MAX_KEY || key.chars().any(|c| c.is_control() || c.is_whitespace()) {
            return Err("this doesn't look like an API key".into());
        }
        entry(provider)?.set_password(key).map_err(|e| format!("the key couldn't be stored: {e}"))
    }

    fn delete(&self, provider: Provider) -> Result<(), String> {
        match entry(provider)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(format!("the key couldn't be removed: {e}")),
        }
    }
}
