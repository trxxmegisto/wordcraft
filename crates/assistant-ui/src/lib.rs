//! The writing assistant's task pane (KWordCraft fork): pick a task (Lektorat, Recherche,
//! Konsistenz), send the selection, a chapter or the whole document to the Flowise workflow (in
//! portions when it is long), review the
//! suggestions and put the chosen ones into the document as tracked changes and comments.
//!
//! The pane plugs into `wordcraft-ui-egui` through its extension point
//! ([`wordcraft_ui_egui::panes::PaneExtension`]); the work is done by `wordcraft-assistant`.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use egui::{RichText, Ui};
use wordcraft_assistant::{AssistantError, Chapter, Kind, Suggestion, Task, apply_as, chapter_at, chapter_text, chapters};
use wordcraft_ui_egui::WordApp;
use wordcraft_ui_egui::panes::PaneExtension;

#[cfg(not(target_arch = "wasm32"))]
mod backend;

/// The pane's key (`ui.extensionPane {"pane": "assistant"}`, `ui.paneDock`).
pub const KEY: &str = "assistant";

/// Longest text analysed in one run, in characters (about a long novel); it goes to Flowise in
/// portions of the size set under „Verbindung“.
pub const MAX_INPUT_CHARS: usize = 1_000_000;

/// The file in the settings folder that keeps the connection settings (never the API key).
pub const SETTINGS_FILE: &str = "assistant.json";

/// Register the assistant pane on `app`; it opens at start when a Flowise chatflow is set up.
/// `ui.extensionPane {"pane": "assistant"}` shows or hides it. The connection settings are kept
/// in [`SETTINGS_FILE`] in WordCraft's settings folder, unless `WORDCRAFT_NO_PREFS` is set.
///
/// It also turns off the update check at start (call it after the app turns that on): the
/// official updater would replace this build with upstream's, which has no assistant, and the
/// fork's version number lags upstream's releases, so it would always offer one. The fork is
/// updated with `kwordcraft/update-wordcraft.ps1`.
pub fn install(app: &mut WordApp) {
    #[cfg(not(target_arch = "wasm32"))]
    app.set_startup_update_check(false);
    #[cfg_attr(target_arch = "wasm32", expect(unused_mut))]
    let mut pane = AssistantPane::new();
    #[cfg(not(target_arch = "wasm32"))]
    if std::env::var_os("WORDCRAFT_NO_PREFS").is_none()
        && let Some(dir) = wordcraft_control_key::settings_dir()
    {
        pane = pane.with_settings_file(dir.join(SETTINGS_FILE));
    }
    if let Err(e) = app.register_pane(Box::new(pane.open_if_configured())) {
        log::warn!("assistant: {e}");
    }
}

/// What the assistant analyses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// The selected text (the whole document when nothing is selected).
    Selection,
    /// The chosen chapter: from its heading to the next heading of the same or a higher level.
    Chapter,
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
    /// The chapter picked for [`Scope::Chapter`] (None: the one with the cursor).
    pub chapter: Option<Chapter>,
    pub rows: Vec<Row>,
    /// The task the listed suggestions came from: their revisions and comments carry its author.
    pub rows_task: Task,
    /// The last outcome shown under the buttons.
    pub message: Option<String>,
    /// Whether [`AssistantPane::message`] reports a problem (shown in the error colour).
    pub message_is_error: bool,
    #[cfg(not(target_arch = "wasm32"))]
    pub config: wordcraft_assistant::flowise::FlowiseConfig,
    /// Where the connection settings are kept, and what was last written there.
    #[cfg(not(target_arch = "wasm32"))]
    settings: Option<(std::path::PathBuf, wordcraft_assistant::flowise::FlowiseConfig)>,
    #[cfg(not(target_arch = "wasm32"))]
    pending: Option<backend::Pending>,
}

