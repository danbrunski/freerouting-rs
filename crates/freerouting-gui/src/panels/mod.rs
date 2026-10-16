//! The side panel's sections and the warnings window. Each is a function
//! `ui(ui, ..)` that returns the commands the user triggered, so a test can
//! run one panel alone with `Harness::new_ui_state`.

pub mod board_stats;
pub mod layers;
pub mod warnings;
