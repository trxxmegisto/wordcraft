//! Ask the assistant about a text file from the command line, with the settings the app saved,
//! and print the suggestions: for tuning the prompts and checking a Flowise chatflow without the
//! app.
//!
//! ```text
//! cargo run -p wordcraft-assistant --example ask -- recherche crates/assistant/testdata/recherche-beispiel.txt
//! ```
//!
//! The task is `lektorat`, `recherche` or `konsistenz`. The settings come from `assistant.json`
//! in WordCraft's settings folder (or the path in `WORDCRAFT_ASSISTANT_SETTINGS`); the API key
//! from `WORDCRAFT_FLOWISE_API_KEY`.

use std::error::Error;

use wordcraft_assistant::flowise::{FlowiseAssistant, FlowiseConfig};
use wordcraft_assistant::{Task, WritingAssistant};

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let (Some(task), Some(file)) = (args.next(), args.next()) else {
        return Err("usage: ask <lektorat|recherche|konsistenz> <text file>".into());
    };
    let task = Task::ALL.into_iter().find(|t| t.id() == task).ok_or("the task is lektorat, recherche or konsistenz")?;
    let text = std::fs::read_to_string(&file)?;
    let settings = match std::env::var_os("WORDCRAFT_ASSISTANT_SETTINGS") {
        Some(path) => std::path::PathBuf::from(path),
        None => std::path::PathBuf::from(std::env::var_os("APPDATA").or_else(|| std::env::var_os("HOME")).ok_or("no settings folder")?)
            .join("WordCraft")
            .join("assistant.json"),
    };
    let mut config: FlowiseConfig = serde_json::from_str(&std::fs::read_to_string(&settings)?)?;
    config.api_key = std::env::var("WORDCRAFT_FLOWISE_API_KEY").ok();
    eprintln!("{} via {} …", task.label(), config.endpoint(task)?);
    let started = std::time::Instant::now();
    let suggestions = FlowiseAssistant::new(config).ask(task, &text)?;
    eprintln!("{} suggestions in {:.0?}\n", suggestions.len(), started.elapsed());
    for s in suggestions {
        let found = if text.contains(&s.original) { "" } else { "   [NOT IN THE TEXT]" };
        let verdict = if task.has_verdict(&s.reason) { "" } else { "   [NO VERDICT: starts unticked]" };
        match s.kind {
            wordcraft_assistant::Kind::Replace => println!("„{}“ → „{}“{found}{verdict}\n    {}\n", s.original, s.replacement, s.reason),
            wordcraft_assistant::Kind::Comment => println!("„{}“{found}{verdict}\n    {}\n", s.original, s.reason),
        }
    }
    Ok(())
}
