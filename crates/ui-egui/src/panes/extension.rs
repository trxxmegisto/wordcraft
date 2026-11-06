//! assistant-hook (KWordCraft): task panes that live outside this crate.
//!
//! A [`PaneExtension`] is registered on the [`WordApp`] once ([`WordApp::register_pane`]) and is
//! then a task pane like the built-in ones: [`Pane::Extension`] with the extension's key, drawn,
//! docked, floated, reordered and closed by the same pane host. `ui.extensionPane {"pane": key,
//! "value"?: bool}` opens, closes or toggles it; `ui.paneDock` docks it.
//!
//! An extension pane's place and order last for the run; `ui.json` keeps only the built-in panes'.

use egui::Ui;
use serde_json::{Value, json};

use super::Pane;
use crate::WordApp;

/// A task pane provided from outside `wordcraft-ui-egui`.
pub trait PaneExtension {
    /// The pane's key (as in `ui.paneDock`): unique, and not one of the built-in panes' keys.
    fn key(&self) -> &'static str;
    /// The title on the pane's title bar, already in the interface language.
    fn title(&self) -> String;
    /// Whether the pane is shown.
    fn is_open(&self) -> bool;
    /// Show or hide the pane (its title bar's Close, `ui.extensionPane`).
    fn set_open(&mut self, open: bool);
    /// Draw the pane's content. The app is the whole application: run commands through
    /// [`WordApp::run`] so undo, the status bar and the views behave as for any other edit.
    fn body(&mut self, app: &mut WordApp, ui: &mut Ui);
}

/// The registered extension panes, in registration order.
#[derive(Default)]
pub struct PaneExtensions(Vec<Box<dyn PaneExtension>>);

impl PaneExtensions {
    /// The pane for `key`, if an extension has that key.
    pub(crate) fn pane(&self, key: &str) -> Option<Pane> {
        self.0.iter().find(|e| e.key() == key).map(|e| Pane::Extension(e.key()))
    }

    /// Every extension pane, in registration order.
    pub(crate) fn panes(&self) -> impl Iterator<Item = Pane> + '_ {
        self.0.iter().map(|e| Pane::Extension(e.key()))
    }

    /// The extension panes open now.
    pub(crate) fn open(&self) -> impl Iterator<Item = Pane> + '_ {
        self.0.iter().filter(|e| e.is_open()).map(|e| Pane::Extension(e.key()))
    }

    pub(crate) fn title(&self, key: &str) -> Option<String> {
        self.get(key).map(|e| e.title())
    }

    pub(crate) fn set_open(&mut self, key: &str, open: bool) {
        if let Some(e) = self.0.iter_mut().find(|e| e.key() == key) {
            e.set_open(open);
        }
    }

    fn get(&self, key: &str) -> Option<&dyn PaneExtension> {
        self.0.iter().find(|e| e.key() == key).map(|e| &**e)
    }
}

impl WordApp {
    /// Add a task pane. A key that is already taken (by a built-in pane or another extension)
    /// is refused.
    pub fn register_pane(&mut self, pane: Box<dyn PaneExtension>) -> Result<(), String> {
        let key = pane.key();
        if key.is_empty() || Pane::from_key(key).is_some() || self.pane_extensions.pane(key).is_some() {
            return Err(format!("the task pane key `{key}` is empty or already taken"));
        }
        self.pane_extensions.0.push(pane);
        Ok(())
    }
}

/// Draw extension pane `key`. The extension is taken out of the app while it draws, so it may
/// use the whole app; panes it registers meanwhile are kept.
pub(super) fn body(app: &mut WordApp, ui: &mut Ui, key: &str) {
    let mut all = std::mem::take(&mut app.pane_extensions);
    if let Some(e) = all.0.iter_mut().find(|e| e.key() == key) {
        e.body(app, ui);
    }
    all.0.append(&mut app.pane_extensions.0);
    app.pane_extensions = all;
}

