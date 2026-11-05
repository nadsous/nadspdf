//! The providers NadsPDF talks to with the user's key, the request it sends and the reply it
//! reads. Building and reading the JSON is here; sending it is `net` (desktop only).

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// Where requests go. The user's key is for one of these.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Provider {
    /// Claude, from Anthropic (`api.anthropic.com`).
    #[default]
    Anthropic,
    /// OpenRouter (`openrouter.ai`): many providers' models with one key; it reports each
    /// request's cost.
    OpenRouter,
    /// Mistral AI (`api.mistral.ai`), in France.
    Mistral,
}

impl Provider {
    pub const ALL: [Provider; 3] = [Provider::Anthropic, Provider::OpenRouter, Provider::Mistral];

    pub fn id(self) -> &'static str {
        match self {
            Provider::Anthropic => "anthropic",
            Provider::OpenRouter => "openrouter",
            Provider::Mistral => "mistral",
        }
    }

    pub fn from_id(id: &str) -> Option<Provider> {
        Provider::ALL.into_iter().find(|p| p.id() == id)
    }

    pub fn label(self) -> &'static str {
        match self {
            Provider::Anthropic => "Anthropic (Claude)",
            Provider::OpenRouter => "OpenRouter",
            Provider::Mistral => "Mistral AI",
        }
    }

    /// The address requests are sent to.
    pub fn endpoint(self) -> &'static str {
        match self {
            Provider::Anthropic => "https://api.anthropic.com/v1/messages",
            Provider::OpenRouter => "https://openrouter.ai/api/v1/chat/completions",
            Provider::Mistral => "https://api.mistral.ai/v1/chat/completions",
        }
    }

    /// Where the user gets a key.
    pub fn keys_page(self) -> &'static str {
        match self {
            Provider::Anthropic => "https://console.anthropic.com/settings/keys",
            Provider::OpenRouter => "https://openrouter.ai/settings/keys",
            Provider::Mistral => "https://console.mistral.ai/api-keys",
        }
    }

    /// Models offered in the picker (any other model id can be typed).
    pub fn models(self) -> &'static [&'static str] {
        match self {
            Provider::Anthropic => &["claude-haiku-5-5", "claude-sonnet-5-5", "claude-opus-5-5"],
            Provider::OpenRouter => &["openrouter/auto"],
            Provider::Mistral => &["mistral-small-latest", "mistral-medium-latest", "mistral-large-latest"],
        }
    }

    pub fn default_model(self) -> &'static str {
        match self {
            Provider::Anthropic => "claude-sonnet-5-5",
            Provider::OpenRouter => "openrouter/auto",
            Provider::Mistral => "mistral-medium-latest",
        }
    }

    /// Whether the provider reports what a request cost (OpenRouter does).
    pub fn reports_cost(self) -> bool {
        self == Provider::OpenRouter
    }
}

/// The user's choices. The key is not here: it lives in the system's credential store.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// The assistant is off until the user turns it on and gives a key.
    pub enabled: bool,
    pub provider: Provider,
    pub model: String,
    /// The provider's prices, in US dollars per million input and output tokens, as the user
    /// copied them from the provider's page: NadsPDF doesn't guess prices. `None`: the cost is
    /// not estimated (OpenRouter reports it anyway).
    pub price_input: Option<f64>,
    pub price_output: Option<f64>,
    /// The longest answer asked for, in tokens.
    pub max_output_tokens: u32,
    /// The most document text sent with one question, in characters.
    pub max_context_chars: usize,
    /// A monthly spending line in US dollars: past it, the panel says so (nothing is blocked).
    pub monthly_alert: Option<f64>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            enabled: false,
            provider: Provider::Anthropic,
            model: Provider::Anthropic.default_model().into(),
            price_input: None,
            price_output: None,
            max_output_tokens: 4096,
            max_context_chars: 120_000,
            monthly_alert: None,
        }
    }
}

