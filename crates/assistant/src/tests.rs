use serde_json::json;
use wordcraft_engine::Session;
use wordcraft_engine::doc::Document;

use crate::{AssistantError, DEFAULT_AUTHOR, Kind, MAX_SUGGESTIONS, Suggestion, Task, apply, apply_as, parse_suggestions, question};

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
    assert!(question(Task::Konsistenz, "x").contains("ausschließlich \"comment\""));
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
        assert!(FlowiseConfig::default().endpoint(Task::Recherche).is_err(), "no flow set at all");
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
    #[test]
    fn tasks_without_their_own_chatflow_use_the_lektorat_one() {
        let mut c = config();
        let lektorat = c.endpoint(Task::Lektorat).unwrap();
        assert_eq!(c.endpoint(Task::Recherche).unwrap(), lektorat);
        assert_eq!(c.endpoint(Task::Konsistenz).unwrap(), lektorat);
        c.recherche_flow = "aaaa-1111".into();
        assert!(c.endpoint(Task::Recherche).unwrap().ends_with("/aaaa-1111"));
        c.konsistenz_flow = "   ".into();
        assert_eq!(c.endpoint(Task::Konsistenz).unwrap(), lektorat, "blank counts as empty");
    }

    #[test]
    fn messages_are_german() {
        let e = FlowiseConfig::default().endpoint(Task::Recherche).unwrap_err().to_string();
        assert_eq!(e, "Der Assistent ist nicht eingerichtet: Für Recherche ist keine Chatflow-ID eingetragen.");
    }
}

#[test]
fn a_deletion_takes_one_neighbouring_space_with_it() {
    for (text, gone, expected) in [
        ("Er war eigentlich müde.", "eigentlich", "Er war müde."),
        ("Er war müde, eigentlich.", "eigentlich", "Er war müde,."), // no space after: takes the one before
        ("Eigentlich", "Eigentlich", ""),
    ] {
        let mut s = Session::new(Document::from_text(text));
        let report = apply(&mut s, &[replace(gone, "")], DEFAULT_AUTHOR);
        assert_eq!(report.applied, 1, "{text}: {report:?}");
        s.run("review.acceptAll", &json!({})).unwrap();
        assert_eq!(body_text(&mut s), expected, "{text}");
    }
}

#[test]
fn each_task_signs_with_its_own_name() {
    let names: Vec<&str> = Task::ALL.iter().map(|t| t.author()).collect();
    assert_eq!(names, ["Lektorat (KI)", "Recherche (KI)", "Konsistenz (KI)"]);
    assert_eq!(Task::Lektorat.author(), DEFAULT_AUTHOR);
}

#[test]
fn research_asks_for_a_verdict_not_a_reminder_to_check() {
    let q = question(Task::Recherche, "Die Mauer fiel 1990.");
    for must in ["\"Falsch:\"", "\"Zweifelhaft:\"", "\"Unklar:\"", "nie nur \"sollte überprüft werden\"", "allgemein bekannt", "Figurenrede"] {
        assert!(q.contains(must), "missing {must}");
    }
    assert!(!question(Task::Lektorat, "x").contains("Faktenprüfer"), "only the research task gets these rules");
}

#[test]
fn every_task_is_told_to_quote_exactly_even_when_the_text_is_wrong() {
    for task in Task::ALL {
        assert!(question(task, "x").contains("Zeichen für Zeichen, ohne Zahlen, Wörter oder Satzzeichen zu ändern"), "{task:?}");
    }
}

#[test]
fn a_slightly_misquoted_passage_still_gets_its_comment() {
    let text = "Ihr Großvater sagte, dass die erste Glashütte 1597 gegründet worden war. Danach schwieg er.";
    for quote in ["Die erste Glashütte 1597 gegründet worden war", "die erste Glashütte 1597 gegründet worden war.", "Die zweite Glashütte."] {
        let mut s = Session::new(Document::from_text(text));
        let report = apply(&mut s, &[comment(quote, "Zweifelhaft: …")], DEFAULT_AUTHOR);
        let expect_found = !quote.contains("zweite");
        assert_eq!(report.applied == 1, expect_found, "{quote}: {report:?}");
    }
}

#[test]
fn a_misquoted_replacement_is_left_alone() {
    let mut s = Session::new(Document::from_text("Er sagte, dass es regnet."));
    let report = apply(&mut s, &[replace("Dass es regnet", "es schneit")], DEFAULT_AUTHOR);
    assert_eq!(report.not_found, vec!["Dass es regnet".to_string()], "only comments get the looser match");
    assert_eq!(body_text(&mut s), "Er sagte, dass es regnet.");
}

