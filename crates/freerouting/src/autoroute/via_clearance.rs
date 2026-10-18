//! Q14: `router.via_clearance`, which clearance class a routed via gets.
//!
//! A via the router inserts takes its clearance class from its `ViaInfo`
//! (Java `ForcedViaInserter`, `InsertFoundConnectionAlgorithm`). The DSN
//! reader gives each net class with a clearance rule its own via infos
//! (`<padstack>-<class>`, with the class's via clearance class), and the
//! class's via rule lists those, so an ordinary route is right. The fanout
//! is not: with `fanout.fallback_to_board_vias` (on by default) its via
//! rule is the class's vias followed by those of the board's first via
//! rule (`RoutingBoard.fanout`, `fanout_pin::fanout_control`), whose infos
//! carry the default class's via clearance class. When the class's via
//! does not fit, the maze search's drill check and the via insertion try
//! the fallback via with its own, smaller clearance, and a net of a
//! 0.25 mm class gets a via cleared at 0.2 mm. KiCad clears a via by its
//! net class, so its DRC reports what Freerouting's own DRC accepts
//! (canary-pwr, 2026-10-17: `+12V_BUS` and `VCAP` vias at 0.20 to
//! 0.245 mm from other nets).
//!
//! The setting chooses:
//!
//! - `via_info` (the default, also when unset): Java's rule, each via info's
//!   own clearance class; profile `legacy`, and routes are byte-identical
//!   to Java's;
//! - `net_class`: a via the autorouter checks or inserts for net N uses N's
//!   net class's via clearance class (`default_item_clearance_classes` of
//!   `Via`), whichever via info it came from. A deliberate divergence
//!   (`docs/DIVERGENCES.md`, "Autorouter"), and profile `enhanced`'s
//!   default since N05.3 accepted it
//!   (`docs/roadmap/prereg/N05-via-clearance-netclass.json`).
//!
//! Only the autorouter reads it ([`AutorouteControl`]): the maze search's
//! drill check, the first-via clearance class (`via_clearance_class`, which
//! decides which vias a search may rip up) and the inserted via. The DRC
//! needs no change: it checks every via with its own clearance class, so a
//! via of a 0.25 mm-class net cleared at 0.2 mm is a violation whenever the
//! via carries its net's class. A net without a net class (the fallback of
//! `AutorouteControl.initNet`) keeps the via info's class.
//!
//! [`AutorouteControl`]: crate::autoroute::control::AutorouteControl

use crate::settings::RouterSettings;

/// The settings key, as the CLI and the JSON file name it.
pub const SETTINGS_KEY: &str = "router.via_clearance";

/// Where a routed via's clearance class comes from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ViaClearanceMode {
    /// Java's rule: the via info's own clearance class.
    #[default]
    ViaInfo,
    /// The via clearance class of the net's class (KiCad's rule).
    NetClass,
}

impl ViaClearanceMode {
    /// The mode of a setting value; unset or empty is
    /// [`ViaClearanceMode::ViaInfo`]. Fails on any other value, naming it.
    pub fn parse(value: Option<&str>) -> Result<Self, String> {
        match value.map(str::trim) {
            None | Some("") | Some("via_info") => Ok(ViaClearanceMode::ViaInfo),
            Some("net_class") => Ok(ViaClearanceMode::NetClass),
            Some(other) => Err(format!(
                "{SETTINGS_KEY}: unknown value '{other}' (expected 'via_info' or 'net_class')"
            )),
        }
    }

    /// The mode `settings` ask for; an unknown value reads as the default
    /// ([`validate`] reports it).
    pub fn of(settings: &RouterSettings) -> Self {
        Self::parse(settings.via_clearance.as_deref()).unwrap_or_default()
    }
}

/// Checks the setting's value (the job's load sequence calls it).
pub fn validate(settings: &RouterSettings) -> Result<ViaClearanceMode, String> {
    ViaClearanceMode::parse(settings.via_clearance.as_deref())
}
