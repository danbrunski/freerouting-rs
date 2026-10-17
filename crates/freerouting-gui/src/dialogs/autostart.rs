//! The auto-start countdown of the front-end mode (G08.5).
//!
//! Java (`GuiManager.createBoardFrame`, at the pin) shows a dialog
//! `auto_start_routing_title` when both `-de` and `-do` are given: the text
//! `auto_start_routing_message` and the buttons "Start Now (n)" and "Cancel".
//! A timer lowers `n` once a second from `dialogConfirmationTimeout` (`-dct`,
//! default 5) and starts routing when it reaches 0. A `-dct` of 0 or less
//! shows no dialog and starts at once.
//!
//! The countdown is GUI time (`egui` input time, so a test harness advances it
//! by its step) and never reaches the router.

use std::time::Duration;

use eframe::egui;

use crate::text;

/// What the user (or the clock) chose.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    /// Start the autorouter, then exit when it finishes.
    StartNow,
    /// Leave routing idle and the app open.
    Cancel,
}

/// A running countdown.
#[derive(Clone, Debug)]
pub struct Autostart {
    timeout: u32,
    /// The GUI time of the first frame the dialog was shown.
    started: Option<f64>,
    /// The button text of the last frame, for tests.
    label: String,
}

impl Autostart {
    pub fn new(timeout: u32) -> Self {
        Autostart {
            timeout,
            started: None,
            label: button_text(timeout),
        }
    }

    /// The seconds shown after `elapsed` seconds: the timeout, lowered by 1
    /// each second; 0 means start.
    pub fn seconds_left(timeout: u32, elapsed: f64) -> u32 {
        let passed = elapsed.max(0.0).floor();
        if passed >= f64::from(timeout) {
            0
        } else {
            timeout - passed as u32
        }
    }

    /// The text of the Start Now button as last shown.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Shows the dialog; returns the choice once made. The clock starts at
    /// the first call.
    pub fn ui(&mut self, ctx: &egui::Context) -> Option<Choice> {
        let now = ctx.input(|i| i.time);
        let started = *self.started.get_or_insert(now);
        let elapsed = now - started;
        let left = Self::seconds_left(self.timeout, elapsed);
        if left == 0 {
            return Some(Choice::StartNow);
        }
        self.label = button_text(left);
        // The next change of the text is at the next whole second.
        let next = (elapsed.floor() + 1.0 - elapsed).max(0.01);
        ctx.request_repaint_after(Duration::from_secs_f64(next));
        let mut choice = None;
        egui::Window::new(text::AUTO_START_ROUTING_TITLE)
            .id(egui::Id::new("autostart-routing"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.set_max_width(420.0);
                ui.label(text::AUTO_START_ROUTING_MESSAGE);
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.button(&self.label).clicked() {
                        choice = Some(Choice::StartNow);
                    }
                    if ui.button(text::AUTO_START_ROUTING_CANCEL).clicked() {
                        choice = Some(Choice::Cancel);
                    }
                });
            });
        choice
    }
}

fn button_text(seconds: u32) -> String {
    format!("{} ({seconds})", text::AUTO_START_ROUTING_STARTNOW)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_down_once_a_second_like_java() {
        let left = |t: f64| Autostart::seconds_left(5, t);
        assert_eq!(left(0.0), 5);
        assert_eq!(left(0.99), 5);
        assert_eq!(left(1.0), 4);
        assert_eq!(left(4.99), 1);
        assert_eq!(left(5.0), 0);
        assert_eq!(left(60.0), 0);
        assert_eq!(Autostart::seconds_left(0, 0.0), 0);
        assert_eq!(button_text(3), "Start Now (3)");
    }
}
