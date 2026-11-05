//! NadsPDF's Assistant panel: five assistants that work on the open PDF with the user's own API
//! key (Anthropic, OpenRouter or Mistral). NadsPDF sends nothing anywhere else and pays nothing:
//! the provider bills the user, and the panel shows what each answer and the month used.
//!
//! - The key lives in the system's credential store ([`KeyStore`], set by the desktop app), never
//!   in the settings file.
//! - Before asking, the panel says which pages will go and roughly how many tokens.
//! - Süleyman's tables open in a grid and export to Excel; Selim's actions are listed with a box
//!   each and run only once confirmed (each is one undo step).
//! - Requests run on a worker thread; the panel can be used (and the request cancelled) meanwhile.

use std::collections::HashMap;
use std::sync::Arc;

use egui::{CornerRadius, Sense, Stroke, vec2};
use pdfcraft_ai::answer::{Action, Cell, Parsed};
use pdfcraft_ai::assistants::Assistant;
use pdfcraft_ai::{Ledger, Message, Provider, Reply, Request, Role, Settings, Usage};
use pdfcraft_engine::DocId;

use crate::theme::{self, Tokens};
use crate::{PdfCraftApp, icons, widgets};

/// Sends a request: (provider, model, key, request) → the reply (blocking; on a worker thread).
pub type AiSend = Arc<dyn Fn(Provider, &str, &str, &Request) -> Result<Reply, String> + Send + Sync>;

/// The system's credential store, for the providers' keys (set by the desktop app).
pub trait KeyStore: Send + Sync {
    fn get(&self, provider: Provider) -> Option<String>;
    fn set(&self, provider: Provider, key: &str) -> Result<(), String>;
    fn delete(&self, provider: Provider) -> Result<(), String>;
}

/// Pages read from a document at most (a longer one sends from its first 1 000).
const MAX_PAGES_READ: usize = 1000;
/// Earlier exchanges sent with a question, so follow-ups make sense.
const HISTORY_TURNS: usize = 6;

/// One message in a conversation.
#[derive(Clone)]
pub struct Turn {
    /// `None`: the user's question.
    pub who: Option<Assistant>,
    pub text: String,
    pub parsed: Option<Parsed>,
    pub usage: Option<Usage>,
    pub cost: Option<f64>,
    /// Which pages went with the question ("1–12").
    pub pages: String,
    pub model: String,
    pub truncated: bool,
    /// Selim's actions the user keeps ticked.
    pub chosen: Vec<bool>,
    pub applied: bool,
}

impl Turn {
    fn question(text: String) -> Turn {
        Turn {
            who: None,
            text,
            parsed: None,
            usage: None,
            cost: None,
            pages: String::new(),
            model: String::new(),
            truncated: false,
            chosen: Vec::new(),
            applied: false,
        }
    }
}

struct Job {
    doc: DocId,
    who: Assistant,
    rx: std::sync::mpsc::Receiver<Result<(Reply, String), String>>,
}

/// The panel's state.
#[derive(Default)]
pub struct AssistantState {
    pub settings: Settings,
    pub ledger: Ledger,
    pub open: bool,
    pub who: Assistant,
    pub input: String,
    pub chats: HashMap<DocId, Vec<Turn>>,
    job: Option<Job>,
    pub error: Option<String>,
    /// The key being typed in the setup card (cleared once stored).
    key_draft: String,
    /// Whether the store holds a key for the chosen provider (asked once per provider).
    has_key: Option<(Provider, bool)>,
    /// The setup card is showing (first use, or Settings clicked).
    setup: bool,
}

impl AssistantState {
    pub fn busy(&self) -> bool {
        self.job.is_some()
    }

    pub(crate) fn persist(&self) -> serde_json::Value {
        serde_json::json!({ "settings": self.settings, "ledger": self.ledger })
    }

    pub(crate) fn restore(&mut self, v: &serde_json::Value) {
        if let Ok(s) = serde_json::from_value::<Settings>(v["settings"].clone()) {
            self.settings = s;
        }
        if let Ok(l) = serde_json::from_value::<Ledger>(v["ledger"].clone()) {
            self.ledger = l;
        }
    }
}

fn this_month() -> String {
    let secs = web_time::SystemTime::now().duration_since(web_time::SystemTime::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    pdfcraft_ai::month_of(secs)
}

/// Tokens for `chars` characters of text, roughly (about four characters each).
fn tokens_for(chars: usize) -> usize {
    chars.div_ceil(4)
}

/// "12 345" for people.
fn grouped(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(' ');
        }
        out.push(c);
    }
    out
}

