//! The key table. Java's keys (`gui.interactive.InteractiveState.keyTyped`
//! and `MenuState`) plus `Home`; the mouse model is in
//! [`crate::viewport`].
//!
//! | Key | Action |
//! |---|---|
//! | `A`, `Home` | zoom all (fit the board) |
//! | `Z` / `O` | zoom in / out at the cursor |
//! | `C` | centre the view on the cursor |
//! | `F` | zoom to the rectangle dragged next |
//! | `1`-`9` | active layer |
//! | `+` / `-` | next / previous signal layer |
//! | `,` | crosshair cursor on or off |
//! | `Esc` | cancel |
//! | `G` | ratsnest on or off |
//! | `V` | clearance violation markers on or off |
//! | `W` | zoom to the selection |
//! | `U`, `Ctrl+Z` | undo |
//! | `B`, `Ctrl+Y` | redo |
//! | `Ctrl+O` | open |

use eframe::egui::{InputState, Key};

use crate::commands::Command;

/// Java's tool keys, kept free for the checklists that add them: S, R and D
/// (inspect, route and drag modes). G, V, U, B and W are used since G03.
pub const RESERVED: [Key; 3] = [Key::S, Key::R, Key::D];

const DIGITS: [Key; 9] = [
    Key::Num1,
    Key::Num2,
    Key::Num3,
    Key::Num4,
    Key::Num5,
    Key::Num6,
    Key::Num7,
    Key::Num8,
    Key::Num9,
];

/// The commands for the keys pressed this frame. `typing` is true while a
/// text field has the keyboard, which then gets every key but `Ctrl+O`.
pub fn key_commands(input: &InputState, typing: bool) -> Vec<Command> {
    let mut out = Vec::new();
    if input.modifiers.command && input.key_pressed(Key::O) {
        out.push(Command::OpenDialog);
    }
    if !typing && input.modifiers.command && !input.modifiers.alt {
        if input.key_pressed(Key::Z) {
            out.push(Command::Undo);
        }
        if input.key_pressed(Key::Y) {
            out.push(Command::Redo);
        }
    }
    if typing || input.modifiers.command || input.modifiers.alt {
        return out;
    }
    let table = [
        (Key::A, Command::FitBoard),
        (Key::Home, Command::FitBoard),
        (Key::Z, Command::ZoomIn),
        (Key::O, Command::ZoomOut),
        (Key::C, Command::CenterOnCursor),
        (Key::F, Command::ZoomToRectangle),
        (Key::Comma, Command::ToggleCrosshair),
        (Key::Escape, Command::Cancel),
        (Key::Plus, Command::StepActiveLayer(1)),
        (Key::Equals, Command::StepActiveLayer(1)),
        (Key::Minus, Command::StepActiveLayer(-1)),
        (Key::G, Command::ToggleRatsnest),
        (Key::V, Command::ToggleViolations),
        (Key::W, Command::ZoomSelection),
        (Key::U, Command::Undo),
        (Key::B, Command::Redo),
    ];
    for (key, command) in table {
        if input.key_pressed(key) {
            out.push(command);
        }
    }
    for (n, key) in DIGITS.iter().enumerate() {
        if input.key_pressed(*key) {
            out.push(Command::SetActiveLayer(n));
        }
    }
    out
}
