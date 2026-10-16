//! The side panel's sections, the Info lists and the warnings window. Each
//! is a function `ui(ui, ..)` that returns the commands the user triggered,
//! so a test can run one panel alone with `Harness::new_ui_state`.

pub mod board_stats;
pub mod incompletes;
pub mod islands;
pub mod layers;
pub mod length;
pub mod object_list;
pub mod unconnected;
pub mod violations;
pub mod warnings;
