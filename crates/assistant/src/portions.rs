//! Long texts go to the assistant in portions that fit the model: split between paragraphs, and
//! inside a paragraph only when it alone is too long (between sentences, else between words).

use crate::{AssistantError, MAX_SUGGESTIONS, Reply, Suggestion, Task, WritingAssistant};

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

/// At most this many facts are carried from portion to portion; later ones are dropped (the
/// first statement of a fact is the one the rest of the text is held to).
pub const MAX_FACTS: usize = 300;
/// The carried facts together stay below this many characters.
pub const MAX_FACTS_CHARS: usize = 20_000;

/// Ask `assistant` about each of `portions` in turn and gather the suggestions, dropping repeats
/// (the same change or comment on the same passage). `progress(done, total)` is called before
/// each request. Stops at the first error: what came back before it is lost, since the writer
/// would otherwise take a partial check for a full one.
///
/// A consistency check of several portions carries facts along: each portion but the last is
/// asked for its facts, and each portion after the first is checked against the facts so far,
/// so a contradiction across a portion boundary is found. The reply's `facts` are those carried.
pub fn ask_portions(
    assistant: &dyn WritingAssistant,
    task: Task,
    portions: &[String],
    mut progress: impl FnMut(usize, usize),
) -> Result<Reply, AssistantError> {
    let carry = task == Task::Konsistenz && portions.len() > 1;
    let mut all: Vec<Suggestion> = Vec::new();
    let mut facts: Vec<String> = Vec::new();
    for (i, portion) in portions.iter().enumerate() {
        progress(i, portions.len());
        let suggestions = if carry {
            let collect = i + 1 < portions.len();
            let reply = assistant.ask_with_facts(task, portion, &facts, collect)?;
            if collect {
                add_facts(&mut facts, reply.facts);
            }
            reply.suggestions
        } else {
            assistant.ask(task, portion)?
        };
        for s in suggestions {
            let repeat = all.iter().any(|a| a.kind == s.kind && a.original == s.original && a.replacement == s.replacement);
            if !repeat && all.len() < MAX_SUGGESTIONS {
                all.push(s);
            }
        }
    }
    Ok(Reply { suggestions: all, facts })
}

/// Add `new` facts to `facts`, skipping ones already there (ignoring case, spacing and the final
/// full stop) and stopping at [`MAX_FACTS`] / [`MAX_FACTS_CHARS`].
fn add_facts(facts: &mut Vec<String>, new: Vec<String>) {
    let key = |f: &str| f.trim().trim_end_matches('.').split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase();
    let mut chars: usize = facts.iter().map(|f| f.chars().count()).sum();
    for fact in new {
        let n = fact.chars().count();
        if facts.len() >= MAX_FACTS || chars.saturating_add(n) > MAX_FACTS_CHARS {
            break;
        }
        let k = key(&fact);
        if k.is_empty() || facts.iter().any(|f| key(f) == k) {
            continue;
        }
        chars += n;
        facts.push(fact);
    }
}
