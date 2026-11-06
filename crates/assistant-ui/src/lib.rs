//! The writing assistant's task pane (KWordCraft fork): pick a task (Lektorat, Recherche,
//! Konsistenz), send the selection or the whole document to the Flowise workflow, review the
//! suggestions and put the chosen ones into the document as tracked changes and comments.
//!
//! The pane plugs into `wordcraft-ui-egui` through its extension point
//! ([`wordcraft_ui_egui::panes::PaneExtension`]); the work is done by `wordcraft-assistant`.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use egui::{RichText, Ui};
use wordcraft_assistant::{AssistantError, DEFAULT_AUTHOR, Kind, Suggestion, Task, apply};
use wordcraft_ui_egui::WordApp;
use wordcraft_ui_egui::panes::PaneExtension;

#[cfg(not(target_arch = "wasm32"))]
mod backend;

/// The pane's key (`ui.extensionPane {"pane": "assistant"}`, `ui.paneDock`).
pub const KEY: &str = "assistant";

/// Longest text sent in one request, in characters (a long manuscript goes chapter by chapter).
pub const MAX_INPUT_CHARS: usize = 200_000;

/// Register the assistant pane on `app`; it opens at start when a Flowise chatflow is set up.
/// `ui.extensionPane {"pane": "assistant"}` shows or hides it.
pub fn install(app: &mut WordApp) {
    if let Err(e) = app.register_pane(Box::new(AssistantPane::new().open_if_configured())) {
        log::warn!("assistant: {e}");
    }
}

/// What the assistant analyses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// The selected text (the whole document when nothing is selected).
    Selection,
    /// The document's main text.
    Document,
}

/// One suggestion in the list and whether it is ticked for applying.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub suggestion: Suggestion,
    pub chosen: bool,
}

/// The assistant pane's state.
pub struct AssistantPane {
    open: bool,
    pub task: Task,
    pub scope: Scope,
    pub rows: Vec<Row>,
    /// The last outcome shown under the buttons.
    pub message: Option<String>,
    #[cfg(not(target_arch = "wasm32"))]
    pub config: wordcraft_assistant::flowise::FlowiseConfig,
    #[cfg(not(target_arch = "wasm32"))]
    pending: Option<backend::Pending>,
}

impl Default for AssistantPane {
    fn default() -> Self {
        Self {
            open: false,
            task: Task::Lektorat,
            scope: Scope::Selection,
            rows: Vec::new(),
            message: None,
            #[cfg(not(target_arch = "wasm32"))]
            config: backend::config_from_env(),
            #[cfg(not(target_arch = "wasm32"))]
            pending: None,
        }
    }
}

impl AssistantPane {
    /// A closed pane, set up from the environment (`WORDCRAFT_FLOWISE_URL`,
    /// `WORDCRAFT_FLOWISE_LEKTORAT`, `…_RECHERCHE`, `…_KONSISTENZ`, `WORDCRAFT_FLOWISE_API_KEY`).
    pub fn new() -> Self {
        Self::default()
    }

    /// Open the pane at start when a Flowise chatflow is set up.
    pub fn open_if_configured(mut self) -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.open = Task::ALL.into_iter().any(|t| self.config.endpoint(t).is_ok());
        }
        self
    }

    /// Whether a request is on its way.
    pub fn busy(&self) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.pending.is_some()
        }
        #[cfg(target_arch = "wasm32")]
        {
            false
        }
    }

    /// Take the answer of a request that has finished.
    pub fn receive(&mut self, answer: Result<Vec<Suggestion>, AssistantError>) {
        match answer {
            Ok(list) if list.is_empty() => {
                self.rows.clear();
                self.message = Some("Keine Anmerkungen.".into());
            }
            Ok(list) => {
                self.message = Some(format!("{} Vorschläge.", list.len()));
                self.rows = list.into_iter().map(|suggestion| Row { suggestion, chosen: true }).collect();
            }
            Err(e) => self.message = Some(e.to_string()),
        }
    }

    /// Put the ticked suggestions into the document and drop them from the list.
    pub fn apply_chosen(&mut self, app: &mut WordApp) {
        let chosen: Vec<Suggestion> = self.rows.iter().filter(|r| r.chosen).map(|r| r.suggestion.clone()).collect();
        if chosen.is_empty() {
            self.message = Some("Kein Vorschlag ausgewählt.".into());
            return;
        }
        let report = apply(&mut app.session, &chosen, DEFAULT_AUTHOR);
        self.rows.retain(|r| !r.chosen);
        let mut msg = format!("{} übernommen.", report.applied);
        if !report.not_found.is_empty() {
            msg.push_str(&format!(" {} Textstellen nicht gefunden.", report.not_found.len()));
        }
        if !report.failed.is_empty() {
            msg.push_str(&format!(" {} fehlgeschlagen.", report.failed.len()));
        }
        self.message = Some(msg);
    }
}

