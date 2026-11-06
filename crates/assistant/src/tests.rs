use serde_json::json;
use wordcraft_engine::Session;
use wordcraft_engine::doc::Document;

use crate::{AssistantError, DEFAULT_AUTHOR, Kind, MAX_SUGGESTIONS, Suggestion, Task, apply, parse_suggestions, question};

fn body_text(s: &mut Session) -> String {
    let v = s.run("document.text", &json!({})).unwrap();
    v["text"].as_str().unwrap_or_default().to_string()
}

fn replace(original: &str, replacement: &str) -> Suggestion {
    Suggestion { kind: Kind::Replace, original: original.into(), replacement: replacement.into(), reason: "kürzer".into() }
}

fn comment(original: &str, reason: &str) -> Suggestion {
    Suggestion { kind: Kind::Comment, original: original.into(), replacement: String::new(), reason: reason.into() }
}

#[test]
fn parses_a_bare_array() {
    let s = parse_suggestions(r#"[{"kind":"replace","original":"lief schnell","replacement":"rannte","reason":"kürzer"}]"#).unwrap();
    assert_eq!(s, vec![replace("lief schnell", "rannte")]);
}

#[test]
fn parses_json_inside_prose_and_code_fences() {
    let answer = "Hier meine Vorschläge:\n```json\n[{\"kind\":\"comment\",\"original\":\"Hund\",\"reason\":\"Rasse?\"}]\n```\nViel Erfolg!";
    assert_eq!(parse_suggestions(answer).unwrap(), vec![comment("Hund", "Rasse?")]);
}

#[test]
fn parses_an_object_with_a_suggestions_list() {
    let s = parse_suggestions(r#"{"suggestions":[{"original":"Hund","reason":"Name?"}]}"#).unwrap();
    assert_eq!(s, vec![comment("Hund", "Name?")], "kind defaults to comment");
}

#[test]
fn an_empty_list_is_fine() {
    assert_eq!(parse_suggestions("[]").unwrap(), vec![]);
}

#[test]
fn skips_malformed_and_useless_items() {
    let answer = r#"[
        {"kind":"replace","original":"a","replacement":"a","reason":"no change"},
        {"kind":"comment","original":"","reason":"no passage"},
        {"kind":"comment","original":"x","reason":"  "},
        {"kind":"rewrite","original":"x","reason":"unknown kind"},
        {"original": 42},
        {"kind":"comment","original":"ok","reason":"kept"}
    ]"#;
    assert_eq!(parse_suggestions(answer).unwrap(), vec![comment("ok", "kept")]);
}

#[test]
fn caps_the_number_of_suggestions() {
    let items: Vec<_> = (0..MAX_SUGGESTIONS + 50).map(|i| json!({"original": format!("w{i}"), "reason": "r"})).collect();
    assert_eq!(parse_suggestions(&json!(items).to_string()).unwrap().len(), MAX_SUGGESTIONS);
}

#[test]
fn an_answer_without_json_is_an_error() {
    assert!(matches!(parse_suggestions("Der Text ist gut so."), Err(AssistantError::Format(_))));
    assert!(matches!(parse_suggestions(r#"{"answer":"gut"}"#), Err(AssistantError::Format(_))));
    assert!(matches!(parse_suggestions("42"), Err(AssistantError::Format(_))));
}

#[test]
fn hostile_answers_never_panic() {
    for answer in ["", "[", "]", "][", "{[}]", "ü[ä]ö", "[{\"original\":\"\u{0}\"}]", &"[".repeat(10_000)] {
        let _ = parse_suggestions(answer);
    }
}

#[test]
fn the_question_carries_the_task_the_format_and_the_text() {
    for task in Task::ALL {
        let q = question(task, "Der Hund lief schnell.");
        assert!(q.contains("Der Hund lief schnell."), "{task:?}");
        assert!(q.contains("JSON-Array"), "{task:?}");
    }
    assert!(question(Task::Recherche, "x").contains("ausschließlich \"comment\""));
}

#[test]
fn a_replacement_becomes_a_tracked_change_by_the_assistant() {
    let mut s = Session::new(Document::from_text("Der Hund lief schnell nach Hause."));
    s.author = "Autor".into();
    let report = apply(&mut s, &[replace("lief schnell", "rannte")], DEFAULT_AUTHOR);
    assert_eq!(report.applied, 1, "{report:?}");
    assert_eq!(body_text(&mut s), "Der Hund rannte nach Hause.");
    assert!(!s.doc.settings.track_changes, "Track Changes is back to the document's setting");
    assert_eq!(s.author, "Autor", "the session's author is restored");
    // Rejecting everything brings the original back: it was a revision, not an overwrite.
    s.run("review.rejectAll", &json!({})).unwrap();
    assert_eq!(body_text(&mut s), "Der Hund lief schnell nach Hause.");
}

#[test]
fn a_comment_lands_on_the_passage_under_the_assistants_name() {
    let mut s = Session::new(Document::from_text("Der Hund lief schnell."));
    let report = apply(&mut s, &[comment("Hund", "Welche Rasse?")], DEFAULT_AUTHOR);
    assert_eq!(report.applied, 1, "{report:?}");
    assert_eq!(s.doc.comments.len(), 1);
    assert!(s.doc.comments.values().all(|c| c.author == DEFAULT_AUTHOR));
    assert_eq!(body_text(&mut s), "Der Hund lief schnell.", "a comment leaves the text alone");
}

#[test]
fn missing_passages_are_reported_not_fatal() {
    let mut s = Session::new(Document::from_text("Der Hund lief schnell."));
    let report = apply(&mut s, &[comment("Katze", "?"), comment("Hund", "!")], DEFAULT_AUTHOR);
    assert_eq!(report.applied, 1);
    assert_eq!(report.not_found, vec!["Katze".to_string()]);
}

#[test]
fn document_tracking_stays_on_when_it_was_on() {
    let mut s = Session::new(Document::from_text("Der Hund lief schnell."));
    s.run("review.trackChanges", &json!({"value": true})).unwrap();
    apply(&mut s, &[replace("schnell", "rasch")], DEFAULT_AUTHOR);
    assert!(s.doc.settings.track_changes);
}

#[cfg(not(target_arch = "wasm32"))]
mod flowise {
    use crate::Task;
    use crate::flowise::{FlowiseConfig, answer_text};

    fn config() -> FlowiseConfig {
        FlowiseConfig { lektorat_flow: "3f2a9c1e-0000-4abc-8def-123456789abc".into(), ..FlowiseConfig::default() }
    }

    #[test]
    fn builds_the_prediction_url() {
        let mut c = config();
        c.base_url = "http://localhost:3000/".into();
        assert_eq!(c.endpoint(Task::Lektorat).unwrap(), "http://localhost:3000/api/v1/prediction/3f2a9c1e-0000-4abc-8def-123456789abc");
    }

    #[test]
    fn refuses_bad_addresses_and_flow_ids() {
        let mut c = config();
        assert!(c.endpoint(Task::Recherche).is_err(), "no flow set");
        c.base_url = "file:///etc".into();
        assert!(c.endpoint(Task::Lektorat).is_err());
        c.base_url = "http://localhost:3000".into();
        c.lektorat_flow = "../../admin".into();
        assert!(c.endpoint(Task::Lektorat).is_err());
    }

    #[test]
    fn the_api_key_is_never_serialized() {
        let mut c = config();
        c.api_key = Some("geheim".into());
        let saved = serde_json::to_string(&c).unwrap();
        assert!(!saved.contains("geheim"));
    }

    #[test]
    fn reads_text_or_structured_answers() {
        assert_eq!(answer_text(r#"{"text":"[]","chatId":"x"}"#).unwrap(), "[]");
        assert_eq!(answer_text(r#"{"json":[{"original":"a"}]}"#).unwrap(), r#"[{"original":"a"}]"#);
        assert!(answer_text("<html>").is_err());
        assert!(answer_text(r#"{"error":"x"}"#).is_err());
    }
}