/// `ui.extensionPane {"pane": key, "value"?: bool}`: show (`true`), hide (`false`) or toggle an
/// extension pane.
pub(crate) fn command(app: &mut WordApp, p: &Value) -> Result<Value, String> {
    let key = p.get("pane").and_then(Value::as_str).ok_or("`pane` must be an extension pane's key")?;
    let Some(ext) = app.pane_extensions.get(key) else { return Err(format!("unknown extension pane `{key}`")) };
    let open = match p.get("value") {
        None => !ext.is_open(),
        Some(Value::Bool(on)) => *on,
        Some(_) => return Err("`value` must be true or false".into()),
    };
    app.pane_extensions.set_open(key, open);
    Ok(json!({"pane": key, "open": open}))
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use craft_ui::panes::{Edge, PaneRequest, Place};
    use serde_json::json;
    use wordcraft_doc::Document;
    use wordcraft_engine::Session;

    use super::*;
    use crate::panes::{apply, open};

    struct Probe {
        key: &'static str,
        open: bool,
        drawn: Rc<Cell<u32>>,
    }

    impl PaneExtension for Probe {
        fn key(&self) -> &'static str {
            self.key
        }
        fn title(&self) -> String {
            "Probe".into()
        }
        fn is_open(&self) -> bool {
            self.open
        }
        fn set_open(&mut self, open: bool) {
            self.open = open;
        }
        fn body(&mut self, app: &mut WordApp, ui: &mut Ui) {
            self.drawn.set(self.drawn.get() + 1);
            // The whole app is reachable while the pane draws.
            let _ = app.run("document.text", json!({}));
            ui.label("probe");
        }
    }

    fn app_with_probe() -> (WordApp, Rc<Cell<u32>>) {
        let mut app = WordApp::new(Session::new(Document::from_text("Hallo")), Default::default());
        let drawn = Rc::new(Cell::new(0));
        app.register_pane(Box::new(Probe { key: "probe", open: false, drawn: drawn.clone() })).unwrap();
        (app, drawn)
    }

    #[test]
    fn keys_must_be_new() {
        let (mut app, drawn) = app_with_probe();
        for key in ["probe", "comments", "styles", ""] {
            assert!(app.register_pane(Box::new(Probe { key, open: false, drawn: drawn.clone() })).is_err(), "{key}");
        }
    }

    #[test]
    fn opens_closes_and_toggles_like_a_built_in_pane() {
        let (mut app, _) = app_with_probe();
        assert!(!open(&app).contains(&Pane::Extension("probe")));
        assert_eq!(app.run("ui.extensionPane", json!({"pane": "probe"})).unwrap(), json!({"pane": "probe", "open": true}));
        assert!(open(&app).contains(&Pane::Extension("probe")));
        // The title bar's Close goes through the pane host's request.
        apply(&mut app, PaneRequest::Close(Pane::Extension("probe")));
        assert!(!open(&app).contains(&Pane::Extension("probe")));
        app.run("ui.extensionPane", json!({"pane": "probe", "value": true})).unwrap();
        app.run("ui.extensionPane", json!({"pane": "probe", "value": true})).unwrap();
        assert!(open(&app).contains(&Pane::Extension("probe")), "true shows, it doesn't toggle");
    }

    #[test]
    fn docks_floats_and_orders_by_its_key() {
        let (mut app, _) = app_with_probe();
        app.run("ui.extensionPane", json!({"pane": "probe", "value": true})).unwrap();
        app.run("view.commentsPane", json!({"value": true})).unwrap();
        assert_eq!(app.run("ui.paneDock", json!({"pane": "probe"})).unwrap(), json!({"pane": "probe", "dock": "right"}));
        assert_eq!(app.run("ui.paneDock", json!({"pane": "probe", "dock": "float"})).unwrap(), json!({"pane": "probe", "dock": "float"}));
        apply(&mut app, PaneRequest::Place(Pane::Extension("probe"), Place::Docked(Edge::Start)));
        assert_eq!(app.run("ui.paneDock", json!({"pane": "probe"})).unwrap(), json!({"pane": "probe", "dock": "left"}));
        // Last by default; moved before Comments it stays there.
        let open_now = |app: &WordApp| open(app).into_iter().filter(|p| matches!(p, Pane::Comments | Pane::Extension(_))).collect::<Vec<_>>();
        assert_eq!(open_now(&app), [Pane::Comments, Pane::Extension("probe")]);
        app.run("ui.paneDock", json!({"pane": "probe", "before": "comments"})).unwrap();
        assert_eq!(open_now(&app), [Pane::Extension("probe"), Pane::Comments]);
        app.run("ui.paneDock", json!({"pane": "comments", "before": "probe"})).unwrap();
        assert_eq!(open_now(&app), [Pane::Comments, Pane::Extension("probe")]);
    }

    #[test]
    fn draws_with_the_whole_app_and_keeps_its_registration() {
        let (mut app, drawn) = app_with_probe();
        let ctx = egui::Context::default();
        ctx.run_ui(egui::RawInput::default(), |ui| body(&mut app, ui, "probe")).drop_without_applying_deltas();
        assert_eq!(drawn.get(), 1);
        assert!(app.pane_extensions.pane("probe").is_some(), "the extension is back after drawing");
        ctx.run_ui(egui::RawInput::default(), |ui| body(&mut app, ui, "unknown")).drop_without_applying_deltas();
        assert_eq!(drawn.get(), 1);
    }

    #[test]
    fn junk_params_are_refused_not_fatal() {
        let (mut app, _) = app_with_probe();
        for bad in [json!({}), json!({"pane": 3}), json!({"pane": "nope"}), json!({"pane": "probe", "value": "yes"}), json!(null), json!([1])] {
            assert!(app.run("ui.extensionPane", bad.clone()).is_err(), "{bad}");
        }
        assert!(app.run("ui.paneDock", json!({"pane": "probe", "before": "probe"})).is_err());
    }
}
