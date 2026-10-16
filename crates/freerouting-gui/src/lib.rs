//! The Freerouting desktop app, as a library so integration tests can drive
//! [`App`] with `egui_kittest`.
//!
//! The GUI only draws and maps input; board logic lives in the engine crate.
//! The modules:
//!
//! | Module | What it does |
//! |---|---|
//! | [`app`] | `App`: eframe state, `execute(Command)`, the frame loop |
//! | [`commands`] | `Command`, every user action |
//! | [`input`] | the key table (Java's keys) |
//! | [`viewport`] | camera, visibility, mouse model, hover status |
//! | [`session`] | the open design: the engine board, files, loading |
//! | [`scene`], [`view`] | the board flattened to drawable shapes, and drawing |
//! | [`panels`], [`menu`] | side panel sections, warnings window, menu bar |
//! | [`settings`] | persisted preferences |
//! | [`stats`], [`bench`], [`capture`], [`args`] | frame statistics, `--bench`, `--screenshot` |

pub mod app;
pub mod args;
pub mod bench;
pub mod capture;
pub mod commands;
pub mod file_dialog;
pub mod input;
mod layout;
pub mod menu;
pub mod panels;
pub mod scene;
pub mod session;
pub mod settings;
pub mod stats;
pub mod view;
pub mod viewport;

pub use app::App;