impl Settings {
    /// The cost of `usage` in US dollars: the provider's own figure when it gave one, else from
    /// the prices the user entered, else unknown.
    pub fn cost(&self, usage: &Usage) -> Option<f64> {
        if usage.cost_usd.is_some() {
            return usage.cost_usd;
        }
        let (pi, po) = (self.price_input?, self.price_output?);
        let c = (usage.input_tokens as f64 * pi + usage.output_tokens as f64 * po) / 1_000_000.0;
        c.is_finite().then_some(c)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Role {
    User,
    Assistant,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: String,
}

/// One request: the assistant's instructions, the conversation so far, and the answer's length.
#[derive(Clone, Debug, PartialEq)]
pub struct Request {
    pub system: String,
    pub messages: Vec<Message>,
    pub max_tokens: u32,
}

/// What one request used.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    /// What the provider says the request cost, in US dollars (OpenRouter).
    pub cost_usd: Option<f64>,
}

impl Usage {
    pub fn add(&mut self, other: &Usage) {
        self.input_tokens = self.input_tokens.saturating_add(other.input_tokens);
        self.output_tokens = self.output_tokens.saturating_add(other.output_tokens);
        self.cost_usd = match (self.cost_usd, other.cost_usd) {
            (Some(a), Some(b)) => Some(a + b),
            (a, b) => a.or(b),
        };
    }
}

/// The provider's answer.
#[derive(Clone, Debug, PartialEq)]
pub struct Reply {
    pub text: String,
    pub usage: Usage,
    /// The model that answered (OpenRouter's auto picks one).
    pub model: String,
    /// The answer stopped at the length limit.
    pub truncated: bool,
}

fn role(r: Role) -> &'static str {
    match r {
        Role::User => "user",
        Role::Assistant => "assistant",
    }
}

/// The JSON body for `request` to `provider` with `model`.
pub fn body(provider: Provider, model: &str, request: &Request) -> Value {
    let messages: Vec<Value> = request.messages.iter().map(|m| json!({ "role": role(m.role), "content": m.content })).collect();
    match provider {
        Provider::Anthropic => json!({
            "model": model,
            "max_tokens": request.max_tokens,
            "system": request.system,
            "messages": messages,
        }),
        Provider::OpenRouter | Provider::Mistral => {
            let mut all = vec![json!({ "role": "system", "content": request.system })];
            all.extend(messages);
            let mut body = json!({ "model": model, "max_tokens": request.max_tokens, "messages": all });
            if provider == Provider::OpenRouter
                && let Some(obj) = body.as_object_mut()
            {
                // Ask OpenRouter to report the request's cost with its usage.
                obj.insert("usage".into(), json!({ "include": true }));
            }
            body
        }
    }
}

fn number(v: &Value) -> u64 {
    v.as_u64().unwrap_or(0)
}

/// Read a successful answer's body.
pub fn parse_reply(provider: Provider, body: &str) -> Result<Reply, String> {
    let v: Value = serde_json::from_str(body).map_err(|e| format!("the provider's answer can't be read ({e})"))?;
    match provider {
        Provider::Anthropic => {
            let text: String =
                v["content"].as_array().map(|parts| parts.iter().filter_map(|p| p["text"].as_str()).collect::<Vec<_>>().join("")).unwrap_or_default();
            let u = &v["usage"];
            // Cached prompt reads and writes count as input.
            let input = number(&u["input_tokens"])
                .saturating_add(number(&u["cache_read_input_tokens"]))
                .saturating_add(number(&u["cache_creation_input_tokens"]));
            Ok(Reply {
                text,
                usage: Usage { input_tokens: input, output_tokens: number(&u["output_tokens"]), cost_usd: None },
                model: v["model"].as_str().unwrap_or_default().to_string(),
                truncated: v["stop_reason"].as_str() == Some("max_tokens"),
            })
        }
        Provider::OpenRouter | Provider::Mistral => {
            let choice = &v["choices"][0];
            let text = match &choice["message"]["content"] {
                Value::String(s) => s.clone(),
                // Some models answer with a list of parts.
                Value::Array(parts) => parts.iter().filter_map(|p| p["text"].as_str()).collect::<Vec<_>>().join(""),
                _ => String::new(),
            };
            let u = &v["usage"];
            let cost = u["cost"].as_f64().filter(|c| c.is_finite() && *c >= 0.0);
            Ok(Reply {
                text,
                usage: Usage { input_tokens: number(&u["prompt_tokens"]), output_tokens: number(&u["completion_tokens"]), cost_usd: cost },
                model: v["model"].as_str().unwrap_or_default().to_string(),
                truncated: choice["finish_reason"].as_str() == Some("length"),
            })
        }
    }
}

