//! The Freerouting desktop app, as a library so integration tests can drive
//! [`App`] with `egui_kittest`.
//!
//! The GUI only draws and maps input; board logic lives in the engine crate.
//! The modules:
//!
//! | Module | What it does |
//! |---|---|
//! | [`app`] | `App`: eframe state, `execute(Command)`, the frame loop |
//! | [`analysis`], [`overlays`], [`selection`], [`lists`] | background ratsnest and DRC, the overlays drawn on the canvas, the selection, the Info lists |
//! | [`commands`] | `Command`, every user action |
//! | [`dialogs`] | the rules dialogs: net classes, clearance matrix, vias, via rules, routing settings (G06) |
//! | [`input`] | the key table (Java's keys) |
//! | [`viewport`] | camera, visibility, mouse model, hover status |
//! | [`look`], [`windows`] | colours, visibility levels and the virtual layers; the display, colour, log and About windows (G04) |
//! | [`session`] | the open design: the engine board, files, loading |
//! | [`scene`], [`view`] | the board flattened to drawable shapes, and drawing |
//! | [`canvas`] | the board's geometry in chunks, the wgpu pipeline that draws it |
//! | [`bench_synthetic`] | the tiled stress board for `--bench-synthetic` |
//! | [`panels`], [`menu`] | side panel sections, warnings window, menu bar |
//! | [`tools`] | inspect mode, selection operations, safe edits, context menus, keepout construction (G05); the editor host, route, drag, pin swap and move (G09) |
//! | [`settings`] | persisted preferences |
//! | [`stats`], [`bench`], [`capture`], [`args`] | frame statistics, `--bench`, `--screenshot` |

pub mod analysis;
pub mod app;
mod app_analysis;
pub mod args;
pub mod bench;
mod bench_drag;
pub mod bench_synthetic;
pub mod canvas;
pub mod capture;
pub mod commands;
pub mod dialogs;
pub mod drc_export;
pub mod file_dialog;
pub mod input;
mod layout;
pub mod lists;
pub mod look;
pub mod menu;
pub mod overlays;
pub mod panels;
pub mod scene;
pub mod selection;
pub mod session;
pub mod settings;
pub mod stats;
pub mod text;
pub mod toolbar;
pub mod tools;
pub mod view;
pub mod viewport;
pub mod windows;

pub use app::App;
