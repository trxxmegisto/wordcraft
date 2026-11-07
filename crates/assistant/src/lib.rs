//! Writing assistant: editing ("Lektorat"), research and consistency suggestions for the
//! document, produced by an external AI workflow and applied as tracked changes and comments.
//!
//! The assistant never rewrites the document on its own. A backend ([`WritingAssistant`], for
//! now [`flowise::FlowiseAssistant`]) answers with structured [`Suggestion`]s; [`apply`] turns
//! them into revisions and comments under their own author, so the writer accepts or rejects
//! each one with the normal Review tools.
//!
//! This crate is a self-contained extension (KWordCraft fork): it only uses the engine's public
//! command interface and document model, so upstream changes to WordCraft rarely touch it.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod apply;
mod chapters;
#[cfg(not(target_arch = "wasm32"))]
pub mod flowise;
mod portions;
mod prompt;
mod suggestion;

pub use apply::{ApplyReport, DEFAULT_AUTHOR, apply, apply_as};
pub use chapters::{Chapter, MAX_TITLE_CHARS, chapter_at, chapter_text, chapters};
pub use portions::{DEFAULT_PORTION_CHARS, MAX_PORTION_CHARS, MIN_PORTION_CHARS, ask_portions, portions};
pub use portions::{MAX_FACTS, MAX_FACTS_CHARS};
pub use prompt::{Task, VERDICTS, question, question_with_facts};
pub use suggestion::{
    Kind, MAX_FACT_CHARS, MAX_FACTS_PER_ANSWER, MAX_FIELD_CHARS, MAX_SUGGESTIONS, Reply, Suggestion, parse_reply, parse_suggestions,
};

/// Errors from the assistant. Their messages are German: they are shown to the writer as they are.
#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum AssistantError {
    /// The assistant isn't set up (no address or no workflow for the task).
    #[error("Der Assistent ist nicht eingerichtet: {0}")]
    NotConfigured(String),
    /// Nothing answered, or the connection broke.
    #[error("Flowise ist nicht erreichbar: {0}")]
    Unreachable(String),
    /// The service answered with an HTTP error status.
    #[error("Flowise hat mit HTTP-Status {0} geantwortet.")]
    Http(u16),
    /// The answer isn't in the agreed format.
    #[error("Die Antwort ist nicht verwendbar: {0}")]
    Format(String),
}

/// A backend that analyses text and answers with suggestions.
pub trait WritingAssistant {
    /// Analyse `text` for `task`. Blocking: call it off the UI thread.
    fn ask(&self, task: Task, text: &str) -> Result<Vec<Suggestion>, AssistantError>;

    /// Analyse one portion of a long text against the facts `known` from earlier portions and,
    /// with `collect`, also report this portion's facts. Blocking. A backend that can't carry
    /// facts answers as [`WritingAssistant::ask`] does, with no facts.
    fn ask_with_facts(&self, task: Task, text: &str, known: &[String], collect: bool) -> Result<Reply, AssistantError> {
        let _ = (known, collect);
        Ok(Reply { suggestions: self.ask(task, text)?, facts: Vec::new() })
    }
}

#[cfg(test)]
mod tests;