impl Default for AssistantPane {
    fn default() -> Self {
        Self {
            open: false,
            task: Task::Lektorat,
            scope: Scope::Selection,
            chapter: None,
            rows: Vec::new(),
            rows_task: Task::Lektorat,
            message: None,
            message_is_error: false,
            #[cfg(not(target_arch = "wasm32"))]
            config: backend::with_env(Default::default()),
            #[cfg(not(target_arch = "wasm32"))]
            settings: None,
            #[cfg(not(target_arch = "wasm32"))]
            pending: None,
        }
    }
}

impl AssistantPane {
    /// A closed pane, set up from the environment (`WORDCRAFT_FLOWISE_URL`,
    /// `WORDCRAFT_FLOWISE_LEKTORAT`, `…_RECHERCHE`, `…_KONSISTENZ`, `WORDCRAFT_FLOWISE_API_KEY`).
    /// It keeps no settings file.
    pub fn new() -> Self {
        Self::default()
    }

    /// Keep the connection settings in `path`: read them now (the environment still wins), and
    /// write them whenever they are changed in the pane.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn with_settings_file(mut self, path: std::path::PathBuf) -> Self {
        let saved = backend::load(&path);
        self.config = backend::with_env(saved.clone());
        self.settings = Some((path, saved));
        self
    }

    /// Write the connection settings if they changed since they were last written.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn save_settings(&mut self) {
        let Some((path, saved)) = &mut self.settings else { return };
        if *saved == self.config {
            return;
        }
        if let Err(e) = backend::save(path, &self.config) {
            log::warn!("assistant: couldn't save {}: {e}", path.display());
        }
        // Also after a failure: don't retry every frame; the next change tries again.
        *saved = self.config.clone();
    }

    fn note(&mut self, text: impl Into<String>) {
        self.message = Some(text.into());
        self.message_is_error = false;
    }

    fn problem(&mut self, text: impl Into<String>) {
        self.message = Some(text.into());
        self.message_is_error = true;
    }

    /// Open the pane at start when a Flowise chatflow is set up.
    pub fn open_if_configured(#[cfg_attr(target_arch = "wasm32", expect(unused_mut))] mut self) -> Self {
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
                self.note("Keine Anmerkungen.");
            }
            Ok(list) => {
                // Research notes without a verdict start unticked: they are rarely worth a comment.
                let task = self.rows_task;
                self.rows = list.into_iter().map(|suggestion| Row { chosen: task.has_verdict(&suggestion.reason), suggestion }).collect();
                let unticked = self.rows.iter().filter(|r| !r.chosen).count();
                self.note(match unticked {
                    0 if self.rows.len() == 1 => "1 Vorschlag.".to_string(),
                    0 => format!("{} Vorschläge.", self.rows.len()),
                    1 => format!("{} Vorschläge; einer ohne Urteil ist nicht ausgewählt.", self.rows.len()),
                    n => format!("{} Vorschläge; {n} ohne Urteil sind nicht ausgewählt.", self.rows.len()),
                });
            }
            Err(e @ AssistantError::Unreachable(_)) => self.problem(format!("{e} Läuft Flowise unter der eingetragenen Adresse?")),
            Err(e @ AssistantError::NotConfigured(_)) => self.problem(format!("{e} Siehe „Verbindung“ unten.")),
            Err(e) => self.problem(e.to_string()),
        }
    }

    /// [`AssistantPane::receive`] for a text sent in `total` portions, the last one asked about
    /// being `at` (counting from 0): says how many were checked, or at which one it failed.
    pub fn receive_parts(&mut self, answer: Result<Vec<Suggestion>, AssistantError>, at: usize, total: usize) {
        self.receive(answer);
        if total > 1 {
            let msg = self.message.take().unwrap_or_default();
            self.message = Some(if self.message_is_error {
                format!("Bei Teil {} von {total}: {msg}", at + 1)
            } else {
                format!("{msg} ({total} Teile geprüft.)")
            });
        }
    }

    /// Put the ticked suggestions into the document and drop them from the list.
    pub fn apply_chosen(&mut self, app: &mut WordApp) {
        let chosen: Vec<Suggestion> = self.rows.iter().filter(|r| r.chosen).map(|r| r.suggestion.clone()).collect();
        if chosen.is_empty() {
            self.problem("Kein Vorschlag ausgewählt.");
            return;
        }
        let report = apply_as(&mut app.session, &chosen, self.rows_task.author(), self.rows_task.explains_replacements());
        // Applied rows leave the list; the ones that weren't found or failed stay, unticked, so
        // the writer sees which.
        let kept = |r: &Row| report.not_found.contains(&r.suggestion.original) || report.failed.iter().any(|(o, _)| *o == r.suggestion.original);
        self.rows.retain(|r| !r.chosen || kept(r));
        for r in &mut self.rows {
            r.chosen = false;
        }
        let msg = report_message(report.applied, report.not_found.len(), report.failed.len());
        if report.not_found.is_empty() && report.failed.is_empty() {
            self.note(msg);
        } else {
            self.problem(msg);
        }
    }
}

