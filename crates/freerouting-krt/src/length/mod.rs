//! Length and time matching (X07): the `krt-meander` board stage.
//!
//! Java Freerouting checks per-class minimum and maximum trace lengths
//! (`NetIncompletes.calcLengthViolation`) but never lengthens a trace. This
//! stage does, after the optimizer, with trombone meanders inserted as
//! ordinary traces, each bump checked by the exact DRC:
//! - [`measure`]: a net's length, trace plus via barrels plus pad-to-die
//!   (X07.1);
//! - [`targets`]: class windows and `router.length_matching.groups`
//!   (X07.2);
//! - [`meander`]: one bump's exact integer geometry (X07.3);
//! - [`matching`]: the loop that places bumps through B09 transactions
//!   (X07.4);
//! - [`timing`]: delays from the stackup's εr, for time groups (X07.5).
//!
//! **Selection.** `router.special.length_matching = "krt-meander"` (or the
//! id in `router.special.stages`) adds the stage; it runs after the
//! optimizer ([`StagePhase::AfterOptimizer`]), so a batch run's pull-tight
//! can't undo the meanders. An interactive optimize later may straighten
//! them again. Unselected, nothing here runs and sessions stay
//! Freerouting's.
//!
//! **Inputs.** The stage built by the registry has no stackup: a DSN input
//! carries none, and the CLI does not hand the KiCad reader's extras to a
//! stage yet (C07). Its report then says "stackup unknown" and time groups
//! are refused. [`KrtMeander::with_model`] takes the KiCad reader's
//! stackup and pad-to-die lengths ([`LengthModel::from_kicad`]).
//!
//! **Determinism** (D2): groups by name, nets by number, runs by length
//! then trace id and segment, bumps along a run from its start; budgets are
//! counts of bump positions, never time. Running the stage twice changes
//! nothing the second time: a net is never lengthened past its aim, and a
//! group's aim is its longest member, which the stage doesn't touch.

use freerouting::autoroute::stages::{
    BoardStage, StageCtx, StagePhase, StageRegistry, StageReport,
};
use freerouting::board::RoutingBoard;
use freerouting::settings::RouterSettings;

pub mod matching;
pub mod meander;
pub mod measure;
pub mod targets;
pub mod timing;

pub use matching::{MatchReport, NetOutcome, Status, run_matching};
pub use measure::{LengthModel, NetLength, Stackup, Units, measure_net};

/// The stage id.
pub const MEANDER_ID: &str = "krt-meander";

/// The `krt-meander` board stage (module docs).
#[derive(Clone, Debug, Default)]
pub struct KrtMeander {
    /// The stackup and pad-to-die lengths.
    pub model: LengthModel,
    /// The groups as given (`router.length_matching.groups`).
    pub groups: Vec<String>,
}

impl KrtMeander {
    /// The stage of `settings`, without a stackup.
    pub fn from_settings(settings: &RouterSettings) -> KrtMeander {
        KrtMeander {
            model: LengthModel::none(),
            groups: settings.length_matching.groups.clone().unwrap_or_default(),
        }
    }

    /// The same stage with `model`'s stackup and pad-to-die lengths.
    pub fn with_model(mut self, model: LengthModel) -> KrtMeander {
        self.model = model;
        self
    }

    /// Runs the matching on `board` and returns what it did. A settings
    /// error leaves the board alone and is the report's only error.
    pub fn run_on(&self, board: &mut RoutingBoard, ctx: &StageCtx) -> MatchReport {
        let nets = targets::board_nets(board);
        let groups = targets::parse_groups(&self.groups).and_then(|specs| {
            specs
                .iter()
                .map(|s| targets::resolve_group(s, &nets))
                .collect::<Result<Vec<_>, _>>()
        });
        match groups {
            Ok(groups) => run_matching(board, &groups, &self.model, ctx.cancel),
            Err(e) => MatchReport {
                errors: vec![format!("settings error: {e}; nothing changed")],
                stackup_unknown: self.model.stackup_for(board).is_none(),
                ..MatchReport::default()
            },
        }
    }
}

impl BoardStage for KrtMeander {
    fn id(&self) -> &'static str {
        MEANDER_ID
    }

    fn phase(&self) -> StagePhase {
        StagePhase::AfterOptimizer
    }

    fn run(&mut self, board: &mut RoutingBoard, ctx: &mut StageCtx) -> StageReport {
        let report = self.run_on(board, ctx);
        StageReport {
            log: report.log(board),
        }
    }
}

/// Registers `krt-meander`.
pub(crate) fn register(registry: &mut StageRegistry) {
    registry.board_stages.insert(MEANDER_ID, |settings| {
        Box::new(KrtMeander::from_settings(settings))
    });
}