/// What to tell the user when the provider answers with an error status.
pub fn error_message(provider: Provider, status: u16, body: &str) -> String {
    let detail = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| v["error"]["message"].as_str().or(v["message"].as_str()).or(v["detail"].as_str()).map(str::to_string))
        .map(|d| d.chars().take(300).collect::<String>())
        .unwrap_or_default();
    let what = match status {
        401 | 403 => format!("{} refused the key: check it in Preferences ▸ Assistant.", provider.label()),
        402 => format!("{}: the account has no credit left.", provider.label()),
        404 => "the model wasn't found: check its name in Preferences ▸ Assistant.".to_string(),
        413 => "the document is too long for this model: ask about fewer pages.".to_string(),
        429 => format!("{} limits how often you can ask: wait a moment and try again.", provider.label()),
        500..=599 => format!("{} is unavailable at the moment (error {status}).", provider.label()),
        _ => format!("{} answered with error {status}.", provider.label()),
    };
    if detail.is_empty() { what } else { format!("{what} ({detail})") }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> Request {
        Request { system: "Be brief.".into(), messages: vec![Message { role: Role::User, content: "Hi".into() }], max_tokens: 100 }
    }

    #[test]
    fn bodies_follow_each_providers_format() {
        let a = body(Provider::Anthropic, "claude-sonnet-5-5", &request());
        assert_eq!(a["system"], "Be brief.");
        assert_eq!(a["messages"][0]["role"], "user");
        let o = body(Provider::OpenRouter, "openrouter/auto", &request());
        assert_eq!(o["messages"][0]["role"], "system");
        assert_eq!(o["usage"]["include"], true, "OpenRouter reports the cost");
        let m = body(Provider::Mistral, "mistral-small-latest", &request());
        assert!(m.get("usage").is_none());
    }

    #[test]
    fn replies_and_usage_are_read() {
        let a = r#"{"model":"claude-sonnet-5-5","content":[{"type":"text","text":"Bon"},{"type":"text","text":"jour"}],"stop_reason":"end_turn","usage":{"input_tokens":10,"cache_read_input_tokens":5,"output_tokens":3}}"#;
        let r = parse_reply(Provider::Anthropic, a).unwrap();
        assert_eq!((r.text.as_str(), r.usage.input_tokens, r.usage.output_tokens, r.truncated), ("Bonjour", 15, 3, false));
        let o = r#"{"model":"m","choices":[{"message":{"content":"ok"},"finish_reason":"length"}],"usage":{"prompt_tokens":7,"completion_tokens":2,"cost":0.0012}}"#;
        let r = parse_reply(Provider::OpenRouter, o).unwrap();
        assert_eq!((r.usage.cost_usd, r.truncated), (Some(0.0012), true));
        assert!(parse_reply(Provider::Mistral, "not json").is_err());
        // Missing pieces read as empty, never a panic.
        let r = parse_reply(Provider::Mistral, "{}").unwrap();
        assert_eq!(r.text, "");
    }

    #[test]
    fn cost_comes_from_the_provider_or_the_users_prices_never_a_guess() {
        let mut s = Settings::default();
        let u = Usage { input_tokens: 1_000_000, output_tokens: 500_000, cost_usd: None };
        assert_eq!(s.cost(&u), None, "no prices entered: not estimated");
        s.price_input = Some(3.0);
        s.price_output = Some(15.0);
        assert_eq!(s.cost(&u), Some(10.5));
        assert_eq!(s.cost(&Usage { cost_usd: Some(0.2), ..u }), Some(0.2), "the provider's figure wins");
    }

    #[test]
    fn errors_say_what_to_do() {
        assert!(error_message(Provider::Mistral, 401, "{}").contains("refused the key"));
        assert!(error_message(Provider::Anthropic, 429, r#"{"error":{"message":"slow down"}}"#).contains("slow down"));
    }
}