fn dollars(v: f64) -> String {
    if v < 0.01 && v > 0.0 { format!("{v:.4} $") } else { format!("{v:.2} $") }
}

/// The avatar picture of `who`.
fn avatar(ui: &mut egui::Ui, who: Assistant, size: f32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(size, size), Sense::click());
    let bytes: &'static [u8] = match who {
        Assistant::Osman => include_bytes!("../../../assets/app-icon/assistants/osman.svg"),
        Assistant::Mehmed => include_bytes!("../../../assets/app-icon/assistants/mehmed.svg"),
        Assistant::Suleyman => include_bytes!("../../../assets/app-icon/assistants/suleyman.svg"),
        Assistant::Selim => include_bytes!("../../../assets/app-icon/assistants/selim.svg"),
        Assistant::Mahmud => include_bytes!("../../../assets/app-icon/assistants/mahmud.svg"),
    };
    egui::Image::from_bytes(format!("bytes://assistant/{}.svg", who.id()), bytes).paint_at(ui, rect);
    resp
}

/// A light Markdown reading: headings, bullets and bold; the rest as written.
fn markdown(ui: &mut egui::Ui, text: &str, t: &Tokens) {
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.is_empty() {
            ui.add_space(4.0);
            continue;
        }
        let (body, heading) = match trimmed.strip_prefix("### ").or_else(|| trimmed.strip_prefix("## ")).or_else(|| trimmed.strip_prefix("# ")) {
            Some(h) => (h.to_string(), true),
            None => match trimmed.strip_prefix("- ").or_else(|| trimmed.strip_prefix("* ")) {
                Some(b) => (format!("•  {b}"), false),
                None => (trimmed.to_string(), false),
            },
        };
        let strong = heading || (body.starts_with("**") && body.ends_with("**") && body.len() > 4);
        let plain = body.replace("**", "");
        let rich = egui::RichText::new(plain).color(t.text);
        ui.label(if strong { rich.font(theme::semibold(13.5)) } else { rich.font(theme::regular(13.0)) });
    }
}

impl PdfCraftApp {
    /// View ▸ Assistant: show or hide the panel.
    pub fn toggle_assistant(&mut self) {
        self.assistant.open = !self.assistant.open;
    }

    /// Whether the store has a key for `provider` (asked once, then remembered).
    fn assistant_has_key(&mut self, provider: Provider) -> bool {
        if let Some((p, has)) = self.assistant.has_key
            && p == provider
        {
            return has;
        }
        let has = self.ai_keys.as_ref().is_some_and(|k| k.get(provider).is_some_and(|key| !key.trim().is_empty()));
        self.assistant.has_key = Some((provider, has));
        has
    }

