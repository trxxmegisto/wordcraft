use serde_json::json;
use wordcraft_assistant::{AssistantError, Kind, Suggestion, Task};
use wordcraft_engine::Session;
use wordcraft_engine::doc::Document;
use wordcraft_ui_egui::WordApp;
use wordcraft_ui_egui::panes::PaneExtension;

use crate::{AssistantPane, KEY, MAX_INPUT_CHARS, Scope, chapter_label, find_again, headline, input_text, install, report_message};

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
    assert_eq!(input_text(&app, Scope::Selection, None).unwrap(), "Der Hund lief schnell nach Hause.", "nothing selected: the whole text");
    app.run("select.text", json!({"text": "lief schnell"})).unwrap();
    assert_eq!(input_text(&app, Scope::Selection, None).unwrap(), "lief schnell");
    assert_eq!(input_text(&app, Scope::Document, None).unwrap(), "Der Hund lief schnell nach Hause.");
}

#[test]
fn long_input_is_capped() {
    let app = app(&"ä".repeat(MAX_INPUT_CHARS + 10));
    assert_eq!(input_text(&app, Scope::Document, None).unwrap().chars().count(), MAX_INPUT_CHARS);
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
    assert!(app.session.doc.comments.values().all(|c| c.author == "Lektorat (KI)"));
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
    pane.pending = Some(crate::backend::Pending::start(pane.config.clone(), Task::Lektorat, vec!["Text".into()], egui::Context::default()));
    let ctx = egui::Context::default();
    let start = std::time::Instant::now();
    while pane.busy() && start.elapsed() < std::time::Duration::from_secs(30) {
        ctx.run_ui(egui::RawInput::default(), |ui| pane.body(&mut app, ui)).drop_without_applying_deltas();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(!pane.busy());
    assert!(pane.message_is_error);
    assert!(pane.message.as_deref().unwrap_or_default().contains("Flowise ist nicht erreichbar"), "{:?}", pane.message);
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

#[test]
fn suggestions_sign_with_the_task_they_came_from() {
    let mut app = app("Der Hund lief.");
    let mut pane = AssistantPane::new();
    pane.rows_task = Task::Recherche;
    pane.task = Task::Konsistenz; // switching the tab afterwards doesn't change who wrote them
    pane.receive(Ok(vec![comment("Hund", "Unklar: Rasse prüfen")]));
    pane.apply_chosen(&mut app);
    let authors: Vec<&str> = app.session.doc.comments.values().map(|c| c.author.as_str()).collect();
    assert_eq!(authors, ["Recherche (KI)"]);
}

#[test]
fn problems_are_marked_and_say_what_to_do() {
    let mut pane = AssistantPane::new();
    pane.receive(Err(AssistantError::NotConfigured("Für Recherche ist keine Chatflow-ID eingetragen.".into())));
    assert!(pane.message_is_error);
    assert!(pane.message.as_deref().unwrap_or_default().ends_with("Siehe „Verbindung“ unten."), "{:?}", pane.message);
    pane.receive(Ok(vec![comment("Hund", "?")]));
    assert!(!pane.message_is_error, "a good answer clears the error look");
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn connection_settings_are_kept_but_never_the_api_key() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sub").join(crate::SETTINGS_FILE);
    let mut pane = AssistantPane::new().with_settings_file(path.clone());
    pane.save_settings();
    assert!(!path.exists(), "nothing changed, nothing written");
    pane.config.base_url = "http://flowise.local:3000".into();
    pane.config.recherche_flow = "aaaa-1111".into();
    pane.config.api_key = Some("geheim".into());
    pane.save_settings();
    let saved = std::fs::read_to_string(&path).unwrap();
    assert!(saved.contains("aaaa-1111") && !saved.contains("geheim"), "{saved}");
    let again = crate::backend::load(&path);
    assert_eq!(again.recherche_flow, "aaaa-1111");
    assert_eq!(again.base_url, "http://flowise.local:3000");
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn broken_or_huge_settings_files_fall_back_to_the_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(crate::SETTINGS_FILE);
    for junk in ["", "{", "[1,2]", "{\"base_url\": 42}", &"x".repeat(100_000)] {
        std::fs::write(&path, junk).unwrap();
        assert_eq!(crate::backend::load(&path), Default::default(), "{}", junk.len());
    }
}

#[test]
fn research_hints_without_a_verdict_start_unticked() {
    let mut pane = AssistantPane::new();
    pane.rows_task = Task::Recherche;
    pane.receive(Ok(vec![comment("Hund", "Falsch: Katze."), comment("Wiese", "Das sollte überprüft werden.")]));
    let ticked: Vec<bool> = pane.rows.iter().map(|r| r.chosen).collect();
    assert_eq!(ticked, [true, false]);
    assert_eq!(pane.message.as_deref(), Some("2 Vorschläge; einer ohne Urteil ist nicht ausgewählt."));
    pane.rows_task = Task::Lektorat;
    pane.receive(Ok(vec![comment("Wiese", "Füllwort")]));
    assert!(pane.rows[0].chosen, "Lektorat notes need no verdict");
}

#[test]
fn an_accepted_research_correction_replaces_the_wrong_fact() {
    let mut app = app("Als am 9. November 1990 die Mauer fiel.");
    let mut pane = AssistantPane::new();
    pane.rows_task = Task::Recherche;
    pane.receive(Ok(vec![Suggestion {
        kind: Kind::Replace,
        original: "9. November 1990".into(),
        replacement: "9. November 1989".into(),
        reason: "Falsch: Die Mauer fiel 1989.".into(),
    }]));
    pane.apply_chosen(&mut app);
    assert_eq!(body_text(&mut app), "Als am 9. November 1989 die Mauer fiel.");
    let authors: Vec<&str> = app.session.doc.comments.values().map(|c| c.author.as_str()).collect();
    assert_eq!(authors, ["Recherche (KI)"]);
}

/// An app whose document is `(text, style)` paragraphs, the caret at the end.
fn styled(paras: &[(&str, &str)]) -> WordApp {
    let mut app = app("");
    for (i, (text, style)) in paras.iter().enumerate() {
        if i > 0 {
            app.run("text.newParagraph", json!({})).unwrap();
        }
        app.run("text.insert", json!({"text": text})).unwrap();
        app.run("para.style", json!({"style": style})).unwrap();
    }
    app
}

fn book() -> WordApp {
    styled(&[("Kapitel 1", "Heading1"), ("Marta kam.", "Normal"), ("Kapitel 2", "Heading1"), ("Martha ging.", "Normal")])
}

#[test]
fn analyses_the_chosen_chapter() {
    let app = book();
    assert_eq!(
        input_text(&app, Scope::Chapter, Some(2)).unwrap(),
        "Kapitel 2
Martha ging.
"
    );
    assert_eq!(input_text(&app, Scope::Chapter, None).unwrap_err(), "Kein Kapitel ausgewählt.");
    assert!(input_text(&app, Scope::Chapter, Some(1)).unwrap_err().contains("neu auswählen"), "block 1 is no heading");
}

#[test]
fn the_picked_chapter_is_found_again_after_edits() {
    let app = book();
    let list = wordcraft_assistant::chapters(&app.session.doc);
    let picked = list[1].clone();
    assert_eq!(find_again(&picked, &list), Some(picked.clone()));
    // A paragraph inserted above moves the heading: found by its title.
    let moved: Vec<_> = list.iter().map(|c| wordcraft_assistant::Chapter { block: c.block + 1, ..c.clone() }).collect();
    assert_eq!(find_again(&picked, &moved).map(|c| c.block), Some(3));
    // Renamed: gone, so the pane falls back to the chapter with the cursor.
    let renamed: Vec<_> = list.iter().map(|c| wordcraft_assistant::Chapter { title: format!("{} neu", c.title), ..c.clone() }).collect();
    assert_eq!(find_again(&picked, &renamed), None);
    assert_eq!(chapter_label(&wordcraft_assistant::Chapter { block: 0, level: 3, title: "Szene".into() }), "      Szene");
}

#[test]
fn the_chapter_with_the_cursor_is_picked_first() {
    let mut app = book(); // the caret is in "Martha ging.", chapter 2
    let mut pane = AssistantPane::new();
    pane.scope = Scope::Chapter;
    let ctx = egui::Context::default();
    ctx.run_ui(egui::RawInput::default(), |ui| pane.body(&mut app, ui)).drop_without_applying_deltas();
    assert_eq!(pane.chapter.as_ref().map(|c| c.title.as_str()), Some("Kapitel 2"));
}

#[test]
fn several_portions_say_so_in_the_message() {
    let mut pane = AssistantPane::new();
    pane.receive_parts(Ok(vec![comment("Marta", "Name?")]), 2, 3);
    assert_eq!(pane.message.as_deref(), Some("1 Vorschlag. (3 Teile geprüft.)"));
    pane.receive_parts(Err(AssistantError::Http(500)), 1, 3);
    assert!(pane.message_is_error);
    assert_eq!(pane.message.as_deref(), Some("Bei Teil 2 von 3: Flowise hat mit HTTP-Status 500 geantwortet."));
    pane.receive_parts(Ok(vec![]), 0, 1);
    assert_eq!(pane.message.as_deref(), Some("Keine Anmerkungen."), "one portion: no count");
}
