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
    apply_as(session, suggestions, author, false)
}

/// [`apply`], and with `explain` each replacement also gets its reason as a comment on the new
/// text (research corrections: the writer sees why a fact was changed).
pub fn apply_as(session: &mut Session, suggestions: &[Suggestion], author: &str, explain: bool) -> ApplyReport {
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
        if !select_passage(session, s) {
            report.not_found.push(s.original.clone());
            continue;
        }
        let result = match s.kind {
            Kind::Replace => session.run("text.insert", &json!({"text": s.replacement})),
            Kind::Comment => session.run("review.newComment", &json!({"text": s.reason})),
        };
        match result {
            Ok(_) => {
                report.applied += 1;
                if explain && s.kind == Kind::Replace && !s.reason.trim().is_empty() && !s.replacement.is_empty() {
                    comment_inserted(session, &s.replacement, &s.reason);
                }
            }
            Err(e) => report.failed.push((s.original.clone(), e.to_string())),
        }
    }
    if let Err(e) = session.run("review.trackChanges", &json!({"value": saved_tracking})) {
        log::warn!("assistant: couldn't restore Track Changes: {e}");
    }
    session.author = saved_author;
    report
}

/// Select the suggestion's passage. A deletion (an empty replacement) takes one space next to
/// the passage with it, the following one if there is one, so no double space is left behind.
///
/// Models misquote in small, predictable ways, so a passage that isn't found is looked for again
/// with its first letter in the other case and without a full stop it ended with. A replacement
/// is only made on the passage exactly as quoted: a looser match could replace the wrong text.
fn select_passage(session: &mut Session, s: &Suggestion) -> bool {
    let select = |session: &mut Session, text: &str| session.run("select.text", &json!({"text": text})).is_ok();
    let deletion = s.kind == Kind::Replace && s.replacement.is_empty() && !s.original.starts_with(' ') && !s.original.ends_with(' ');
    if deletion && (select(session, &format!("{} ", s.original)) || select(session, &format!(" {}", s.original))) {
        return true;
    }
    if select(session, &s.original) {
        return true;
    }
    s.kind == Kind::Comment && near_quotes(&s.original).iter().any(|q| select(session, q))
}

/// Close variants of a misquoted passage: the first letter in the other case, and without a
/// trailing full stop (and both).
fn near_quotes(original: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = original.chars();
    let swapped = chars.next().map(|first| {
        let other: String = if first.is_uppercase() { first.to_lowercase().collect() } else { first.to_uppercase().collect() };
        other + chars.as_str()
    });
    for candidate in [Some(original.to_string()), swapped].into_iter().flatten() {
        if let Some(stripped) = candidate.strip_suffix('.').filter(|t| !t.trim().is_empty()) {
            out.push(stripped.to_string());
        }
        if candidate != original {
            out.push(candidate);
        }
    }
    out
}

/// Put `reason` as a comment on `inserted`, the text just typed before the caret. A comment that
/// can't be placed is only logged: the replacement itself is in.
fn comment_inserted(session: &mut Session, inserted: &str, reason: &str) {
    let placed = (|| -> Option<()> {
        let sel = session.run("document.selection", &json!({})).ok()?;
        let focus = sel.get("focus")?.clone();
        let off = focus.get("off")?.as_u64().and_then(|o| usize::try_from(o).ok())?.checked_sub(inserted.len())?;
        let mut anchor = focus.clone();
        *anchor.get_mut("off")? = json!(off);
        session.run("select.range", &json!({"anchor": anchor, "focus": focus})).ok()?;
        session.run("review.newComment", &json!({"text": reason})).ok()?;
        Some(())
    })();
    if placed.is_none() {
        log::warn!("assistant: couldn't put the reason on the replacement as a comment");
    }
}
