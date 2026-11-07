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

const FORMAT: &str = "Antworte ausschließlich mit einem JSON-Array, ohne weiteren Text. Jedes Element ist ein Objekt:\n";

const FORMAT_WITH_FACTS: &str = "Antworte ausschließlich mit einem JSON-Objekt, ohne weiteren Text: {\"suggestions\": [...], \"facts\": [...]}.\n\
\"facts\" ist eine Liste kurzer Sätze mit den Fakten aus diesem Text, die für spätere Teile wichtig sind: Namen und ihre Schreibweise, Alter, Aussehen, Beziehungen, Orte, Zeitangaben und Ereignisse, zum Beispiel \"Anna Berger hat blaue Augen.\" oder \"Die Firma sitzt in Hamburg.\". Höchstens 30 Sätze; wiederhole keine bereits bekannten Fakten.\n\
Jedes Element von \"suggestions\" ist ein Objekt:\n";

const ITEM: &str = "{\"kind\": \"replace\" oder \"comment\", \"original\": die betroffene Textstelle, zeichengenau und wörtlich aus dem Text kopiert, so kurz wie möglich, aber eindeutig, \"replacement\": der neue Text (nur bei replace), \"reason\": eine kurze Begründung auf Deutsch}.\n\
\"original\" muss genau so im Text stehen: Kopiere die Stelle Zeichen für Zeichen, ohne Zahlen, Wörter oder Satzzeichen zu ändern, auch wenn sie falsch sind, und ohne Zeilenumbrüche; die Korrektur gehört in \"replacement\" oder \"reason\".\n";

const NOTHING: &str = "Wenn es nichts anzumerken gibt, antworte mit [].";
const NOTHING_WITH_FACTS: &str = "Wenn es nichts anzumerken gibt, ist \"suggestions\" leer: [].";

/// The full question for `task` about `text`.
pub fn question(task: Task, text: &str) -> String {
    question_with_facts(task, text, &[], false)
}

/// The question for one portion of a long text: `known` are the facts the earlier portions
/// established (the text is checked against them), and with `collect` the answer is asked to
/// list this portion's facts too (see [`crate::Reply`]).
pub fn question_with_facts(task: Task, text: &str, known: &[String], collect: bool) -> String {
    let (format, nothing) = if collect { (FORMAT_WITH_FACTS, NOTHING_WITH_FACTS) } else { (FORMAT, NOTHING) };
    let mut q = format!("{}\n\n{format}{ITEM}{nothing}\n\n", task.instructions());
    if !known.is_empty() {
        q.push_str("Bereits bekannte Fakten aus den vorherigen Teilen desselben Textes:\n<<<FAKTEN\n");
        for fact in known {
            q.push_str("- ");
            q.push_str(fact);
            q.push('\n');
        }
        q.push_str(
            "FAKTEN>>>\nWiderspricht der Text unten einem dieser Fakten, melde das als \"comment\" an der Stelle im Text und \
             nenne den Fakt in der Begründung, zum Beispiel \"Widerspruch: Vorher hatte Anna blaue Augen.\". \
             \"original\" stammt immer aus dem Text unten, nie aus den Fakten.\n\n",
        );
    }
    q.push_str("<<<TEXT\n");
    q.push_str(text);
    q.push_str("\nTEXT>>>");
    q
}
