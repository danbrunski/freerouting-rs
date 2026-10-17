//! Differential pairs (X08, X09): the `krt-diffpair` board stage.
//!
//! Java Freerouting has no differential pairs. KiCadRoutingTools routes a
//! pair as one centreline and offsets it into the P and N tracks; this
//! stage does the same on Freerouting's gridless search:
//! - [`model`]: which nets pair, with each pair's width and gap (X08.1);
//! - [`centreline`]: the centreline through the room/door maze search at
//!   the compound half width (X08.2);
//! - [`offset`]: the P and N tracks, miter corners in integer geometry
//!   (X08.3);
//! - [`legs`]: the terminal legs, the B09 insertion with its DRC gate and
//!   the setback fallback (X08.4);
//! - [`report`]: coupled length and gap share per pair (X08.5);
//! - [`measure`]: the float distances the checks and the report use;
//! - [`vias`]: coupled via pairs at a layer change (X09.1);
//! - [`skew`]: intra-pair skew compensation with X07's bumps (X09.4);
//! - [`optimizer`]: the set of pair items the batch optimizer leaves alone
//!   (X09.3).
//!
//! A pair whose P and N would cross is reported and not routed (X09.2,
//! [`legs::crossing`]).
//!
//! **Selection.** `router.special.diff_pairs = "krt-diffpair"` (or the id
//! in `router.special.stages`) adds the stage. It runs before the fanout
//! and the first routing pass ([`StagePhase::BeforeFanout`]: the fanout's
//! searches may complete a simple connection outright), so the pairs are
//! routed before every other net; the fanout and the passes then route the
//! rest (and whatever pair the stage left unrouted, single-ended).
//! Unselected, nothing here runs and sessions stay Freerouting's.
//!
//! **Inputs.** The stage the registry builds has no KiCad project: a DSN
//! carries no pair rule, and the CLI does not hand the KiCad reader's
//! project to a stage yet (C07). It then pairs only by names, and only
//! with `router.diff_pairs.infer_from_names = true`.
//! [`KrtDiffPair::with_classes`] takes K05.3's project classes.
//!
//! **Scope.** Two-pin nets; one layer (the lowest signal layer all four
//! pins reach), or, when the four pins share none, the start pins' layer
//! and the end pins' layer joined by coupled via pairs (X09.1). With
//! `router.diff_pairs.skew_um` (unset: 25 µm) the shorter track gets
//! meander bumps (X09.4). The coupled tracks and vias are user fixed, so
//! later passes and the optimizer leave them alone (X09.3); the SES marks
//! them `protect`.
//!
//! **Determinism** (D2): pairs in ascending P-net number, ends by the
//! lowest-index assignment, attempts in a fixed order; every search budget
//! counts work, never time.

use freerouting::autoroute::stages::{
    BoardStage, StageCtx, StagePhase, StageRegistry, StageReport,
};
use freerouting::board::RoutingBoard;
use freerouting::settings::RouterSettings;

pub mod centreline;
pub mod legs;
pub mod measure;
pub mod model;
pub mod offset;
pub mod optimizer;
pub mod report;
pub mod skew;
pub mod vias;

pub use legs::{PairOptions, PairOutcome, PairStatus, route_pair};
pub use model::{Pair, PairClasses, PairModel, build_pairs, name_key};

use crate::length::Units;

/// The stage id.
pub const DIFFPAIR_ID: &str = "krt-diffpair";

/// The `krt-diffpair` board stage ([module docs](self)).
#[derive(Clone, Debug, Default)]
pub struct KrtDiffPair {
    /// K05.3's project classes.
    pub classes: PairClasses,
    /// `router.diff_pairs.infer_from_names`.
    pub infer_from_names: bool,
    /// The router's tuning.
    pub options: PairOptions,
}

/// What the stage did.
#[derive(Clone, Debug, Default)]
pub struct DiffPairReport {
    /// The model's notes (ambiguities, partnerless class members).
    pub notes: Vec<String>,
    /// One outcome per pair, in ascending P-net number.
    pub pairs: Vec<PairOutcome>,
}

impl KrtDiffPair {
    /// The stage of `settings`, without a project.
    pub fn from_settings(settings: &RouterSettings) -> KrtDiffPair {
        KrtDiffPair {
            classes: PairClasses::none(),
            infer_from_names: settings.diff_pairs.infer_from_names.unwrap_or(false),
            options: PairOptions {
                skew_um: Some(settings.diff_pairs.skew_um.unwrap_or(skew::DEFAULT_SKEW_UM)),
                ..PairOptions::default()
            },
        }
    }