#[test]
fn a_research_correction_replaces_the_fact_and_says_why() {
    let mut s = Session::new(Document::from_text("Als am 9. November 1990 die Mauer fiel, saß sie da."));
    let fix = Suggestion {
        kind: Kind::Replace,
        original: "9. November 1990".into(),
        replacement: "9. November 1989".into(),
        reason: "Falsch: Die Berliner Mauer fiel am 9. November 1989.".into(),
    };
    let report = apply_as(&mut s, &[fix], Task::Recherche.author(), Task::Recherche.explains_replacements());
    assert_eq!(report.applied, 1, "{report:?}");
    assert_eq!(body_text(&mut s), "Als am 9. November 1989 die Mauer fiel, saß sie da.");
    let authors: Vec<&str> = s.doc.comments.values().map(|c| c.author.as_str()).collect();
    assert_eq!(authors, ["Recherche (KI)"], "the reason is a comment on the new text");
    // Rejecting the change brings the original fact back.
    s.run("review.rejectAll", &json!({})).unwrap();
    assert_eq!(body_text(&mut s), "Als am 9. November 1990 die Mauer fiel, saß sie da.");
}

#[test]
fn lektorat_replacements_stay_without_a_comment() {
    let mut s = Session::new(Document::from_text("Der Hund lief schnell."));
    apply_as(&mut s, &[replace("lief schnell", "rannte")], Task::Lektorat.author(), Task::Lektorat.explains_replacements());
    assert!(s.doc.comments.is_empty());
}

#[test]
fn research_notes_need_a_verdict() {
    assert!(Task::Recherche.has_verdict("Falsch: 1989."));
    assert!(Task::Recherche.has_verdict("  Unklar: Ortschronik."));
    assert!(!Task::Recherche.has_verdict("Das sollte überprüft werden."));
    assert!(Task::Lektorat.has_verdict("kürzer"), "other tasks take any reason");
}

/// A document of `(text, style)` paragraphs, in order.
fn styled(paras: &[(&str, &str)]) -> Session {
    let mut s = Session::new(Document::new());
    for (i, (text, style)) in paras.iter().enumerate() {
        if i > 0 {
            s.run("text.newParagraph", &json!({})).unwrap();
        }
        s.run("text.insert", &json!({"text": text})).unwrap();
        s.run("para.style", &json!({"style": style})).unwrap();
    }
    s
}

fn book() -> Session {
    styled(&[
        ("Vorwort", "Normal"),
        ("Kapitel 1", "Heading1"),
        ("Marta kam im Winter.", "Normal"),
        ("Die Hütte", "Heading2"),
        ("Der Ofen glühte.", "Normal"),
        ("Kapitel 2", "Heading1"),
        ("Martha ging im Frühjahr.", "Normal"),
    ])
}

#[test]
fn chapters_are_the_headings_with_their_levels() {
    let s = book();
    let list = crate::chapters(&s.doc);
    let shown: Vec<(usize, u8, &str)> = list.iter().map(|c| (c.block, c.level, c.title.as_str())).collect();
    assert_eq!(shown, vec![(1, 1, "Kapitel 1"), (3, 2, "Die Hütte"), (5, 1, "Kapitel 2")]);
    assert_eq!(crate::chapter_at(&list, 4).map(|c| c.block), Some(3), "a paragraph belongs to the last heading before it");
    assert_eq!(crate::chapter_at(&list, 0), None, "text before the first heading belongs to no chapter");
}

#[test]
fn a_chapter_runs_to_the_next_heading_of_its_level_or_higher() {
    let s = book();
    assert_eq!(
        crate::chapter_text(&s.doc, 1).unwrap(),
        "Kapitel 1\nMarta kam im Winter.\nDie Hütte\nDer Ofen glühte.\n",
        "Heading 1 takes its sections"
    );
    assert_eq!(crate::chapter_text(&s.doc, 3).unwrap(), "Die Hütte\nDer Ofen glühte.\n", "Heading 2 stops at the next Heading 1");
    assert_eq!(crate::chapter_text(&s.doc, 5).unwrap(), "Kapitel 2\nMartha ging im Frühjahr.\n", "the last chapter runs to the end");
    assert_eq!(crate::chapter_text(&s.doc, 2), None, "not a heading");
    assert_eq!(crate::chapter_text(&s.doc, 99), None, "out of range");
}

