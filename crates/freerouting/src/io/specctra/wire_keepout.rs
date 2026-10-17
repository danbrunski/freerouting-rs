//! Q09.3: `router.wire_keepout`, how a Specctra `wire_keepout` is built.
//!
//! Specctra's `wire_keepout` keeps wires out and lets vias through; KiCad
//! writes one for a rule area that bans tracks only. Java's lexer reads it
//! as `keepout` (`SpecctraFileDescription.flex:201`), so it blocks vias too,
//! and the DSN reader builds it that way: an [`AreaKind::Obstacle`],
//! recorded in [`BasicBoard::wire_keepouts`]. The setting chooses:
//!
//! - `keepout` (the default, also when unset): Java's reading; nothing
//!   changes, so routes are byte-identical to Java's;
//! - `traces_only`: the recorded areas become [`AreaKind::TraceObstacle`]
//!   (traces of every net kept out, vias of every net let through). A
//!   deliberate divergence (`docs/DIVERGENCES.md`, "Board model").
//!
//! Only structure-level `wire_keepout`s are recorded. One in an image
//! (`library`) becomes a package keepout like `keepout` and stays one.
//!
//! [`AreaKind::Obstacle`]: crate::board::items::AreaKind::Obstacle
//! [`AreaKind::TraceObstacle`]: crate::board::items::AreaKind::TraceObstacle

use crate::board::BasicBoard;
use crate::job::{LogLevel, LogLine};
use crate::settings::RouterSettings;

/// The settings key, as the CLI and the JSON file name it.
pub const SETTINGS_KEY: &str = "router.wire_keepout";

/// How a `wire_keepout` is built.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WireKeepoutMode {
    /// Java's reading: a full keepout, blocking traces and vias.
    #[default]
    Keepout,
    /// A trace-only keepout: vias pass.
    TracesOnly,
}

impl WireKeepoutMode {
    /// The mode of a setting value; unset is [`WireKeepoutMode::Keepout`].
    /// Fails on any other value, naming it.
    pub fn parse(value: Option<&str>) -> Result<Self, String> {
        match value.map(str::trim) {
            None | Some("") | Some("keepout") => Ok(WireKeepoutMode::Keepout),
            Some("traces_only") => Ok(WireKeepoutMode::TracesOnly),
            Some(other) => Err(format!(
                "{SETTINGS_KEY}: unknown value '{other}' (expected 'keepout' or 'traces_only')"
            )),
        }
    }
}

/// Builds the board's `wire_keepout`s as `settings` ask (the C03 apply
/// step, before the pour guard). Returns the log line, if any.
pub fn apply(board: &mut BasicBoard, settings: &RouterSettings) -> Result<Option<LogLine>, String> {
    match WireKeepoutMode::parse(settings.wire_keepout.as_deref())? {
        WireKeepoutMode::Keepout => Ok(None),
        WireKeepoutMode::TracesOnly => {
            let changed = board.make_wire_keepouts_trace_only();
            Ok(Some(LogLine {
                level: LogLevel::Info,
                text: format!(
                    "{SETTINGS_KEY} = traces_only: {changed} wire_keepout area(s) keep out traces \
                     of every net and let vias through."
                ),
            }))
        }
    }
}
