//! Requests to Flowise run on their own thread; the pane polls for the answer each frame.

use std::sync::mpsc::{self, Receiver, TryRecvError};

use wordcraft_assistant::flowise::{FlowiseAssistant, FlowiseConfig};
use wordcraft_assistant::{AssistantError, Suggestion, Task, WritingAssistant};

type Answer = Result<Vec<Suggestion>, AssistantError>;

/// A request on its way.
pub(crate) struct Pending(Receiver<Answer>);

impl Pending {
    /// Send `text` for `task` on a new thread; `ctx` is woken when the answer is in.
    pub(crate) fn start(config: FlowiseConfig, task: Task, text: String, ctx: egui::Context) -> Self {
        let (tx, rx) = mpsc::channel();
        let spawned = std::thread::Builder::new().name("assistant".into()).spawn(move || {
            let answer = FlowiseAssistant::new(config).ask(task, &text);
            let _ = tx.send(answer);
            ctx.request_repaint();
        });
        if let Err(e) = spawned {
            let (tx, rx) = mpsc::channel();
            let _ = tx.send(Err(AssistantError::Unreachable(format!("couldn't start the request: {e}"))));
            return Self(rx);
        }
        Self(rx)
    }

    /// The answer, once it is in.
    pub(crate) fn poll(&mut self) -> Option<Answer> {
        match self.0.try_recv() {
            Ok(answer) => Some(answer),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(Err(AssistantError::Unreachable("the request stopped without an answer".into()))),
        }
    }
}

/// The Flowise settings from the environment, over the defaults.
pub(crate) fn config_from_env() -> FlowiseConfig {
    let var = |name: &str| std::env::var(name).ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
    let mut c = FlowiseConfig::default();
    if let Some(url) = var("WORDCRAFT_FLOWISE_URL") {
        c.base_url = url;
    }
    c.lektorat_flow = var("WORDCRAFT_FLOWISE_LEKTORAT").unwrap_or_default();
    c.recherche_flow = var("WORDCRAFT_FLOWISE_RECHERCHE").unwrap_or_default();
    c.konsistenz_flow = var("WORDCRAFT_FLOWISE_KONSISTENZ").unwrap_or_default();
    c.api_key = var("WORDCRAFT_FLOWISE_API_KEY");
    c
}
