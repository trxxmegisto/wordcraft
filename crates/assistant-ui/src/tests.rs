use serde_json::json;
use wordcraft_assistant::{AssistantError, DEFAULT_AUTHOR, Kind, Suggestion, Task};
use wordcraft_engine::Session;
use wordcraft_engine::doc::Document;
use wordcraft_ui_egui::WordApp;
use wordcraft_ui_egui::panes::PaneExtension;

use crate::{AssistantPane, KEY, MAX_INPUT_CHARS, Scope, headline, input_text, install, report_message};

fn app(text: &str) -> WordApp {
    WordApp::new(Session::new(Document::from_text(text)), Default::default())
}

fn replace(original: &str, replacement: &str) -> Suggestion {
    Suggestion { kind: Kind::Replace, original: original.into(), replacement: replacement.into(), reason: "kürzer".into() }
}

fn comment(original: &str, reason: &str) -> Suggestion {
    Suggestion { kind: Kind::Comment, original: original.into(), replacement: String::new(), reason: reason.into() }
}

fn body_text(app: &mut WordApp) -> String {
    app.run("document.text", json!({})).unwrap()["text"].as_str().unwrap_or_default().to_string()
}

#[test]
fn installs_as_a_task_pane_that_opens_by_command() {
    let mut app = app("Der Hund lief.");
    install(&mut app);
    let shown = app.run("ui.extensionPane", json!({"pane": KEY, "value": true})).unwrap();
    assert_eq!(shown, json!({"pane": KEY, "open": true}));
    assert_eq!(app.run("ui.paneDock", json!({"pane": KEY})).unwrap(), json!({"pane": KEY, "dock": "right"}));
}

#[test]
fn analyses_the_selection_or_else_the_whole_text() {
    let mut app = app("Der Hund lief schnell nach Hause.");
    assert_eq!(input_text(&app, Scope::Selection), "Der Hund lief schnell nach Hause.", "nothing selected: the whole text");
    app.run("select.text", json!({"text": "lief schnell"})).unwrap();
    assert_eq!(input_text(&app, Scope::Selection), "lief schnell");
    assert_eq!(input_text(&app, Scope::Document), "Der Hund lief schnell nach Hause.");
}

#[test]
fn long_input_is_capped() {
    let app = app(&"ä".repeat(MAX_INPUT_CHARS + 10));
    assert_eq!(input_text(&app, Scope::Document).chars().count(), MAX_INPUT_CHARS);
}

#[test]
fn answers_fill_the_list_and_errors_show_as_a_message() {
    let mut pane = AssistantPane::new();
    pane.receive(Ok(vec![replace("lief schnell", "rannte"), comment("Hund", "Rasse?")]));
    assert_eq!(pane.rows.len(), 2);
    assert!(pane.rows.iter().all(|r| r.chosen), "everything starts ticked");
    pane.receive(Err(AssistantError::Http(500)));
    assert_eq!(pane.rows.len(), 2, "a failed request keeps the list");
    assert!(pane.message.as_deref().unwrap_or_default().contains("500"));
    pane.receive(Ok(vec![]));
    assert!(pane.rows.is_empty());
}

#[test]
fn only_ticked_suggestions_go_into_the_document_as_revisions() {
    let mut app = app("Der Hund lief schnell nach Hause.");
    let mut pane = AssistantPane::new();
    pane.receive(Ok(vec![replace("lief schnell", "rannte"), comment("Hund", "Rasse?"), comment("Katze", "fehlt")]));
    pane.rows[1].chosen = false;
    pane.apply_chosen(&mut app);
    assert_eq!(body_text(&mut app), "Der Hund rannte nach Hause.");
    assert!(app.session.doc.comments.is_empty(), "the unticked comment stayed out");
    let left: Vec<&str> = pane.rows.iter().map(|r| r.suggestion.original.as_str()).collect();
    assert_eq!(left, ["Hund", "Katze"], "applied ones leave the list; the unticked and the missing one stay");
    assert!(pane.rows.iter().all(|r| !r.chosen), "what stays is unticked");
    assert_eq!(pane.message.as_deref(), Some("1 übernommen. 1 Textstelle nicht gefunden."));
    pane.rows[0].chosen = true;
    pane.apply_chosen(&mut app);
    assert_eq!(pane.rows.len(), 1);
    assert!(app.session.doc.comments.values().all(|c| c.author == DEFAULT_AUTHOR));
    app.run("review.rejectAll", json!({})).unwrap();
    assert_eq!(body_text(&mut app), "Der Hund lief schnell nach Hause.");
}

#[test]
fn nothing_ticked_changes_nothing() {
    let mut app = app("Der Hund lief.");
    let mut pane = AssistantPane::new();
    pane.apply_chosen(&mut app);
    assert_eq!(body_text(&mut app), "Der Hund lief.");
    assert!(pane.message.is_some());
}

#[test]
fn draws_every_state_without_panicking() {
    let mut app = app("Der Hund lief.");
    let mut pane = AssistantPane::new();
    pane.set_open(true);
    let ctx = egui::Context::default();
    for task in Task::ALL {
        pane.task = task;
        ctx.run_ui(egui::RawInput::default(), |ui| pane.body(&mut app, ui)).drop_without_applying_deltas();
    }
    pane.receive(Ok(vec![replace("lief", "rannte"), comment("Hund", "")]));
    ctx.run_ui(egui::RawInput::default(), |ui| pane.body(&mut app, ui)).drop_without_applying_deltas();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn an_unreachable_flowise_comes_back_as_a_message() {
    let mut app = app("Der Hund lief.");
    let mut pane = AssistantPane::new();
    // A closed local port and a valid flow id: the request fails fast, on its own thread.
    pane.config.base_url = "http://127.0.0.1:9".into();
    pane.config.lektorat_flow = "3f2a9c1e-0000-4abc-8def-123456789abc".into();
    pane.config.timeout_secs = 5;
    pane.pending = Some(crate::backend::Pending::start(pane.config.clone(), Task::Lektorat, "Text".into(), egui::Context::default()));
    let ctx = egui::Context::default();
    let start = std::time::Instant::now();
    while pane.busy() && start.elapsed() < std::time::Duration::from_secs(30) {
        ctx.run_ui(egui::RawInput::default(), |ui| pane.body(&mut app, ui)).drop_without_applying_deltas();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(!pane.busy());
    assert!(pane.message.as_deref().unwrap_or_default().contains("can't be reached"), "{:?}", pane.message);
}

#[test]
fn deletions_read_as_such_and_messages_count_in_german() {
    assert_eq!(headline(&replace("eigentlich", "")), "„eigentlich“ streichen");
    assert_eq!(headline(&replace("lief schnell", "rannte")), "„lief schnell“ → „rannte“");
    assert_eq!(headline(&comment("Hund", "?")), "„Hund“");
    assert_eq!(report_message(2, 0, 0), "2 übernommen.");
    assert_eq!(report_message(0, 1, 0), "0 übernommen. 1 Textstelle nicht gefunden.");
    assert_eq!(report_message(1, 3, 2), "1 übernommen. 3 Textstellen nicht gefunden. 2 fehlgeschlagen.");
}
