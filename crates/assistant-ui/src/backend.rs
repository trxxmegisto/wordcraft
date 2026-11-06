//! Requests to Flowise run on their own thread; the pane polls for the answer each frame.

use std::path::Path;
use std::sync::mpsc::{self, Receiver, TryRecvError};

use wordcraft_assistant::flowise::{FlowiseAssistant, FlowiseConfig};
use wordcraft_assistant::{AssistantError, Suggestion, Task, ask_portions};

type Answer = Result<Vec<Suggestion>, AssistantError>;

enum Message {
    /// Portion `.0` of `.1` (counting from 0) is being asked about.
    Progress(usize, usize),
    Done(Answer),
}

/// A request on its way.
pub(crate) struct Pending {
    rx: Receiver<Message>,
    /// The portion being asked about and how many there are (counting from 0).
    pub(crate) progress: (usize, usize),
}

impl Pending {
    /// Send `portions` for `task` one after another on a new thread; `ctx` is woken at each step.
    pub(crate) fn start(config: FlowiseConfig, task: Task, portions: Vec<String>, ctx: egui::Context) -> Self {
        let total = portions.len();
        let (tx, rx) = mpsc::channel();
        let spawned = std::thread::Builder::new().name("assistant".into()).spawn(move || {
            let assistant = FlowiseAssistant::new(config);
            let answer = ask_portions(&assistant, task, &portions, |done, total| {
                let _ = tx.send(Message::Progress(done, total));
                ctx.request_repaint();
            });
            let _ = tx.send(Message::Done(answer));
            ctx.request_repaint();
        });
        if let Err(e) = spawned {
            let (tx, rx) = mpsc::channel();
            let _ = tx.send(Message::Done(Err(AssistantError::Unreachable(format!("Die Anfrage konnte nicht gestartet werden ({e}).")))));
            return Self { rx, progress: (0, total) };
        }
        Self { rx, progress: (0, total) }
    }

    /// The answer, once it is in; meanwhile keeps [`Pending::progress`] current.
    pub(crate) fn poll(&mut self) -> Option<Answer> {
        loop {
            match self.rx.try_recv() {
                Ok(Message::Progress(done, total)) => self.progress = (done, total),
                Ok(Message::Done(answer)) => return Some(answer),
                Err(TryRecvError::Empty) => return None,
                Err(TryRecvError::Disconnected) => return Some(Err(AssistantError::Unreachable("Die Anfrage endete ohne Antwort.".into()))),
            }
        }
    }
}

/// `base` with the `WORDCRAFT_FLOWISE_*` environment variables over it; the API key only ever
/// comes from the environment.
pub(crate) fn with_env(mut c: FlowiseConfig) -> FlowiseConfig {
    let var = |name: &str| std::env::var(name).ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
    if let Some(url) = var("WORDCRAFT_FLOWISE_URL") {
        c.base_url = url;
    }
    if let Some(flow) = var("WORDCRAFT_FLOWISE_LEKTORAT") {
        c.lektorat_flow = flow;
    }
    if let Some(flow) = var("WORDCRAFT_FLOWISE_RECHERCHE") {
        c.recherche_flow = flow;
    }
    if let Some(flow) = var("WORDCRAFT_FLOWISE_KONSISTENZ") {
        c.konsistenz_flow = flow;
    }
    c.api_key = var("WORDCRAFT_FLOWISE_API_KEY");
    c
}

/// Settings files bigger than this are ignored (they hold a few short strings).
const MAX_SETTINGS_BYTES: u64 = 64 * 1024;

/// The settings saved in `path`; the defaults when there are none or they can't be read.
pub(crate) fn load(path: &Path) -> FlowiseConfig {
    let read = || -> Option<FlowiseConfig> {
        if std::fs::metadata(path).ok()?.len() > MAX_SETTINGS_BYTES {
            log::warn!("assistant: {} is too big; ignored", path.display());
            return None;
        }
        let text = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&text).map_err(|e| log::warn!("assistant: {} can't be read: {e}", path.display())).ok()
    };
    read().unwrap_or_default()
}

/// Save `config` (without the API key) to `path`, through a temporary file so a crash never
/// leaves half a file.
pub(crate) fn save(path: &Path, config: &FlowiseConfig) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let text = serde_json::to_string_pretty(config).map_err(std::io::Error::other)?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
}
