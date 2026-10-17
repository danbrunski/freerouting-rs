//! The side panel's sections, the Info lists, the display windows and the
//! status bar. Each is a function `ui(ui, ..)` that returns the commands the
//! user triggered, so a test can run one panel alone with
//! `Harness::new_ui_state`.

pub mod about;
pub mod board_stats;
pub mod colors;
pub mod components;
pub mod display;
pub mod incompletes;
pub mod islands;
pub mod item_info;
pub mod layers;
pub mod length;
pub mod library;
pub mod log;
pub mod nets;
pub mod object_list;
pub mod progress;
pub mod routing_summary;
pub mod select_settings;
pub mod selection_toolbar;
pub mod status;
pub mod unconnected;
pub mod violations;
pub mod warnings;