    /// Ask `question` to the chosen assistant about the active document.
    pub fn ask_assistant(&mut self, question: &str) {
        let question = question.trim().to_string();
        if question.is_empty() || self.assistant.busy() {
            return;
        }
        let Some((_, id)) = self.active_ids() else { return };
        let Some(doc) = self.session.get(id) else { return };
        let (Some(send), Some(keys)) = (self.ai_send.clone(), self.ai_keys.clone()) else {
            self.assistant.error = Some(tl!("The assistant isn't available in this copy of NadsPDF.").into());
            return;
        };
        let settings = self.assistant.settings.clone();
        let who = self.assistant.who;
        let bytes = doc.bytes.clone();
        let password = doc.password.clone();
        let count = doc.info.pages.len();
        let chat = self.assistant.chats.entry(id).or_default();
        // Earlier answered exchanges, oldest first, alternating user / assistant.
        let mut history: Vec<Message> = Vec::new();
        let answered: Vec<&Turn> = chat.iter().collect();
        for pair in answered.windows(2) {
            if let [q, a] = pair
                && q.who.is_none()
                && a.who.is_some()
            {
                history.push(Message { role: Role::User, content: q.text.clone() });
                history.push(Message { role: Role::Assistant, content: a.text.clone() });
            }
        }
        let skip = history.len().saturating_sub(HISTORY_TURNS * 2);
        let history: Vec<Message> = history.into_iter().skip(skip).collect();
        chat.push(Turn::question(question.clone()));
        self.assistant.error = None;
        let (tx, rx) = std::sync::mpsc::channel();
        let ctx = self.ctx.clone();
        let work = move || {
            let result = (|| {
                let key = keys.get(settings.provider).ok_or_else(|| "no API key is stored for this provider".to_string())?;
                let config = pdfcraft_render::RenderConfig { password: password.as_deref().map(Arc::from), ..Default::default() };
                let mut renderer = pdfcraft_render::PageRenderer::new(bytes, config);
                let pages: Vec<String> = (0..count.min(MAX_PAGES_READ))
                    .map(|page| {
                        let out = renderer.render(pdfcraft_render::RenderRequest {
                            page,
                            kind: pdfcraft_render::RequestKind::Text,
                            scale: 1.0,
                            ..Default::default()
                        });
                        out.text.map(|t| t.plain_text()).unwrap_or_default()
                    })
                    .collect();
                let context = pdfcraft_ai::context::build(&pages, &question, settings.max_context_chars);
                let mut messages = history;
                messages.push(Message { role: Role::User, content: format!("{}\n\n{question}", context.text) });
                let request = Request { system: who.system(), messages, max_tokens: settings.max_output_tokens };
                let reply = send(settings.provider, &settings.model, &key, &request)?;
                Ok((reply, pdfcraft_ai::context::describe(&context.pages)))
            })();
            let _ = tx.send(result);
            if let Some(ctx) = &ctx {
                ctx.request_repaint();
            }
        };
        #[cfg(not(target_arch = "wasm32"))]
        if let Err(e) = std::thread::Builder::new().name("nadspdf-assistant".into()).spawn(work) {
            self.assistant.error = Some(e.to_string());
            return;
        }
        #[cfg(target_arch = "wasm32")]
        work();
        self.assistant.job = Some(Job { doc: id, who, rx });
    }

    /// Pick up a finished answer (each frame).
    pub(crate) fn poll_assistant(&mut self) {
        let Some(job) = &self.assistant.job else { return };
        let result = match job.rx.try_recv() {
            Ok(r) => r,
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => Err(tl!("The request stopped unexpectedly.").into()),
        };
        let (doc, who) = (job.doc, job.who);
        self.assistant.job = None;
        match result {
            Ok((reply, pages)) => {
                let cost = self.assistant.settings.cost(&reply.usage);
                self.assistant.ledger.record(&this_month(), &reply.usage, cost);
                let parsed = pdfcraft_ai::answer::parse(&reply.text);
                let chosen = vec![true; parsed.actions.len()];
                let turn = Turn {
                    who: Some(who),
                    text: reply.text.clone(),
                    parsed: Some(parsed),
                    usage: Some(reply.usage),
                    cost,
                    pages,
                    model: reply.model,
                    truncated: reply.truncated,
                    chosen,
                    applied: false,
                };
                self.assistant.chats.entry(doc).or_default().push(turn);
            }
            Err(e) => self.assistant.error = Some(e),
        }
    }

    /// Stop waiting for the answer (it is dropped when it arrives; the provider may still bill it).
    pub fn cancel_assistant(&mut self) {
        self.assistant.job = None;
    }

    /// Carry out Selim's ticked actions in answer `turn` of the active document.
    fn apply_assistant_actions(&mut self, turn: usize) {
        let Some((_, id)) = self.active_ids() else { return };
        let Some(t) = self.assistant.chats.get(&id).and_then(|c| c.get(turn)).cloned() else { return };
        let Some(parsed) = &t.parsed else { return };
        let mut done = 0usize;
        for (action, keep) in parsed.actions.iter().zip(&t.chosen) {
            if !keep {
                continue;
            }
            done += 1;
            match action {
                Action::Highlight { text } => {
                    self.assistant_highlight(text);
                }
                Action::Redact { text } => {
                    self.redact_search = crate::RedactSearchDraft { mode: crate::RedactSearchMode::Phrase, text: text.clone(), ..Default::default() };
                    self.redact_search();
                }
                Action::Bookmark { title, page } => {
                    let count = self.session.get(id).map_or(0, |d| d.info.pages.len());
                    let index = self.session.get(id).map_or(0, |d| d.info.outline.len());
                    if *page >= 1 && *page <= count {
                        self.apply_edit(pdfcraft_engine::Edit::AddBookmark {
                            parent: Vec::new(),
                            index,
                            title: title.clone(),
                            page: page - 1,
                            left: None,
                            top: None,
                        });
                    }
                }
                Action::GoToPage { page } => {
                    if let Some(v) = self.active.and_then(|i| self.views.get_mut(i)) {
                        v.goto = Some((page.saturating_sub(1), 0.0));
                    }
                }
                Action::ExtractPages { pages } => {
                    self.open_extract_dialog();
                    self.extract_draft.pages = pages.clone();
                }
            }
        }
        if let Some(t) = self.assistant.chats.get_mut(&id).and_then(|c| c.get_mut(turn)) {
            t.applied = true;
        }
        self.notify_fmt("{n} actions carried out", &[("n", &done.to_string())]);
    }

