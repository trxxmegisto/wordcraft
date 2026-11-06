//! The question sent to the backend: the task's instructions, the answer format, and the text.
//!
//! The workflow on the other side (a Flowise chatflow) may add its own system prompt; the format
//! contract here is what keeps every answer machine-readable.

/// What the assistant is asked to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Task {
    /// Editing: style, flow, repetition, logic. Spelling is left to the built-in proofing.
    Lektorat,
    /// Research: facts to check, sources to look for. Comments only.
    Recherche,
    /// Consistency: names, timeline and details against the rest of the book. Comments only.
    Konsistenz,
}

impl Task {
    /// Every task, in menu order.
    pub const ALL: [Task; 3] = [Task::Lektorat, Task::Recherche, Task::Konsistenz];

    /// Stable id (settings keys, command ids).
    pub fn id(self) -> &'static str {
        match self {
            Task::Lektorat => "lektorat",
            Task::Recherche => "recherche",
            Task::Konsistenz => "konsistenz",
        }
    }

    /// Name shown to the user.
    pub fn label(self) -> &'static str {
        match self {
            Task::Lektorat => "Lektorat",
            Task::Recherche => "Recherche",
            Task::Konsistenz => "Konsistenz",
        }
    }

    /// Author name the task's revisions and comments carry, so the Review tools tell the tasks
    /// apart (and give each its own colour).
    pub fn author(self) -> &'static str {
        match self {
            Task::Lektorat => "Lektorat (KI)",
            Task::Recherche => "Recherche (KI)",
            Task::Konsistenz => "Konsistenz (KI)",
        }
    }

    fn instructions(self) -> &'static str {
        match self {
            Task::Lektorat => {
                "Du bist ein erfahrener Lektor. Prüfe den Text auf Stil, Lesefluss, Wortwiederholungen, \
                 Füllwörter, unklare Formulierungen und Logikbrüche. Korrigiere keine reinen Rechtschreib- \
                 oder Kommafehler, das übernimmt die eingebaute Prüfung. Bewahre die Stimme des Autors. \
                 Verwende \"replace\" für konkrete Umformulierungen und \"comment\" für Hinweise ohne \
                 eindeutige Lösung."
            }
            Task::Recherche => {
                "Du unterstützt einen Autor bei der Recherche. Markiere Aussagen, Fakten, Daten, Orte und \
                 Fachbegriffe, die geprüft werden sollten, und gib im Kommentar an, was zu prüfen ist und \
                 wo man es nachschlagen kann. Nenne Quellen nur, wenn du dir sicher bist. Verwende \
                 ausschließlich \"comment\"."
            }
            Task::Konsistenz => {
                "Du prüfst einen Romantext auf innere Widersprüche: Namen, Schreibweisen, Alter, Aussehen, \
                 Orte, Zeitangaben und Abläufe. Beziehe die Notizen und früheren Kapitel ein, die dir der \
                 Workflow bereitstellt. Verwende ausschließlich \"comment\"."
            }
        }
    }
}

const FORMAT: &str = "Antworte ausschließlich mit einem JSON-Array, ohne weiteren Text. Jedes Element ist ein Objekt:\n\
{\"kind\": \"replace\" oder \"comment\", \"original\": die betroffene Textstelle, zeichengenau und wörtlich aus dem Text kopiert, so kurz wie möglich, aber eindeutig, \"replacement\": der neue Text (nur bei replace), \"reason\": eine kurze Begründung auf Deutsch}.\n\
Wenn es nichts anzumerken gibt, antworte mit [].";

/// The full question for `task` about `text`.
pub fn question(task: Task, text: &str) -> String {
    format!("{}\n\n{FORMAT}\n\n<<<TEXT\n{text}\nTEXT>>>", task.instructions())
}
