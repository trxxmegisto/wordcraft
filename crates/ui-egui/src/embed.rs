//! WordCraft inside another egui app: a tab in a host's docking layout rather than a window of
//! its own.
//!
//! The host builds a [`WordApp`] as usual, calls [`WordApp::set_embedded`] once, forwards
//! [`WordApp::logic`] and [`WordApp::raw_input_hook`] from its own `eframe::App`, and draws the
//! editor with [`WordApp::embedded_ui`] into the area it gives WordCraft. Embedded, WordCraft:
//!
//! - shows every document in that one area, as the web app does (no native windows);
//! - draws no title bar, and sends no window commands (title, full screen, drag, maximize): the
//!   window is the host's;
//! - takes the keyboard back after a click only when the click was in its own area, and runs its
//!   command shortcuts (Ctrl+S, Ctrl+Z…) only while its area was the last one clicked, so typing
//!   and shortcuts in the host's other panes stay the host's.

use crate::WordApp;
use egui::{Pos2, Rect};

/// The embedded app's state: where the host last drew it, and whether it was the last area
/// clicked.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Embed {
    /// The area the host gave WordCraft on the last frame it was drawn.
    area: Option<Rect>,
    /// The last click in the window landed in `area`.
    active: bool,
}

impl WordApp {
    /// Draw inside a host's window instead of a window of its own (see the module docs). Call
    /// once, before the first frame.
    pub fn set_embedded(&mut self, on: bool) {
        if on {
            self.windows.single();
            self.embed = Some(Embed::default());
        } else {
            self.embed = None;
        }
    }

    /// Whether the app is drawn inside a host's window ([`WordApp::set_embedded`]).
    pub fn embedded(&self) -> bool {
        self.embed.is_some()
    }

    /// Whether the embedded editor was the last area clicked, so the keyboard is WordCraft's.
    /// Always true for the WordCraft app itself.
    pub fn embedded_active(&self) -> bool {
        self.embed.is_none_or(|e| e.active)
    }

    /// Make the embedded editor the active area (the host brought its tab forward, say) and give
    /// the page the keyboard.
    pub fn embedded_activate(&mut self) {
        if let Some(e) = &mut self.embed {
            e.active = true;
            self.canvas.want_focus = true;
        }
    }

    /// Draw the editor (ribbon, page, panes, status bar, dialogs) into the space left in `ui`.
    /// Embedded only; the WordCraft app itself draws with [`WordApp::ui`].
    pub fn embedded_ui(&mut self, ui: &mut egui::Ui) {
        let area = ui.available_rect_before_wrap();
        if let Some(e) = &mut self.embed {
            e.area = Some(area);
            if let Some(pos) = ui.input(|i| if i.pointer.any_pressed() { i.pointer.interact_pos() } else { None }) {
                // A click on a dialog or menu WordCraft opened is WordCraft's too, wherever it lies.
                let on_popup = ui.ctx().layer_id_at(pos).is_some_and(|l| l.order != egui::Order::Background);
                e.active = area.contains(pos) || (e.active && on_popup);
            }
        }
        self.window_ui(ui);
    }

    /// Whether a click at `pos` belongs to WordCraft: always for the app itself; embedded, only
    /// inside the area the host gave it.
    pub(crate) fn owns_point(&self, pos: Pos2) -> bool {
        match self.embed {
            None => true,
            Some(e) => e.area.is_some_and(|a| a.contains(pos)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Embed;
    use egui::{Rect, pos2};

    #[test]
    fn a_click_outside_the_area_is_not_wordcrafts() {
        let e = Embed { area: Some(Rect::from_min_max(pos2(100.0, 100.0), pos2(400.0, 300.0))), active: true };
        let inside = e.area.is_some_and(|a| a.contains(pos2(200.0, 200.0)));
        let outside = e.area.is_some_and(|a| a.contains(pos2(50.0, 200.0)));
        assert!(inside);
        assert!(!outside);
    }
}
