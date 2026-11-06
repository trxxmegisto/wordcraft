//! Put suggestions into the document as tracked changes and comments.
//!
//! Everything goes through the engine's public commands (`select.text`, `text.insert`,
//! `review.newComment`, `review.trackChanges`), the same ones the ribbon, MCP and the control
//! channel use, so undo, revision marks and comment balloons behave as for any other edit.

use serde_json::json;
use wordcraft_engine::Session;

use crate::{Kind, Suggestion};

/// Author name the assistant's revisions and comments carry, so they stand out (and take their
/// own colour) in the Review tools.
pub const DEFAULT_AUTHOR: &str = "Lektorat (KI)";

/// What [`apply`] did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ApplyReport {
    /// Suggestions now in the document.
    pub applied: usize,
    /// Passages that weren't found in the document (the text changed, or the model didn't quote it exactly).
    pub not_found: Vec<String>,
    /// Suggestions an engine command refused, with its error.
    pub failed: Vec<(String, String)>,
}

/// Apply `suggestions` to the document under `author`.
///
/// Replacements are made with Track Changes on, whatever the document's setting, so each one is a
/// revision the writer accepts or rejects; the setting and the session's author are restored
/// afterwards. Each passage is located by its first occurrence in the document.
pub fn apply(session: &mut Session, suggestions: &[Suggestion], author: &str) -> ApplyReport {
    let mut report = ApplyReport::default();
    if suggestions.is_empty() {
        return report;
    }
    let saved_author = std::mem::replace(&mut session.author, author.to_string());
    let saved_tracking = session.doc.settings.track_changes;
    if let Err(e) = session.run("review.trackChanges", &json!({"value": true})) {
        session.author = saved_author;
        report.failed.push((String::new(), e.to_string()));
        return report;
    }
    for s in suggestions {
        if session.run("select.text", &json!({"text": s.original})).is_err() {
            report.not_found.push(s.original.clone());
            continue;
        }
        let result = match s.kind {
            Kind::Replace => session.run("text.insert", &json!({"text": s.replacement})),
            Kind::Comment => session.run("review.newComment", &json!({"text": s.reason})),
        };
        match result {
            Ok(_) => report.applied += 1,
            Err(e) => report.failed.push((s.original.clone(), e.to_string())),
        }
    }
    if let Err(e) = session.run("review.trackChanges", &json!({"value": saved_tracking})) {
        log::warn!("assistant: couldn't restore Track Changes: {e}");
    }
    session.author = saved_author;
    report
}
