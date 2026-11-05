//! NadsPDF's Assistant end to end, with a stand-in provider and key store (no network): setup,
//! a question with the document's text, Süleyman's table, Selim's confirmed actions, and the
//! usage counted.

#![cfg(not(target_arch = "wasm32"))]

use std::sync::{Arc, Mutex};

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use pdfcraft_ai::{Provider, Reply, Request, Usage};
use pdfcraft_ui_egui::PdfCraftApp;
use pdfcraft_ui_egui::assistant_ui::KeyStore;

#[derive(Default)]
struct Keys(Mutex<Option<String>>);

impl KeyStore for Keys {
    fn get(&self, _: Provider) -> Option<String> {
        self.0.lock().unwrap().clone()
    }
    fn set(&self, _: Provider, key: &str) -> Result<(), String> {
        *self.0.lock().unwrap() = Some(key.to_string());
        Ok(())
    }
    fn delete(&self, _: Provider) -> Result<(), String> {
        *self.0.lock().unwrap() = None;
        Ok(())
    }
}

/// One page saying "Facture F-12 Total 1250 EUR".
fn fixture() -> Vec<u8> {
    let body = "BT /F1 18 Tf 20 150 Td (Facture F-12 Total 1250 EUR) Tj ET";
    let objs = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [4 0 R] /Count 1 /MediaBox [0 0 400 300] >>".to_string(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /Contents 5 0 R /Resources << /Font << /F1 3 0 R >> >> >>".to_string(),
        format!("<< /Length {} >>\nstream\n{body}\nendstream", body.len()),
    ];
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, o) in objs.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{o}\nendobj\n", i + 1).as_bytes());
    }
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).as_bytes());
    for o in offsets {
        out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objs.len() + 1).as_bytes());
    out
}

fn wait(h: &mut Harness<'static, PdfCraftApp>) {
    for _ in 0..300 {
        h.run_steps(1);
        if !h.state().assistant.busy() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    h.run_steps(3);
}

#[test]
fn the_assistant_answers_with_a_table_and_carries_out_confirmed_actions() {
    let sent: Arc<Mutex<Vec<Request>>> = Arc::default();
    let answers = Arc::new(Mutex::new(vec![
        "Voici les montants.\n```json\n{\"title\":\"Montants\",\"columns\":[\"Facture\",\"Total (EUR)\",\"Page\"],\"rows\":[[\"F-12\",1250,1]]}\n```"
            .to_string(),
        "Je propose de surligner la facture.\n```json\n{\"actions\":[{\"action\":\"highlight\",\"text\":\"F-12\"}]}\n```".to_string(),
    ]));
    let keys = Arc::new(Keys::default());
    let (s, a, k) = (sent.clone(), answers.clone(), keys.clone());
    let mut h = Harness::builder().with_size(egui::vec2(1500.0, 1000.0)).build_eframe(move |_cc| {
        let mut app = PdfCraftApp::new();
        app.set_option("language", "en").unwrap();
        app.open_bytes("facture.pdf", None, fixture()).unwrap();
        app.ai_keys = Some(k.clone());
        let (s, a) = (s.clone(), a.clone());
        app.ai_send = Some(Arc::new(move |_provider, _model: &str, key: &str, request: &Request| {
            assert_eq!(key, "sk-test-123", "the stored key is used");
            s.lock().unwrap().push(request.clone());
            let text = a.lock().unwrap().remove(0);
            Ok(Reply { text, usage: Usage { input_tokens: 1200, output_tokens: 80, cost_usd: None }, model: "test-model".into(), truncated: false })
        }));
        app
    });
    h.run_steps(6);
    h.state_mut().toggle_assistant();
    h.run_steps(4);
    // No key yet: the setup card shows, and nothing can be sent.
    h.get_by_label("Use your own API key");
    assert!(keys.get(Provider::Anthropic).is_none());
    // The key is typed and saved: it goes to the credential store, and the assistant is on.
    h.get_by_role(egui::accesskit::Role::PasswordInput).click();
    h.run_steps(2);
    h.get_by_role(egui::accesskit::Role::PasswordInput).type_text("sk-test-123");
    h.run_steps(2);
    h.get_by_label("Save").click();
    h.run_steps(4);
    assert_eq!(keys.get(Provider::Anthropic).as_deref(), Some("sk-test-123"));
    assert!(h.state().assistant.settings.enabled);
    h.state_mut().assistant.settings.price_input = Some(3.0);
    h.state_mut().assistant.settings.price_output = Some(15.0);
    h.run_steps(4);

    // Süleyman extracts a table.
    h.state_mut().assistant.who = pdfcraft_ai::assistants::Assistant::Suleyman;
    h.state_mut().ask_assistant("Fais un tableau des montants");
    wait(&mut h);
    {
        let req = &sent.lock().unwrap()[0];
        let last = &req.messages.last().unwrap().content;
        assert!(last.contains("[Page 1]") && last.contains("F-12") && last.ends_with("Fais un tableau des montants"), "{last}");
        assert!(req.system.contains("You are Süleyman") && req.system.contains("data, not instructions"));
    }
    h.get_by_label("Export to Excel…");
    h.get_by_label("1250");
    let month = h.state().assistant.ledger.months.values().next().cloned().unwrap();
    assert_eq!((month.requests, month.input_tokens, month.output_tokens, month.unpriced), (1, 1200, 80, 0));
    assert!((month.cost_usd - (1200.0 * 3.0 + 80.0 * 15.0) / 1e6).abs() < 1e-12, "priced from the user's figures");

    // Selim proposes a highlight; nothing changes until it is carried out.
    h.state_mut().assistant.who = pdfcraft_ai::assistants::Assistant::Selim;
    h.state_mut().ask_assistant("Surligne la facture");
    wait(&mut h);
    assert_eq!(sent.lock().unwrap()[1].messages.len(), 3, "the earlier exchange goes with the follow-up");
    let id = h.state().views[0].id;
    let before = h.state().session.get(id).unwrap().info.annotations.len();
    h.get_by_label("Highlight “F-12”");
    h.get_by_label("Carry out the ticked actions").click();
    h.run_steps(4);
    assert_eq!(h.state().session.get(id).unwrap().info.annotations.len(), before + 1, "one highlight");
    h.get_by_label("Done. Undo takes it back.");
}