    /// Highlight every place `text` appears in the active document (one undo step).
    fn assistant_highlight(&mut self, text: &str) {
        let Some((_, id)) = self.active_ids() else { return };
        let Some(doc) = self.session.get(id) else { return };
        let config = pdfcraft_render::RenderConfig { password: doc.password.as_deref().map(Arc::from), ..Default::default() };
        let mut r = pdfcraft_render::PageRenderer::new(doc.bytes.clone(), config);
        let style = self.comment_prefs.style(crate::comments::CommentTool::Highlight);
        let author = self.comment_prefs.author.clone();
        let mut edits = Vec::new();
        for (page, info) in doc.info.pages.iter().enumerate() {
            let out = r.render(pdfcraft_render::RenderRequest { page, kind: pdfcraft_render::RequestKind::Text, scale: 1.0, ..Default::default() });
            let Some(found) = out.text else { continue };
            for hit in found.find_opts(text, false, false) {
                let quads: Vec<[f64; 8]> = found.line_rects(hit).into_iter().map(|r| info.view_rect_to_quad(r)).collect();
                if !quads.is_empty() {
                    edits.push(pdfcraft_engine::Edit::AddAnnotation(pdfcraft_engine::NewAnnotation {
                        page,
                        shape: pdfcraft_engine::Shape::TextMarkup { kind: pdfcraft_engine::Markup::Highlight, quads },
                        style: style.clone(),
                        contents: String::new(),
                        author: author.clone(),
                    }));
                }
            }
        }
        if !edits.is_empty() {
            self.apply_edit(pdfcraft_engine::Edit::Batch { label: "Highlight".into(), edits });
        }
    }

    /// Save Süleyman's table from answer `turn` as an Excel workbook.
    fn export_assistant_table(&mut self, turn: usize) {
        let Some((_, id)) = self.active_ids() else { return };
        let Some(table) = self.assistant.chats.get(&id).and_then(|c| c.get(turn)).and_then(|t| t.parsed.as_ref()).and_then(|p| p.table.clone())
        else {
            return;
        };
        let name = if table.title.is_empty() { "Table".to_string() } else { table.title.clone() };
        let rows: Vec<Vec<pdfcraft_export::XlsxCell>> = table
            .rows
            .iter()
            .map(|r| {
                r.iter()
                    .map(|c| match c {
                        Cell::Empty => pdfcraft_export::XlsxCell::Empty,
                        Cell::Text(s) => pdfcraft_export::XlsxCell::Text(s.clone()),
                        Cell::Number(n) => pdfcraft_export::XlsxCell::Number(*n),
                    })
                    .collect()
            })
            .collect();
        let bytes = pdfcraft_export::xlsx(&name, &table.columns, &rows);
        let file = format!("{}.xlsx", crate::files::file_stem_from_user(&name).chars().take(80).collect::<String>());
        #[cfg(not(target_arch = "wasm32"))]
        {
            let dialog = rfd::AsyncFileDialog::new().set_file_name(&file).add_filter("Excel", &["xlsx"]);
            self.ask_one(crate::pickers::Ask::Save(dialog), None, move |app, path| match std::fs::write(&path, &bytes) {
                Ok(()) => {
                    app.notify_fmt("Saved {name}", &[("name", &path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())])
                }
                Err(e) => app.notify_fmt("Couldn't save {name}: {e}", &[("name", &path.display().to_string()), ("e", &e.to_string())]),
            });
        }
        #[cfg(target_arch = "wasm32")]
        if let Err(e) = crate::editing::save::download(&file, &bytes) {
            self.notify_fmt("Couldn't save {name}: {e}", &[("name", &file), ("e", &e)]);
        }
    }
}

