//! Flowise backend: each task is a chatflow, called through Flowise's prediction API
//! (`POST {base}/api/v1/prediction/{flow}` with `{"question": ...}`, answer in `text`).

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{AssistantError, Suggestion, Task, WritingAssistant, parse_suggestions, question};

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
            api_key: None,
        }
    }
}

impl FlowiseConfig {
    /// The chatflow id for `task` (may be empty).
    pub fn flow(&self, task: Task) -> &str {
        match task {
            Task::Lektorat => &self.lektorat_flow,
            Task::Recherche => &self.recherche_flow,
            Task::Konsistenz => &self.konsistenz_flow,
        }
    }

    /// The prediction URL for `task`, checked: an `http(s)` base and a flow id of letters,
    /// digits and dashes only (a Flowise UUID), so nothing can be smuggled into the path.
    pub fn endpoint(&self, task: Task) -> Result<String, AssistantError> {
        let base = self.base_url.trim().trim_end_matches('/');
        if !(base.starts_with("http://") || base.starts_with("https://")) {
            return Err(AssistantError::NotConfigured("the Flowise address must start with http:// or https://".into()));
        }
        let flow = self.flow(task).trim();
        if flow.is_empty() {
            return Err(AssistantError::NotConfigured(format!("no Flowise chatflow set for {}", task.label())));
        }
        if !flow.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            return Err(AssistantError::NotConfigured(format!("the chatflow id for {} isn't valid", task.label())));
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
        let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(timeout)).build().into();
        Self { config, agent }
    }

    /// The configuration in use.
    pub fn config(&self) -> &FlowiseConfig {
        &self.config
    }
}

impl WritingAssistant for FlowiseAssistant {
    fn ask(&self, task: Task, text: &str) -> Result<Vec<Suggestion>, AssistantError> {
        let url = self.config.endpoint(task)?;
        let body = json!({ "question": question(task, text) }).to_string();
        let mut request = self.agent.post(&url).header("Content-Type", "application/json");
        if let Some(key) = self.config.api_key.as_deref().filter(|k| !k.is_empty()) {
            request = request.header("Authorization", format!("Bearer {key}"));
        }
        let mut response = request.send(body.as_str()).map_err(map_error)?;
        let raw = response.body_mut().read_to_string().map_err(map_error)?;
        parse_suggestions(&answer_text(&raw)?)
    }
}

fn map_error(e: ureq::Error) -> AssistantError {
    match e {
        ureq::Error::StatusCode(code) => AssistantError::Http(code),
        other => AssistantError::Unreachable(other.to_string()),
    }
}

/// The model's answer inside a Flowise prediction response: `text`, or `json` for chatflows with
/// structured output.
pub fn answer_text(raw: &str) -> Result<String, AssistantError> {
    let v: Value = serde_json::from_str(raw).map_err(|e| AssistantError::Format(format!("Flowise didn't answer with JSON: {e}")))?;
    if let Some(text) = v.get("text").and_then(Value::as_str) {
        return Ok(text.to_string());
    }
    if let Some(structured) = v.get("json") {
        return Ok(structured.to_string());
    }
    Err(AssistantError::Format("Flowise's answer has no `text`".into()))
}
