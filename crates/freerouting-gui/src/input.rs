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
//! | `Esc` | cancel (menu, keepout, zoom rectangle, selection) |
//! | `Enter` | complete a keepout under construction |
//! | `R` `D` `S` | without a selection: route, drag and inspect tool |
//! | `T` `W` `P` | without a selection: start a route at the cursor, pin swap, push-and-shove on or off |
//! | `M` `Ctrl+D` | with a selection: move, copy |
//! | `N` `S` `E` `I` `R` `Delete` | with items selected: extend to nets, connected sets, connections; info; zoom region; delete |
//! | `I` | without a selection: select at the cursor |
//! | `G` | ratsnest on or off |
//! | `V` | clearance violation markers on or off |
//! | `W` | zoom to the selection |
//! | `U`, `Ctrl+Z` | undo |
//! | `B`, `Ctrl+Y` | redo |
//! | `Ctrl+O` | open |

use eframe::egui::{InputState, Key};

use crate::commands::Command;

/// Java's tool keys that were kept free until G09 added them: none left. `R`,
/// `D` and `S` (without a selection) switch the tool, `T`, `W` and `P` start
/// a route, a pin swap and toggle push-and-shove (`tools::route`), `M` and
/// `Ctrl+D` move and copy the selection (`tools::move_component`).
pub const RESERVED: [Key; 0] = [];

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
        (Key::Enter, Command::Complete),
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