/// The panel, at the right of the document.
pub(crate) fn panel(app: &mut PdfCraftApp, ui: &mut egui::Ui) {
    if !app.assistant.open || app.active.is_none() {
        return;
    }
    let t = Tokens::get(ui.ctx());
    egui::Panel::right("nadspdf-assistant")
        .resizable(true)
        .default_size(380.0)
        .size_range(320.0..=640.0)
        .frame(
            egui::Frame::NONE
                .fill(t.panel)
                .corner_radius(14)
                .stroke(Stroke::new(1.0, t.border))
                .outer_margin(egui::Margin { left: 0, right: 6, top: 0, bottom: 8 })
                .inner_margin(egui::Margin::same(12)),
        )
        .show(ui, |ui| body(app, ui, &t));
}

fn body(app: &mut PdfCraftApp, ui: &mut egui::Ui, t: &Tokens) {
    // Header: the assistants, and Settings / Close.
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(tl!("Assistant")).font(theme::semibold(16.0)));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if icons::button(ui, "x", 26.0, false, tl!("Close panel")).clicked() {
                app.assistant.open = false;
            }
            if icons::button(ui, "settings", 26.0, app.assistant.setup, tl!("Assistant settings")).clicked() {
                app.assistant.setup = !app.assistant.setup;
            }
        });
    });
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        for who in Assistant::ALL {
            let active = app.assistant.who == who;
            let (rect, resp) = ui.allocate_exact_size(vec2(58.0, 62.0), Sense::click());
            resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, active, who.name()));
            let press = theme::Press::track(ui, &resp);
            press.wash(ui, rect, 10, active);
            if active {
                ui.painter().rect_stroke(rect, CornerRadius::same(10), Stroke::new(1.5, t.accent), egui::StrokeKind::Inside);
            }
            let pic = egui::Rect::from_center_size(rect.center_top() + vec2(0.0, 22.0), vec2(36.0, 36.0));
            let bytes_who = who;
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(pic));
            avatar(&mut child, bytes_who, 36.0);
            ui.painter().text(
                rect.center_bottom() - vec2(0.0, 9.0),
                egui::Align2::CENTER_CENTER,
                who.name(),
                theme::medium(11.5),
                if active { t.accent_text } else { t.text_muted },
            );
            if theme::hand(resp).on_hover_text(tl!(who.role())).clicked() {
                app.assistant.who = who;
            }
        }
    });
    ui.label(
        egui::RichText::new(format!("{} — {}", app.assistant.who.name(), tl!(app.assistant.who.role())))
            .color(t.text_muted)
            .font(theme::regular(12.0)),
    );
    ui.separator();

    let provider = app.assistant.settings.provider;
    let ready = app.assistant.settings.enabled && app.assistant_has_key(provider);
    if app.assistant.setup || !ready {
        setup(app, ui, t);
        return;
    }
    conversation(app, ui, t);
}

