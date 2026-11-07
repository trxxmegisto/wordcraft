//! Flowise backend: each task is a chatflow, called through Flowise's prediction API
//! (`POST {base}/api/v1/prediction/{flow}` with `{"question": ...}`, answer in `text`).

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{AssistantError, Reply, Suggestion, Task, WritingAssistant, parse_reply, parse_suggestions, question, question_with_facts};

/// Longest a call may take by default (long chapters through a large model take a while).
pub const DEFAULT_TIMEOUT_SECS: u64 = 180;

/// Where Flowise runs and which chatflow handles which task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FlowiseConfig {
    /// For example `http://localhost:3000`.
    pub base_url: String,
    /// Chatflow id for [`Task::Lektorat`].
    pub lektorat_flow: String,
    /// Chatflow id for [`Task::Recherche`].
    pub recherche_flow: String,
    /// Chatflow id for [`Task::Konsistenz`].
    pub konsistenz_flow: String,
    /// Seconds before a call gives up.
    pub timeout_secs: u64,
    /// Longest text sent in one request, in characters: longer texts go in portions
    /// ([`crate::portions`]). Set it to what the chatflow's model can take.
    pub portion_chars: usize,
    /// Flowise API key (sent as a bearer token). Never saved with the settings: the caller loads
    /// it from the operating system's credential store.
    #[serde(skip)]
    pub api_key: Option<String>,
}

impl Default for FlowiseConfig {
    fn default() -> Self {
        Self {
            base_url: "http://localhost:3000".into(),
            lektorat_flow: String::new(),
            recherche_flow: String::new(),
            konsistenz_flow: String::new(),
            timeout_secs: DEFAULT_TIMEOUT_SECS,
            portion_chars: crate::DEFAULT_PORTION_CHARS,
            api_key: None,
        }
    }
}

impl FlowiseConfig {
    /// The chatflow id for `task`: its own, else the Lektorat chatflow (one chatflow can serve
    /// every task, since the question carries the task's instructions). May be empty.
    pub fn flow(&self, task: Task) -> &str {
        let own = match task {
            Task::Lektorat => &self.lektorat_flow,
            Task::Recherche => &self.recherche_flow,
            Task::Konsistenz => &self.konsistenz_flow,
        };
        if own.trim().is_empty() { &self.lektorat_flow } else { own }
    }

    /// The prediction URL for `task`, checked: an `http(s)` base and a flow id of letters,
    /// digits and dashes only (a Flowise UUID), so nothing can be smuggled into the path.
    pub fn endpoint(&self, task: Task) -> Result<String, AssistantError> {
        let base = self.base_url.trim().trim_end_matches('/');
        if !(base.starts_with("http://") || base.starts_with("https://")) {
            return Err(AssistantError::NotConfigured("Die Flowise-Adresse muss mit http:// oder https:// beginnen.".into()));
        }
        let flow = self.flow(task).trim();
        if flow.is_empty() {
            return Err(AssistantError::NotConfigured(format!("Für {} ist keine Chatflow-ID eingetragen.", task.label())));
        }
        if !flow.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            return Err(AssistantError::NotConfigured(format!(
                "Die Chatflow-ID für {} ist ungültig (nur Buchstaben, Ziffern und Bindestriche).",
                task.label()
            )));
        }
        Ok(format!("{base}/api/v1/prediction/{flow}"))
    }
}

/// Talks to a Flowise instance.
pub struct FlowiseAssistant {
    config: FlowiseConfig,
    agent: ureq::Agent,
}

impl FlowiseAssistant {
    /// A client for `config`.
    pub fn new(config: FlowiseConfig) -> Self {
        let timeout = Duration::from_secs(config.timeout_secs.clamp(5, 3600));
        // No connection reuse: a long text goes in several requests, and a server that closed
        // the previous connection (as Node does after a few idle seconds) would fail the next one.
        let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(timeout)).max_idle_connections(0).build().into();
        Self { config, agent }
    }

    /// The configuration in use.
    pub fn config(&self) -> &FlowiseConfig {
        &self.config
    }
}

impl WritingAssistant for FlowiseAssistant {
    fn ask(&self, task: Task, text: &str) -> Result<Vec<Suggestion>, AssistantError> {
        parse_suggestions(&self.send(task, &question(task, text))?)
    }

    fn ask_with_facts(&self, task: Task, text: &str, known: &[String], collect: bool) -> Result<Reply, AssistantError> {
        parse_reply(&self.send(task, &question_with_facts(task, text, known, collect))?)
    }
}

impl FlowiseAssistant {
    /// Post `question` to `task`'s chatflow and return the answer's text.
    fn send(&self, task: Task, question: &str) -> Result<String, AssistantError> {
        let url = self.config.endpoint(task)?;
        let body = json!({ "question": question }).to_string();
        let mut request = self.agent.post(&url).header("Content-Type", "application/json");
        if let Some(key) = self.config.api_key.as_deref().filter(|k| !k.is_empty()) {
            request = request.header("Authorization", format!("Bearer {key}"));
        }
        let mut response = request.send(body.as_str()).map_err(map_error)?;
        let raw = response.body_mut().read_to_string().map_err(map_error)?;
        answer_text(&raw)
    }
}

fn map_error(e: ureq::Error) -> AssistantError {
    match e {
        ureq::Error::StatusCode(code) => AssistantError::Http(code),
        other => {
            let mut msg = other.to_string();
            if !msg.ends_with(['.', '!', '?']) {
                msg.push('.');
            }
            AssistantError::Unreachable(msg)
        }
    }
}

/// The model's answer inside a Flowise prediction response: `text`, or `json` for chatflows with
/// structured output.
pub fn answer_text(raw: &str) -> Result<String, AssistantError> {
    let v: Value = serde_json::from_str(raw).map_err(|e| AssistantError::Format(format!("Flowise hat kein JSON geliefert ({e}).")))?;
    if let Some(text) = v.get("text").and_then(Value::as_str) {
        return Ok(text.to_string());
    }
    if let Some(structured) = v.get("json") {
        return Ok(structured.to_string());
    }
    Err(AssistantError::Format("In der Antwort von Flowise fehlt das Feld `text`.".into()))
}