#[test]
fn empty_headings_and_long_titles() {
    let long = "Ü".repeat(crate::MAX_TITLE_CHARS + 5);
    let s = styled(&[("", "Heading1"), (&long, "Heading1"), ("Text", "Normal")]);
    let list = crate::chapters(&s.doc);
    assert_eq!(list.len(), 1, "an empty heading starts no chapter");
    assert_eq!(list[0].title.chars().count(), crate::MAX_TITLE_CHARS + 1);
    assert!(list[0].title.ends_with('…'));
    assert!(crate::chapters(&Document::new()).is_empty());
}

#[test]
fn a_short_text_is_one_portion_unchanged() {
    assert_eq!(crate::portions("Ein Satz.\nNoch einer.", 15_000), vec!["Ein Satz.\nNoch einer.".to_string()]);
    assert!(crate::portions("  \n\n ", 15_000).is_empty(), "blank text: nothing to send");
    assert!(crate::portions("", 15_000).is_empty());
}

#[test]
fn portions_break_between_paragraphs_and_keep_all_text() {
    let para = format!("{}\n", "Wort ".repeat(120)); // 601 characters
    let text = para.repeat(5);
    let parts = crate::portions(&text, 1_000); // the smallest size: one paragraph each
    assert_eq!(parts.len(), 5);
    assert!(parts.iter().all(|p| *p == para), "each portion is whole paragraphs");
    assert_eq!(parts.concat(), text);
}

#[test]
fn a_long_paragraph_breaks_after_a_sentence() {
    let sentence = "Der Ofen glühte die ganze Nacht über. "; // 38 characters
    let text = sentence.repeat(60); // 2280 characters, one paragraph
    let parts = crate::portions(&text, 1_000);
    assert!(parts.len() >= 3);
    for p in &parts {
        assert!(p.chars().count() <= 1_000);
        assert!(p.starts_with("Der Ofen"), "cut between sentences: {p:?}");
    }
    assert_eq!(parts.concat(), text);
}

#[test]
fn text_without_breaks_is_cut_at_char_boundaries() {
    let text = "ä".repeat(2_500);
    let parts = crate::portions(&text, 0); // raised to the smallest size
    assert_eq!(parts.iter().map(|p| p.chars().count()).collect::<Vec<_>>(), vec![1_000, 1_000, 500]);
    assert_eq!(parts.concat(), text);
    let spaced = "ö".repeat(999) + " " + &"ü".repeat(1_500);
    assert_eq!(crate::portions(&spaced, 1_000).concat(), spaced, "cut after the space, nothing lost");
}

/// Answers each portion with what `answers` gives for it, and records the portions it got.
struct Scripted {
    answers: Vec<Result<Vec<Suggestion>, AssistantError>>,
    asked: std::cell::RefCell<Vec<String>>,
}

impl crate::WritingAssistant for Scripted {
    fn ask(&self, _task: Task, text: &str) -> Result<Vec<Suggestion>, AssistantError> {
        let n = self.asked.borrow().len();
        self.asked.borrow_mut().push(text.to_string());
        self.answers.get(n).cloned().unwrap_or(Ok(vec![]))
    }
}

#[test]
fn portions_are_asked_in_turn_and_repeats_dropped() {
    let a = Scripted {
        answers: vec![Ok(vec![comment("Marta", "Name?")]), Ok(vec![comment("Marta", "Name?"), replace("Martha", "Marta")])],
        asked: Default::default(),
    };
    let mut seen = Vec::new();
    let got = crate::ask_portions(&a, Task::Konsistenz, &["eins".into(), "zwei".into()], |done, total| seen.push((done, total))).unwrap();
    assert_eq!(got, vec![comment("Marta", "Name?"), replace("Martha", "Marta")]);
    assert_eq!(*a.asked.borrow(), vec!["eins".to_string(), "zwei".to_string()]);
    assert_eq!(seen, vec![(0, 2), (1, 2)]);
}

#[test]
fn a_failing_portion_stops_the_run() {
    let a = Scripted { answers: vec![Ok(vec![comment("Marta", "Name?")]), Err(AssistantError::Http(500))], asked: Default::default() };
    let got = crate::ask_portions(&a, Task::Lektorat, &["eins".into(), "zwei".into(), "drei".into()], |_, _| {});
    assert_eq!(got, Err(AssistantError::Http(500)));
    assert_eq!(a.asked.borrow().len(), 2, "the third portion isn't sent");
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn settings_saved_before_portions_get_the_default_size() {
    let c: crate::flowise::FlowiseConfig = serde_json::from_str(r#"{"base_url":"http://localhost:3000","lektorat_flow":"abc"}"#).unwrap();
    assert_eq!(c.portion_chars, crate::DEFAULT_PORTION_CHARS);
}