/// "2 übernommen. 1 Textstelle nicht gefunden." — singular and plural as German needs them.
pub fn report_message(applied: usize, not_found: usize, failed: usize) -> String {
    let mut msg = format!("{applied} übernommen.");
    match not_found {
        0 => {}
        1 => msg.push_str(" 1 Textstelle nicht gefunden."),
        n => msg.push_str(&format!(" {n} Textstellen nicht gefunden.")),
    }
    if failed > 0 {
        msg.push_str(&format!(" {failed} fehlgeschlagen."));
    }
    msg
}

/// How a suggestion is shown in the list.
pub fn headline(s: &Suggestion) -> String {
    match s.kind {
        Kind::Replace if s.replacement.is_empty() => format!("„{}“ streichen", s.original),
        Kind::Replace => format!("„{}“ → „{}“", s.original, s.replacement),
        Kind::Comment => format!("„{}“", s.original),
    }
}

/// The text to analyse: the selection (else the whole main text), the chapter whose heading is
/// body block `chapter`, or the whole main text; capped at [`MAX_INPUT_CHARS`]. Err (a German
/// message) when the chapter is missing.
pub fn input_text(app: &WordApp, scope: Scope, chapter: Option<usize>) -> Result<String, String> {
    let whole = || app.session.doc.final_text(wordcraft_engine::doc::StoryRef::Body);
    let text = match scope {
        Scope::Selection => Some(app.session.selected_text()).filter(|t| !t.trim().is_empty()).unwrap_or_else(whole),
        Scope::Chapter => {
            let block = chapter.ok_or("Kein Kapitel ausgewählt.")?;
            chapter_text(&app.session.doc, block).ok_or("Das Kapitel gibt es nicht mehr. Bitte neu auswählen.")?
        }
        Scope::Document => whole(),
    };
    Ok(text.chars().take(MAX_INPUT_CHARS).collect())
}

/// The top-level body block the cursor is in.
fn cursor_block(app: &WordApp) -> Option<usize> {
    let focus = &app.session.sel.focus;
    if focus.story != wordcraft_engine::doc::StoryRef::Body {
        return None;
    }
    focus.path.0.first().map(|b| *b as usize)
}

/// `picked` found again in `list` after edits: same heading block and title, else the same title,
/// else none.
pub fn find_again(picked: &Chapter, list: &[Chapter]) -> Option<Chapter> {
    list.iter().find(|c| c.block == picked.block && c.title == picked.title).or_else(|| list.iter().find(|c| c.title == picked.title)).cloned()
}

