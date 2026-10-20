//! Q21: `router.fanout.pin_escape`, the fine-pitch pin escape switch.
//!
//! - `off` (or unset, the default in both profiles): nothing runs, and the
//!   session is byte-identical to a job without the key.
//! - `straight`: the board stage [`PIN_ESCAPE_STAGE`] runs before the fanout
//!   ([`StagePhase::BeforeFanout`](super::StagePhase::BeforeFanout)). It is
//!   `freerouting-krt`'s X10 stub planner (`fanout::qfn::escape`): on every
//!   QFN, QFP, SOIC or SOP part with edge rows at a pitch of at most 0.65 mm,
//!   one straight stub per pin along the pin's outward axis, at
//!   `max(board minimum, min(class width, neck-down width))`, out past the
//!   package outline; a stub that doesn't keep its clearance is not
//!   inserted. The engine only names the stage: `freerouting-krt` registers
//!   it, and `freerouting-stages` hands that registry to the CLI and the
//!   GUI, so a job on [`StageRegistry::builtin`](super::StageRegistry::builtin)
//!   with `straight` fails as an unknown stage.
//!
//! The selected stage is listed after `router.special.diff_pairs` in
//! [`StageSelection::stages`](super::StageSelection::stages), so with both
//! keys set the pairs are routed first and the stubs skip their pins
//! (already connected).
//!
//! Java has no such pass (DIVERGENCES, "freerouting-krt"); `legacy` leaves the
//! key unset.

use super::UnknownStage;
use crate::settings::RouterSettings;

/// The settings key.
pub const SETTINGS_KEY: &str = "router.fanout.pin_escape";
/// The board stage `straight` selects (registered by `freerouting-krt`).
pub const PIN_ESCAPE_STAGE: &str = "krt-pin-escape";
/// The accepted values.
pub const MODES: [&str; 2] = ["off", "straight"];

/// The value of `router.fanout.pin_escape`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PinEscape {
    /// No escape stubs (unset).
    #[default]
    Off,
    /// Straight stubs on fine-pitch edge-row pins (module docs).
    Straight,
}

impl PinEscape {
    /// The mode of a setting value; unset or empty is [`PinEscape::Off`].
    /// Fails on any other value, naming it.
    pub fn parse(value: Option<&str>) -> Result<Self, String> {
        match value.map(str::trim) {
            None | Some("") | Some("off") => Ok(PinEscape::Off),
            Some("straight") => Ok(PinEscape::Straight),
            Some(other) => Err(format!(
                "{SETTINGS_KEY}: unknown value '{other}' (expected 'off' or 'straight')"
            )),
        }
    }

    /// The mode of `settings`; an unknown value reads as off here
    /// ([`check_settings`] refuses it before routing).
    pub fn of(settings: &RouterSettings) -> Self {
        Self::parse(settings.fanout.pin_escape.as_deref()).unwrap_or_default()
    }

    /// The board stage the mode selects.
    pub fn stage(self) -> Option<&'static str> {
        match self {
            PinEscape::Off => None,
            PinEscape::Straight => Some(PIN_ESCAPE_STAGE),
        }
    }

    /// The setting value of the mode.
    pub fn as_str(self) -> &'static str {
        match self {
            PinEscape::Off => "off",
            PinEscape::Straight => "straight",
        }
    }
}

/// Refuses an unknown `router.fanout.pin_escape` value, as an unknown stage
/// id is refused (`run_pipeline` calls it before anything changes).
pub fn check_settings(settings: &RouterSettings) -> Result<(), UnknownStage> {
    PinEscape::parse(settings.fanout.pin_escape.as_deref())
        .map(|_| ())
        .map_err(|_| UnknownStage {
            key: SETTINGS_KEY,
            id: settings.fanout.pin_escape.clone().unwrap_or_default(),
            known: MODES.to_vec(),
        })
}
