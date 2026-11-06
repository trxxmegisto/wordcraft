//! Long texts go to the assistant in portions that fit the model: split between paragraphs, and
//! inside a paragraph only when it alone is too long (between sentences, else between words).

use crate::{AssistantError, MAX_SUGGESTIONS, Suggestion, Task, WritingAssistant};

/// Characters per request unless the settings say otherwise.
pub const DEFAULT_PORTION_CHARS: usize = 15_000;
/// The smallest portion size accepted (smaller settings are raised to it).
pub const MIN_PORTION_CHARS: usize = 1_000;
/// The largest portion size accepted (larger settings are lowered to it).
pub const MAX_PORTION_CHARS: usize = 200_000;

/// `text` cut into portions of at most `max_chars` characters (clamped to
/// [`MIN_PORTION_CHARS`]..=[`MAX_PORTION_CHARS`]). Paragraphs stay whole where they fit; blank
/// portions are dropped. A text that fits is one portion, unchanged.
pub fn portions(text: &str, max_chars: usize) -> Vec<String> {
    let max = max_chars.clamp(MIN_PORTION_CHARS, MAX_PORTION_CHARS);
    let mut out = Vec::new();
    let mut current = String::new();
    let mut current_chars = 0;
    for para in text.split_inclusive('\n') {
        for piece in split_long(para, max) {
            let n = piece.chars().count();
            if current_chars + n > max && !current.is_empty() {
                out.push(std::mem::take(&mut current));
                current_chars = 0;
            }
            current.push_str(piece);
            current_chars += n;
        }
    }
    out.push(current);
    out.retain(|p| !p.trim().is_empty());
    out
}

/// `para` in pieces of at most `max` characters: whole if it fits, else cut after a sentence end
/// (`.`, `!`, `?`, `…` followed by a space), else after a space, else at `max` characters.
fn split_long(para: &str, max: usize) -> Vec<&str> {
    let mut pieces = Vec::new();
    let mut rest = para;
    while rest.chars().count() > max {
        // Byte offset just past the first `max` characters: always a char boundary.
        let limit = rest.char_indices().nth(max).map_or(rest.len(), |(i, _)| i);
        let window = rest.get(..limit).unwrap_or(rest);
        let sentence =
            window.char_indices().rev().find(|&(i, c)| c == ' ' && i > 0 && window.get(..i).is_some_and(ends_sentence)).map(|(i, _)| i + 1);
        let space = window.rfind(' ').map(|i| i + 1).filter(|&i| i > 0);
        let cut = sentence.or(space).filter(|&i| i < rest.len()).unwrap_or(limit);
        let (head, tail) = rest.split_at_checked(cut).unwrap_or((rest, ""));
        if head.is_empty() {
            break;
        }
        pieces.push(head);
        rest = tail;
    }
    if !rest.is_empty() {
        pieces.push(rest);
    }
    pieces
}

fn ends_sentence(s: &str) -> bool {
    s.trim_end_matches(['"', '\'', '“', '”', '„', '«', '»', ')']).ends_with(['.', '!', '?', '…'])
}

/// Ask `assistant` about each of `portions` in turn and gather the suggestions, dropping repeats
/// (the same change or comment on the same passage). `progress(done, total)` is called before
/// each request. Stops at the first error: what came back before it is lost, since the writer
/// would otherwise take a partial check for a full one.
pub fn ask_portions(
    assistant: &dyn WritingAssistant,
    task: Task,
    portions: &[String],
    mut progress: impl FnMut(usize, usize),
) -> Result<Vec<Suggestion>, AssistantError> {
    let mut all: Vec<Suggestion> = Vec::new();
    for (i, portion) in portions.iter().enumerate() {
        progress(i, portions.len());
        for s in assistant.ask(task, portion)? {
            let repeat = all.iter().any(|a| a.kind == s.kind && a.original == s.original && a.replacement == s.replacement);
            if !repeat && all.len() < MAX_SUGGESTIONS {
                all.push(s);
            }
        }
    }
    Ok(all)
}