/// First use and Settings: the provider, the key (stored in the system's credential store), the
/// model, the prices for the cost estimate, and what is sent where.
fn setup(app: &mut PdfCraftApp, ui: &mut egui::Ui, t: &Tokens) {
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.label(egui::RichText::new(tl!("Use your own API key")).font(theme::semibold(14.0)));
        ui.label(
            egui::RichText::new(tl!(
                "The assistant works with a key from Anthropic, OpenRouter or Mistral. You pay the provider directly for what you use; NadsPDF takes nothing and sees nothing. The text of the pages you ask about goes from your computer straight to the provider."
            ))
            .color(t.text_muted),
        );
        ui.add_space(8.0);
        let s = &mut app.assistant.settings;
        ui.label(tl!("Provider"));
        ui.horizontal_wrapped(|ui| {
            for p in Provider::ALL {
                if widgets::pill_button(ui, p.label(), s.provider == p).clicked() && s.provider != p {
                    s.provider = p;
                    s.model = p.default_model().into();
                    app.assistant.has_key = None;
                }
            }
        });
        let provider = app.assistant.settings.provider;
        ui.add_space(6.0);
        let model_label = ui.label(tl!("Model"));
        ui.add(egui::TextEdit::singleline(&mut app.assistant.settings.model).desired_width(260.0).char_limit(120)).labelled_by(model_label.id);
        // The provider's usual models, one click each; any other model name can be typed.
        ui.horizontal_wrapped(|ui| {
            for m in provider.models() {
                if widgets::pill_button(ui, m, app.assistant.settings.model == *m).clicked() {
                    app.assistant.settings.model = (*m).to_string();
                }
            }
        });
        ui.add_space(6.0);
        let has = app.assistant_has_key(provider);
        ui.label(if has { tl!("API key: stored in this computer's credential store") } else { tl!("API key") });
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut app.assistant.key_draft).password(true).hint_text(if has { "••••••••" } else { "sk-…" }).desired_width(220.0));
            if widgets::pill_button(ui, tl!("Save"), true).clicked() && !app.assistant.key_draft.trim().is_empty() {
                let key = std::mem::take(&mut app.assistant.key_draft);
                match app.ai_keys.as_ref().map(|k| k.set(provider, key.trim())) {
                    Some(Ok(())) => {
                        app.assistant.has_key = Some((provider, true));
                        app.assistant.settings.enabled = true;
                        app.assistant.setup = false;
                    }
                    Some(Err(e)) => app.assistant.error = Some(e),
                    None => app.assistant.error = Some(tl!("The assistant isn't available in this copy of NadsPDF.").into()),
                }
            }
        });
        ui.horizontal(|ui| {
            if ui.link(tl!("Get a key")).clicked() {
                app.open_url(provider.keys_page());
            }
            if has && ui.link(tl!("Remove the key")).clicked() {
                if let Some(k) = &app.ai_keys
                    && let Err(e) = k.delete(provider)
                {
                    app.assistant.error = Some(e);
                }
                app.assistant.has_key = None;
            }
        });
        ui.add_space(10.0);
        ui.label(egui::RichText::new(tl!("Cost estimate")).font(theme::semibold(13.0)));
        if provider.reports_cost() {
            ui.label(egui::RichText::new(tl!("OpenRouter reports what each answer cost.")).color(t.text_muted).small());
        } else {
            ui.label(
                egui::RichText::new(tl!("Copy the model's prices from the provider's page (US dollars per million tokens) to see what each answer costs. Without them, only tokens are counted."))
                    .color(t.text_muted)
                    .small(),
            );
            let s = &mut app.assistant.settings;
            for (label, value) in [(tl!("Input"), &mut s.price_input), (tl!("Output"), &mut s.price_output)] {
                ui.horizontal(|ui| {
                    let name = ui.label(label);
                    let mut text = value.map(|v| v.to_string()).unwrap_or_default();
                    if ui.add(egui::TextEdit::singleline(&mut text).desired_width(80.0).char_limit(12)).labelled_by(name.id).changed() {
                        *value = text.trim().replace(',', ".").parse::<f64>().ok().filter(|v| v.is_finite() && *v >= 0.0);
                    }
                    ui.label(egui::RichText::new("$ / M").color(t.text_faint));
                });
            }
        }
        ui.horizontal(|ui| {
            let name = ui.label(tl!("Alert me past this monthly spend"));
            let s = &mut app.assistant.settings;
            let mut text = s.monthly_alert.map(|v| v.to_string()).unwrap_or_default();
            if ui.add(egui::TextEdit::singleline(&mut text).desired_width(60.0).char_limit(8)).labelled_by(name.id).changed() {
                s.monthly_alert = text.trim().replace(',', ".").parse::<f64>().ok().filter(|v| v.is_finite() && *v > 0.0);
            }
            ui.label(egui::RichText::new("$").color(t.text_faint));
        });
        ui.add_space(10.0);
        ui.add(widgets::checkbox(&mut app.assistant.settings.enabled, tl!("Turn the assistant on")));
        if let Some(e) = &app.assistant.error {
            ui.colored_label(t.accent_text, e);
        }
        if app.assistant.settings.enabled && app.assistant_has_key(provider) && widgets::pill_button(ui, tl!("Done"), true).clicked() {
            app.assistant.setup = false;
        }
        ui.add_space(8.0);
        usage_lines(app, ui, t, None);
    });
}

/// This month's usage, and the last answer's.
fn usage_lines(app: &PdfCraftApp, ui: &mut egui::Ui, t: &Tokens, last: Option<&Turn>) {
    if let Some(turn) = last
        && let Some(u) = turn.usage
    {
        let cost = turn.cost.map(dollars).unwrap_or_else(|| tl!("cost not estimated").to_string());
        ui.label(
            egui::RichText::new(crate::i18n::fmt(
                tl!("This answer: {in} tokens in, {out} out · {cost}"),
                &[("in", &grouped(u.input_tokens)), ("out", &grouped(u.output_tokens)), ("cost", &cost)],
            ))
            .small()
            .color(t.text_faint),
        );
    }
    let m = app.assistant.ledger.month(&this_month());
    let mut line = crate::i18n::fmt(
        tl!("This month: {n} requests · {tokens} tokens · {cost}"),
        &[("n", &m.requests.to_string()), ("tokens", &grouped(m.input_tokens.saturating_add(m.output_tokens))), ("cost", &dollars(m.cost_usd))],
    );
    if m.unpriced > 0 {
        line.push_str(&crate::i18n::fmt(tl!(" (+ {n} not priced)"), &[("n", &m.unpriced.to_string())]));
    }
    let over = app.assistant.settings.monthly_alert.is_some_and(|a| m.cost_usd > a);
    ui.label(egui::RichText::new(line).small().color(if over { t.accent_text } else { t.text_faint }));
    if over {
        ui.label(egui::RichText::new(tl!("Past your monthly alert.")).small().color(t.accent_text));
    }
}

