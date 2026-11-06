//! The structured answer the assistant agrees to give: a list of suggestions, each tied to an
//! exact passage of the analysed text.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::AssistantError;

/// At most this many suggestions are taken from one answer; the rest are dropped.
pub const MAX_SUGGESTIONS: usize = 200;
/// Longest passage, replacement or reason accepted, in characters.
pub const MAX_FIELD_CHARS: usize = 4000;

/// What a suggestion does to the document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// A margin comment on the passage; the text stays as it is.
    #[default]
    Comment,
    /// Replace the passage (as a tracked change).
    Replace,
}

/// One suggestion from the assistant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Suggestion {
    /// Comment or replacement.
    #[serde(default)]
    pub kind: Kind,
    /// The exact passage, copied character for character from the analysed text.
    pub original: String,
    /// The new text for [`Kind::Replace`]; empty for comments.
    #[serde(default)]
    pub replacement: String,
    /// Why: the comment's text, and the explanation shown for a replacement.
    #[serde(default)]
    pub reason: String,
}

impl Suggestion {
    /// Whether the suggestion can be applied: a non-empty passage, fields within limits, and a
    /// replacement that actually changes something.
    fn usable(&self) -> bool {
        let fits = |s: &str| s.chars().count() <= MAX_FIELD_CHARS;
        if self.original.trim().is_empty() || !fits(&self.original) || !fits(&self.replacement) || !fits(&self.reason) {
            return false;
        }
        match self.kind {
            Kind::Replace => self.replacement != self.original,
            Kind::Comment => !self.reason.trim().is_empty(),
        }
    }
}

/// Read the suggestions out of the assistant's answer.
///
/// Language models wrap JSON in prose or code fences, so this accepts a bare array, an object
/// with a `suggestions` array, or either embedded in other text. Items that don't fit the
/// format are skipped (and logged), not fatal. An answer with no JSON at all is an error.
pub fn parse_suggestions(answer: &str) -> Result<Vec<Suggestion>, AssistantError> {
    let value = find_json(answer).ok_or_else(|| AssistantError::Format("no JSON list of suggestions in the answer".into()))?;
    let items = match value {
        Value::Array(items) => items,
        Value::Object(mut o) => match o.remove("suggestions") {
            Some(Value::Array(items)) => items,
            _ => return Err(AssistantError::Format("expected a list of suggestions".into())),
        },
        _ => return Err(AssistantError::Format("expected a list of suggestions".into())),
    };
    let mut out = Vec::new();
    for item in items.into_iter().take(MAX_SUGGESTIONS) {
        match serde_json::from_value::<Suggestion>(item) {
            Ok(s) if s.usable() => out.push(s),
            Ok(_) => log::warn!("assistant: skipped an unusable suggestion"),
            Err(e) => log::warn!("assistant: skipped a malformed suggestion: {e}"),
        }
    }
    Ok(out)
}

/// The JSON in `answer`: the whole text, else the outermost `[...]`, else the outermost `{...}`.
fn find_json(answer: &str) -> Option<Value> {
    let trimmed = answer.trim();
    if let Ok(v) = serde_json::from_str::<Value>(trimmed) {
        return Some(v);
    }
    for (open, close) in [('[', ']'), ('{', '}')] {
        // Both brackets are ASCII, so these byte offsets are char boundaries.
        if let (Some(a), Some(b)) = (trimmed.find(open), trimmed.rfind(close))
            && a < b
            && let Some(slice) = trimmed.get(a..=b)
            && let Ok(v) = serde_json::from_str::<Value>(slice)
        {
            return Some(v);
        }
    }
    None
}