/// How a chapter is listed: indented by its level.
pub fn chapter_label(c: &Chapter) -> String {
    format!("{}{}", "   ".repeat(usize::from(c.level.saturating_sub(1))), c.title)
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
        if let Some(pending) = self.pending.as_mut()
            && let Some(answer) = pending.poll()
        {
            let (at, total) = pending.progress;
            self.pending = None;
            self.receive_parts(answer, at, total);
        }
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                for task in Task::ALL {
                    ui.selectable_value(&mut self.task, task, task.label());
                }
            });
            ui.horizontal_wrapped(|ui| {
                ui.radio_value(&mut self.scope, Scope::Selection, "Markierung");
                ui.radio_value(&mut self.scope, Scope::Chapter, "Kapitel");
                ui.radio_value(&mut self.scope, Scope::Document, "Ganzes Dokument");
            });
            if self.scope == Scope::Chapter {
                self.chapter_picker(app, ui);
            }
            ui.add_space(4.0);
            self.ask_button(app, ui);
            if let Some(msg) = &self.message {
                if self.message_is_error {
                    ui.colored_label(ui.visuals().error_fg_color, msg);
                } else {
                    ui.label(msg);
                }
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
                ui.checkbox(&mut row.chosen, RichText::new(headline(s)).strong());
                if !s.reason.is_empty() {
                    ui.label(&s.reason);
                }
            }
        });
        #[cfg(not(target_arch = "wasm32"))]
        {
            settings(ui, &mut self.config);
            self.save_settings();
        }
    }
}

impl AssistantPane {
    /// The chapter list under the scope choice; keeps [`AssistantPane::chapter`] on the same
    /// heading while the document changes.
    fn chapter_picker(&mut self, app: &WordApp, ui: &mut Ui) {
        let list = chapters(&app.session.doc);
        if list.is_empty() {
            self.chapter = None;
            ui.small("Keine Überschriften gefunden. Kapitel beginnen bei Absätzen mit „Überschrift 1“ bis „Überschrift 9“.");
            return;
        }
        self.chapter = self
            .chapter
            .as_ref()
            .and_then(|picked| find_again(picked, &list))
            .or_else(|| cursor_block(app).and_then(|b| chapter_at(&list, b)).cloned())
            .or_else(|| list.first().cloned());
        let shown = self.chapter.as_ref().map(|c| c.title.clone()).unwrap_or_default();
        craft_ui::ComboBox::from_id_salt("assistant_chapter").selected_text(shown).width(ui.available_width()).show_ui(ui, |ui| {
            for c in &list {
                let picked = self.chapter.as_ref().is_some_and(|p| p.block == c.block);
                if ui.selectable_label(picked, chapter_label(c)).clicked() {
                    self.chapter = Some(c.clone());
                }
            }
        });
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn ask_button(&mut self, app: &mut WordApp, ui: &mut Ui) {
        if self.busy() {
            ui.horizontal(|ui| {
                ui.spinner();
                match self.pending.as_ref().map(|p| p.progress) {
                    Some((at, total)) if total > 1 => ui.label(format!("{} läuft … Teil {} von {total}", self.task.label(), at + 1)),
                    _ => ui.label(format!("{} läuft …", self.task.label())),
                };
            });
            return;
        }
        if ui.button(format!("{} starten", self.task.label())).clicked() {
            let text = match input_text(app, self.scope, self.chapter.as_ref().map(|c| c.block)) {
                Ok(text) => text,
                Err(e) => {
                    self.problem(e);
                    return;
                }
            };
            let portions = wordcraft_assistant::portions(&text, self.config.portion_chars);
            if portions.is_empty() {
                self.problem("Der Text ist leer.");
                return;
            }
            self.message = None;
            self.rows.clear();
            self.rows_task = self.task;
            self.pending = Some(backend::Pending::start(self.config.clone(), self.task, portions, ui.ctx().clone()));
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
            ui.label("Max. Zeichen");
            ui.add(
                egui::DragValue::new(&mut config.portion_chars)
                    .range(wordcraft_assistant::MIN_PORTION_CHARS..=wordcraft_assistant::MAX_PORTION_CHARS)
                    .speed(500),
            )
            .on_hover_text("So viele Zeichen gehen höchstens in eine Anfrage; längere Texte werden an Absatzgrenzen geteilt.");
            ui.end_row();
        });
        ui.small("Leere Felder nutzen den Lektorat-Chatflow.");
    });
}

#[cfg(test)]
mod tests;