fn conversation(app: &mut PdfCraftApp, ui: &mut egui::Ui, t: &Tokens) {
    let Some((_, id)) = app.active_ids() else { return };
    let turns = app.assistant.chats.get(&id).cloned().unwrap_or_default();
    let footer = 150.0;
    let mut apply = None;
    let mut export = None;
    let mut toggles: Vec<(usize, usize)> = Vec::new();
    egui::ScrollArea::vertical().auto_shrink([false, false]).max_height((ui.available_height() - footer).max(80.0)).stick_to_bottom(true).show(
        ui,
        |ui| {
            if turns.is_empty() {
                ui.add_space(6.0);
                ui.label(egui::RichText::new(tl!("Ask about this document, or start with:")).color(t.text_muted));
                let mut chosen = None;
                for s in app.assistant.who.suggestions() {
                    if widgets::pill_button(ui, tl!(s), false).clicked() {
                        chosen = Some(tl!(s).to_string());
                    }
                }
                if let Some(q) = chosen {
                    app.ask_assistant(&q);
                }
            }
            for (i, turn) in turns.iter().enumerate() {
                match turn.who {
                    None => {
                        ui.with_layout(egui::Layout::top_down(egui::Align::Max), |ui| {
                            egui::Frame::NONE
                                .fill(t.accent_soft)
                                .corner_radius(CornerRadius::same(12))
                                .inner_margin(egui::Margin::symmetric(10, 8))
                                .show(ui, |ui| {
                                    ui.set_max_width(ui.available_width() * 0.85);
                                    ui.label(egui::RichText::new(&turn.text).color(t.text));
                                });
                        });
                    }
                    Some(who) => {
                        ui.horizontal_top(|ui| {
                            avatar(ui, who, 26.0);
                            ui.vertical(|ui| {
                                ui.label(egui::RichText::new(who.name()).font(theme::semibold(12.5)).color(t.text_muted));
                                let parsed = turn.parsed.clone().unwrap_or_default();
                                markdown(ui, &parsed.text, t);
                                if let Some(table) = &parsed.table {
                                    ui.add_space(4.0);
                                    egui::ScrollArea::horizontal().id_salt(("assistant-table", i)).show(ui, |ui| {
                                        egui::Grid::new(("assistant-grid", i)).striped(true).show(ui, |ui| {
                                            for c in &table.columns {
                                                ui.label(egui::RichText::new(c).strong());
                                            }
                                            ui.end_row();
                                            for row in table.rows.iter().take(40) {
                                                for c in row {
                                                    ui.label(c.as_text());
                                                }
                                                ui.end_row();
                                            }
                                        });
                                    });
                                    if table.rows.len() > 40 {
                                        ui.label(
                                            egui::RichText::new(crate::i18n::fmt(tl!("{n} rows in all"), &[("n", &table.rows.len().to_string())]))
                                                .small()
                                                .color(t.text_faint),
                                        );
                                    }
                                    ui.horizontal(|ui| {
                                        if widgets::pill_button(ui, tl!("Export to Excel…"), true).clicked() {
                                            export = Some(i);
                                        }
                                        if widgets::pill_button(ui, tl!("Copy"), false).clicked() {
                                            let mut tsv = table.columns.join("\t");
                                            for row in &table.rows {
                                                tsv.push('\n');
                                                tsv.push_str(&row.iter().map(Cell::as_text).collect::<Vec<_>>().join("\t"));
                                            }
                                            ui.ctx().copy_text(tsv);
                                        }
                                    });
                                }
                                if !parsed.actions.is_empty() {
                                    ui.add_space(4.0);
                                    for (k, action) in parsed.actions.iter().enumerate() {
                                        let mut on = turn.chosen.get(k).copied().unwrap_or(false);
                                        let label = match action {
                                            Action::Highlight { text } => crate::i18n::fmt(tl!("Highlight “{text}”"), &[("text", text)]),
                                            Action::Redact { text } => crate::i18n::fmt(tl!("Mark “{text}” for redaction"), &[("text", text)]),
                                            Action::Bookmark { title, page } => crate::i18n::fmt(
                                                tl!("Bookmark “{title}” → page {page}"),
                                                &[("title", title), ("page", &page.to_string())],
                                            ),
                                            Action::GoToPage { page } => crate::i18n::fmt(tl!("Go to page {page}"), &[("page", &page.to_string())]),
                                            Action::ExtractPages { pages } => crate::i18n::fmt(tl!("Extract pages {pages}"), &[("pages", pages)]),
                                        };
                                        if ui.add_enabled(!turn.applied, widgets::checkbox(&mut on, label)).changed() {
                                            toggles.push((i, k));
                                        }
                                    }
                                    if turn.applied {
                                        ui.label(egui::RichText::new(tl!("Done. Undo takes it back.")).small().color(t.text_faint));
                                    } else if widgets::pill_button(ui, tl!("Carry out the ticked actions"), true).clicked() {
                                        apply = Some(i);
                                    }
                                }
                                if turn.truncated {
                                    ui.label(egui::RichText::new(tl!("The answer stopped at its length limit.")).small().color(t.accent_text));
                                }
                                let pages =
                                    if turn.pages.is_empty() { String::new() } else { crate::i18n::fmt(tl!("pages {p}"), &[("p", &turn.pages)]) };
                                let meta = [pages, turn.model.clone()].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · ");
                                if !meta.is_empty() {
                                    ui.label(egui::RichText::new(meta).small().color(t.text_faint));
                                }
                            });
                        });
                    }
                }
                ui.add_space(8.0);
            }
            if app.assistant.busy() {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(
                        egui::RichText::new(crate::i18n::fmt(tl!("{name} is reading…"), &[("name", app.assistant.who.name())])).color(t.text_muted),
                    );
                });
            }
        },
    );
    for (i, k) in toggles {
        if let Some(c) = app.assistant.chats.get_mut(&id).and_then(|c| c.get_mut(i)).and_then(|t| t.chosen.get_mut(k)) {
            *c = !*c;
        }
    }
    if let Some(i) = apply {
        app.apply_assistant_actions(i);
    }
    if let Some(i) = export {
        app.export_assistant_table(i);
    }

    // Footer: what will be sent, the question, and the usage.
    ui.separator();
    if let Some(e) = app.assistant.error.clone() {
        ui.colored_label(t.accent_text, e);
    }
    let pages = app.session.get(id).map_or(0, |d| d.info.pages.len());
    let budget = app.assistant.settings.max_context_chars;
    ui.label(
        egui::RichText::new(crate::i18n::fmt(
            tl!("Sent to {provider}: the relevant pages of this document ({pages} in all, up to about {tokens} tokens) and your question."),
            &[("provider", app.assistant.settings.provider.label()), ("pages", &pages.to_string()), ("tokens", &grouped(tokens_for(budget) as u64))],
        ))
        .small()
        .color(t.text_faint),
    );
    let mut send = false;
    ui.horizontal(|ui| {
        let edit = egui::TextEdit::multiline(&mut app.assistant.input)
            .hint_text(crate::i18n::fmt(tl!("Ask {name}…"), &[("name", app.assistant.who.name())]))
            .desired_rows(2)
            .desired_width(ui.available_width() - 80.0)
            .char_limit(8000);
        let resp = ui.add(edit);
        // Enter sends; Shift+Enter adds a line.
        if resp.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter) && !i.modifiers.shift) {
            app.assistant.input = app.assistant.input.trim_end_matches('\n').to_string();
            send = true;
        }
        if app.assistant.busy() {
            if widgets::pill_button(ui, tl!("Stop"), false).clicked() {
                app.cancel_assistant();
            }
        } else if widgets::pill_button(ui, tl!("Send"), true).clicked() {
            send = true;
        }
    });
    if send && !app.assistant.busy() {
        let q = std::mem::take(&mut app.assistant.input);
        app.ask_assistant(&q);
    }
    let last = turns.iter().rev().find(|t| t.who.is_some());
    usage_lines(app, ui, t, last);
}