    /// The same stage with K05.3's classes.
    pub fn with_classes(mut self, classes: PairClasses) -> KrtDiffPair {
        self.classes = classes;
        self
    }

    /// Routes the pairs of `board`.
    pub fn run_on(&self, board: &mut RoutingBoard, ctx: &StageCtx) -> DiffPairReport {
        let model = build_pairs(board, &self.classes, self.infer_from_names);
        let mut report = DiffPairReport {
            notes: model.notes,
            pairs: Vec::new(),
        };
        for pair in &model.pairs {
            let outcome = route_pair(board, ctx, pair, &self.options);
            let stopped = matches!(outcome.status, PairStatus::Stopped);
            report.pairs.push(outcome);
            if stopped {
                break;
            }
        }
        report
    }
}

impl DiffPairReport {
    /// The log lines: the notes, then per pair its attempts, its status and,
    /// for a routed pair, its coupling on `board`.
    pub fn log(&self, board: &RoutingBoard) -> Vec<String> {
        let units = Units::of(board);
        let mm = |v: f64| units.to_um(v) / 1000.0;
        let mut out: Vec<String> = self
            .notes
            .iter()
            .map(|n| format!("{DIFFPAIR_ID}: {n}"))
            .collect();
        for o in &self.pairs {
            let name = |net: i32| {
                board
                    .rules
                    .nets
                    .get(net)
                    .map(|n| n.name.clone())
                    .unwrap_or_default()
            };
            let head = format!(
                "{DIFFPAIR_ID}: pair {} (P {} {}, N {} {}, class {})",
                o.pair.base,
                o.pair.p,
                name(o.pair.p),
                o.pair.n,
                name(o.pair.n),
                o.pair.class
            );
            for a in &o.attempts {
                out.push(format!("{head}: {a}"));
            }
            out.push(match &o.status {
                PairStatus::Routed {
                    setback_um,
                    layer,
                    layers,
                    vias,
                    skew,
                    centreline_length,
                    ..
                } => {
                    let c = report::coupling_on(board, &o.pair, layers);
                    let vias = match vias.len() {
                        0 => String::new(),
                        n => format!("; {n} coupled via pair(s)"),
                    };
                    let skew = skew.as_ref().map_or(String::new(), |s| match s.bumps {
                        0 => format!(
                            "; skew {:.4} mm left: no straight run of net {} near the \
                             mismatched end has room for a bump",
                            mm(s.before),
                            s.net
                        ),
                        n => format!(
                            "; skew {:.4} mm → {:.4} mm with {n} bump(s) on net {}",
                            mm(s.before),
                            mm(s.after),
                            s.net
                        ),
                    });
                    format!(
                        "{head}: routed (setback {} mm); coupled length {:.4} mm; {:.1} % of \
                         the pair's {:.4} mm of track within ±10 % of the gap {:.4} mm{vias}{skew}",
                        setback_um / 1000.0,
                        mm(*centreline_length),
                        100.0 * c.share(),
                        mm(c.copper_length),
                        mm(f64::from(o.pair.gap[*layer as usize]))
                    )
                }
                PairStatus::AlreadyRouted => format!("{head}: already connected; left alone"),
                PairStatus::Skipped(e) => format!("{head}: skipped: {e}"),
                PairStatus::Crossing(e) => format!(
                    "{head}: crossing: {e}; not routed by the stage (the fix is a pad swap in \
                     the schematic, which freerouting-rs doesn't touch)"
                ),
                PairStatus::Failed(e) => format!("{head}: not routed: {e}"),
                PairStatus::Stopped => format!("{head}: stopped"),
            });
        }
        out
    }
}

impl BoardStage for KrtDiffPair {
    fn id(&self) -> &'static str {
        DIFFPAIR_ID
    }

    fn phase(&self) -> StagePhase {
        StagePhase::BeforeFanout
    }

    fn run(&mut self, board: &mut RoutingBoard, ctx: &mut StageCtx) -> StageReport {
        let report = self.run_on(board, ctx);
        StageReport {
            log: report.log(board),
        }
    }
}

/// Registers `krt-diffpair`.
pub(crate) fn register(registry: &mut StageRegistry) {
    registry.board_stages.insert(DIFFPAIR_ID, |settings| {
        Box::new(KrtDiffPair::from_settings(settings))
    });
}
