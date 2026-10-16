//! Help > About: the lines of Java's `WindowAbout` (G04.8).
//!
//! Version, the commit the app was built from, the license and the project
//! links. There is no telemetry, sponsor or donate line (G00 question 2).

use eframe::egui;

use crate::text;

/// The lines of the window, in order.
pub fn lines() -> Vec<String> {
    vec![
        text::ABOUT_DESCRIPTION.to_string(),
        format!("{} {}", text::ABOUT_VERSION, text::VERSION),
        format!("{} {}", text::ABOUT_COMMIT, text::GIT_COMMIT),
        text::ABOUT_LICENSE.to_string(),
        text::ABOUT_WARRANTY.to_string(),
        text::ABOUT_PORT.to_string(),
        text::ABOUT_HOMEPAGE.to_string(),
        text::ABOUT_SUPPORT.to_string(),
    ]
}

pub fn ui(ui: &mut egui::Ui) {
    let lines = lines();
    ui.heading(&lines[0]);
    ui.add_space(6.0);
    for line in &lines[1..] {
        ui.label(line);
    }
}
