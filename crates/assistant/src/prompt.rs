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

    /// Whether the task's replacements carry their reason as a comment too: a research
    /// correction changes a fact, and the writer should see why.
    pub fn explains_replacements(self) -> bool {
        self == Task::Recherche
    }

    /// Whether `reason` is usable for this task: research notes must open with a verdict
    /// ([`VERDICTS`]); the other tasks take any reason.
    pub fn has_verdict(self, reason: &str) -> bool {
        self != Task::Recherche || VERDICTS.iter().any(|v| reason.trim_start().starts_with(v))
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
                "Du bist Faktenprüfer für einen Romanautor. Prüfe die Tatsachenbehauptungen im Text \
                 (Daten, Jahreszahlen, Orte, historische Ereignisse, Personen, Fachwissen) mit deinem Wissen.\n\
                 Melde nur Stellen, die falsch sind oder bei denen ein begründeter Zweifel besteht. Melde \
                 nichts, was allgemein bekannt und richtig ist, und keine Stelle nur deshalb, weil sie ein \
                 Datum oder einen Ort enthält. Stimmt eine Angabe nach deinem Wissen, melde sie nicht, auch \
                 nicht, um die Formulierung zu verbessern; Stil ist Sache des Lektorats. Lieber wenige gute Hinweise als viele allgemeine; höchstens zehn.\n\
                 Jede Begründung (\"reason\") beginnt mit einem Urteil: \"Falsch:\", \"Zweifelhaft:\" oder \
                 \"Unklar:\". Bei \"Falsch:\" nenne den richtigen Sachverhalt, zum Beispiel \"Falsch: Die \
                 Berliner Mauer fiel am 9. November 1989.\" Bei \"Zweifelhaft:\" sage, was nach deinem Wissen \
                 stimmt und warum du zweifelst. Bei \"Unklar:\" sage konkret, was zu klären ist und wo (etwa \
                 Ortschronik, Museum, Fachlexikon). Schreibe nie nur \"sollte überprüft werden\".\n\
                 Ist eine Angabe sicher falsch, verwende \"replace\": \"original\" ist die kürzeste Stelle mit \
                 dem Fehler (etwa \"9. November 1990\"), \"replacement\" dieselbe Stelle mit der richtigen \
                 Angabe (\"9. November 1989\"); ändere nur den Fehler, nicht den Wortlaut drumherum. In allen \
                 anderen Fällen (\"Zweifelhaft:\", \"Unklar:\") verwende \"comment\".\n\
                 Behauptet eine Figur etwas Falsches, kann das gewollt sein: Verwende dann \"comment\", nie \
                 \"replace\", und schreibe dazu \"(Figurenrede, eventuell beabsichtigt)\".\n\
                 Nenne Quellen nur, wenn du dir sicher bist."
            }
            Task::Konsistenz => {
                "Du prüfst einen Romantext auf innere Widersprüche: Namen, Schreibweisen, Alter, Aussehen, \
                 Orte, Zeitangaben und Abläufe. Beziehe die Notizen und früheren Kapitel ein, die dir der \
                 Workflow bereitstellt. Verwende ausschließlich \"comment\"."
            }
        }
    }
}

/// The verdicts a research note opens with.
pub const VERDICTS: [&str; 3] = ["Falsch:", "Zweifelhaft:", "Unklar:"];

const FORMAT: &str = "Antworte ausschließlich mit einem JSON-Array, ohne weiteren Text. Jedes Element ist ein Objekt:\n\
{\"kind\": \"replace\" oder \"comment\", \"original\": die betroffene Textstelle, zeichengenau und wörtlich aus dem Text kopiert, so kurz wie möglich, aber eindeutig, \"replacement\": der neue Text (nur bei replace), \"reason\": eine kurze Begründung auf Deutsch}.\n\
\"original\" muss genau so im Text stehen: Kopiere die Stelle Zeichen für Zeichen, ohne Zahlen, Wörter oder Satzzeichen zu ändern, auch wenn sie falsch sind, und ohne Zeilenumbrüche; die Korrektur gehört in \"replacement\" oder \"reason\".\n\
Wenn es nichts anzumerken gibt, antworte mit [].";

/// The full question for `task` about `text`.
pub fn question(task: Task, text: &str) -> String {
    format!("{}\n\n{FORMAT}\n\n<<<TEXT\n{text}\nTEXT>>>", task.instructions())
}