/// The text to analyse: the selection, else the whole main text; capped at [`MAX_INPUT_CHARS`].
pub fn input_text(app: &WordApp, scope: Scope) -> String {
    let selected = app.session.selected_text();
    let text = match scope {
        Scope::Selection if !selected.trim().is_empty() => selected,
        _ => app.session.doc.final_text(wordcraft_engine::doc::StoryRef::Body),
    };
    text.chars().take(MAX_INPUT_CHARS).collect()
}

impl PaneExtension for AssistantPane {
    fn key(&self) -> &'static str {
        KEY
    }

    fn title(&self) -> String {
        "Assistent".into()
    }

    fn is_open(&self) -> bool {
        self.open
    }

    fn set_open(&mut self, open: bool) {
        self.open = open;
    }

    fn body(&mut self, app: &mut WordApp, ui: &mut Ui) {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(answer) = self.pending.as_mut().and_then(backend::Pending::poll) {
            self.pending = None;
            self.receive(answer);
        }
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                for task in Task::ALL {
                    ui.selectable_value(&mut self.task, task, task.label());
                }
            });
            ui.horizontal_wrapped(|ui| {
                ui.radio_value(&mut self.scope, Scope::Selection, "Markierung");
                ui.radio_value(&mut self.scope, Scope::Document, "Ganzes Dokument");
            });
            ui.add_space(4.0);
            self.ask_button(app, ui);
            if let Some(msg) = &self.message {
                ui.label(msg);
            }
            if self.rows.is_empty() {
                return;
            }
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("Ausgewählte übernehmen").clicked() {
                    self.apply_chosen(app);
                }
                if ui.button("Verwerfen").clicked() {
                    self.rows.clear();
                }
            });
            for row in &mut self.rows {
                ui.separator();
                let s = &row.suggestion;
                let head = match s.kind {
                    Kind::Replace => format!("„{}“ → „{}“", s.original, s.replacement),
                    Kind::Comment => format!("„{}“", s.original),
                };
                ui.checkbox(&mut row.chosen, RichText::new(head).strong());
                if !s.reason.is_empty() {
                    ui.label(&s.reason);
                }
            }
        });
        #[cfg(not(target_arch = "wasm32"))]
        settings(ui, &mut self.config);
    }
}

impl AssistantPane {
    #[cfg(not(target_arch = "wasm32"))]
    fn ask_button(&mut self, app: &mut WordApp, ui: &mut Ui) {
        if self.busy() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(format!("{} läuft …", self.task.label()));
            });
            return;
        }
        if ui.button(format!("{} starten", self.task.label())).clicked() {
            let text = input_text(app, self.scope);
            if text.trim().is_empty() {
                self.message = Some("Der Text ist leer.".into());
                return;
            }
            self.message = None;
            self.rows.clear();
            self.pending = Some(backend::Pending::start(self.config.clone(), self.task, text, ui.ctx().clone()));
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn ask_button(&mut self, _app: &mut WordApp, ui: &mut Ui) {
        ui.label("Der Assistent ist in der Web-Version nicht verfügbar.");
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn settings(ui: &mut Ui, config: &mut wordcraft_assistant::flowise::FlowiseConfig) {
    egui::CollapsingHeader::new("Verbindung").default_open(config.lektorat_flow.is_empty()).show(ui, |ui| {
        egui::Grid::new("assistant_settings").num_columns(2).show(ui, |ui| {
            ui.label("Flowise");
            ui.text_edit_singleline(&mut config.base_url);
            ui.end_row();
            ui.label("Lektorat");
            ui.text_edit_singleline(&mut config.lektorat_flow);
            ui.end_row();
            ui.label("Recherche");
            ui.text_edit_singleline(&mut config.recherche_flow);
            ui.end_row();
            ui.label("Konsistenz");
            ui.text_edit_singleline(&mut config.konsistenz_flow);
            ui.end_row();
        });
    });
}

#[cfg(test)]
mod tests;
