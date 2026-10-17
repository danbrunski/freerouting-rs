//! The optimizer's gates (O06.1): the per-pass candidate veto, the pass
//! improvement and the preflight guards.
//!
//! Port of `BatchOptimizer.optimizerCandidateRejectionReason`,
//! `passImprovementPercent`, `evaluatePreFlightGuards` and
//! `areAllViasMandatoryLayerTransitions` (`BatchOptimizer.java:156-276`,
//! `:566-595` at `a773a70ec`).
//!
//! The guards read a [`GuardFigures`] rather than a `BoardStatistics`, so
//! Java's tests (which override `getOptimizerScore` and leave the bounds
//! null) port as plain values. The mandatory-via guard reads a list of
//! [`ViaContacts`], which [`via_contacts`] builds from a board in Java's
//! order (`getVias()`: highest id first).

use std::fmt;

use crate::board::basic_board::BasicBoard;
use crate::drc::json::java_format_fixed;
use crate::scoring::BoardStatistics;
use crate::settings::RouterSettings;

/// Why a pass result is rejected against the incumbent (Java's strings).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RejectionReason {
    /// More incomplete connections than the incumbent.
    ConnectivityRegression,
    /// More clearance violations than the incumbent.
    DrcCountRegression,
    /// The ranking score did not rise.
    OptimizerScoreNotImproved,
}

impl RejectionReason {
    /// Java's reason string.
    pub fn as_str(self) -> &'static str {
        match self {
            RejectionReason::ConnectivityRegression => "CONNECTIVITY_REGRESSION",
            RejectionReason::DrcCountRegression => "DRC_COUNT_REGRESSION",
            RejectionReason::OptimizerScoreNotImproved => "OPTIMIZER_SCORE_NOT_IMPROVED",
        }
    }
}

impl fmt::Display for RejectionReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The incomplete count, the clearance violations and the ranking score of
/// a board (the incumbent or a pass result).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GateFigures {
    /// Incomplete connections.
    pub incomplete: usize,
    /// Clearance violations.
    pub violations: usize,
    /// The optimizer ranking score.
    pub score: f32,
}

/// Java `optimizerCandidateRejectionReason`: completeness and the DRC count
/// veto, then the score must rise strictly. `None` accepts.
pub fn candidate_rejection_reason(
    incumbent: &GateFigures,
    candidate: &GateFigures,
) -> Option<RejectionReason> {
    if candidate.incomplete > incumbent.incomplete {
        return Some(RejectionReason::ConnectivityRegression);
    }
    if candidate.violations > incumbent.violations {
        return Some(RejectionReason::DrcCountRegression);
    }
    // PARITY: `!(candidate > incumbent)`, so NaN rejects.
    if candidate.score.partial_cmp(&incumbent.score) != Some(std::cmp::Ordering::Greater) {
        return Some(RejectionReason::OptimizerScoreNotImproved);
    }
    None
}

/// Java `passImprovementPercent`: `(after - before) / |before| × 100`, the
/// difference in `float`, the rest in `double`; 0 when `before` is 0.
pub fn pass_improvement_percent(before: f32, after: f32) -> f64 {
    if before == 0.0 {
        return 0.0;
    }
    f64::from(after - before) / f64::from(before).abs() * 100.0
}

/// What the preflight guards read of the board statistics. `None` is
/// Java's `null` (a `BoardStatistics()` built without a board).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GuardFigures {
    /// `connections.incompleteCount`.
    pub incomplete: usize,
    /// `vias.totalCount`.
    pub via_count: usize,
    /// `getOptimizerScore(routerSettings)` (the clamped score).
    pub optimizer_score: f32,
    /// `bounds.minTraceLengthMm`.
    pub min_trace_length_mm: Option<f32>,
    /// `traces.totalLengthMm`.
    pub total_length_mm: Option<f32>,
    /// `bounds.minViaCount`.
    pub min_via_count: Option<i32>,
}

impl GuardFigures {
    /// The figures of `stats` scored with `settings`.
    pub fn of(stats: &BoardStatistics, settings: &RouterSettings) -> GuardFigures {
        GuardFigures {
            incomplete: stats.connections.incomplete_count,
            via_count: stats.vias.total_count,
            optimizer_score: stats.optimizer_score(Some(settings)),
            min_trace_length_mm: Some(stats.bounds.min_trace_length_mm),
            total_length_mm: Some(stats.traces.total_length_mm),
            min_via_count: Some(stats.bounds.min_via_count),
        }
    }

    /// `bounds.minTraceLengthMm` when it is set and positive, with
    /// `traces.totalLengthMm`.
    fn lengths(&self) -> Option<(f32, f32)> {
        match (self.min_trace_length_mm, self.total_length_mm) {
            (Some(min), Some(total)) if min > 0.0 => Some((min, total)),
            _ => None,
        }
    }
}

/// Why the optimizer stage is skipped (Java's bypass reasons).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BypassReason {
    /// G1: the board has unrouted connections.
    Incomplete(usize),
    /// G2: no vias and the score is at least 950.
    NoViasHighScore(f32),
    /// G2: no vias and the length is within 5 % of the minimum.
    NoViasNearMinimumLength {
        /// `traces.totalLengthMm`.
        length: f32,
        /// `bounds.minTraceLengthMm`.
        minimum: f32,
    },
    /// G3: the score is at least 995.
    ScoreCeiling(f32),
    /// G3: the length is within 2 % of the minimum, with no excess vias.
    NearMinimumLength {
        /// `traces.totalLengthMm`.
        length: f32,
        /// `bounds.minTraceLengthMm`.
        minimum: f32,
    },
    /// G4: every unfixed via is a mandatory layer transition.
    MandatoryVias,
}

fn f2(value: f32) -> String {
    java_format_fixed(f64::from(value), 2)
}

impl fmt::Display for BypassReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            BypassReason::Incomplete(n) => write!(
                f,
                "the board has {n} unrouted connection(s) (optimizer only runs on completely \
                 routed boards)"
            ),
            BypassReason::NoViasHighScore(score) => write!(
                f,
                "the board has no vias to eliminate and initial optimizer score ({}) is already \
                 >= 950.00",
                f2(score)
            ),
            BypassReason::NoViasNearMinimumLength { length, minimum } => write!(
                f,
                "the board has no vias to eliminate and trace length ({} mm) is within 5% of \
                 theoretical minimum ({} mm)",
                f2(length),
                f2(minimum)
            ),
            BypassReason::ScoreCeiling(score) => write!(
                f,
                "the initial optimizer score ({}) is already at or near theoretical maximum \
                 (995.00)",
                f2(score)
            ),
            BypassReason::NearMinimumLength { length, minimum } => write!(
                f,
                "total trace length ({} mm) is already within 2% of the theoretical minimum ({} \
                 mm)",
                f2(length),
                f2(minimum)
            ),
            BypassReason::MandatoryVias => f.write_str(
                "all vias on the board are mandatory layer transitions between SMD pins that \
                 cannot be eliminated",
            ),
        }
    }
}

/// Java `evaluatePreFlightGuards`. `enabled` is
/// `optimizer.enablePreflightGuards`; `all_vias_mandatory` is evaluated
/// only when guards 1 to 3 pass (Java's G4 walks the board).
pub fn evaluate_preflight_guards(
    figures: &GuardFigures,
    enabled: bool,
    all_vias_mandatory: impl FnOnce() -> bool,
) -> Option<BypassReason> {
    if !enabled {
        return None;
    }
    // Guard 1: incomplete connections.
    if figures.incomplete > 0 {
        return Some(BypassReason::Incomplete(figures.incomplete));
    }
    let score = figures.optimizer_score;
    // Guard 2: no vias and an optimal length.
    if figures.via_count == 0 {
        if score >= 950.0 {
            return Some(BypassReason::NoViasHighScore(score));
        }
        if let Some((minimum, length)) = figures.lengths()
            && length <= minimum * 1.05f32
        {
            return Some(BypassReason::NoViasNearMinimumLength { length, minimum });
        }
    }
    // Guard 3: the score ceiling or the theoretical optimum.
    if score >= 995.0 {
        return Some(BypassReason::ScoreCeiling(score));
    }
    if let Some((minimum, length)) = figures.lengths()
        && length <= minimum * 1.02f32
        && figures.min_via_count.is_none_or(|min| {
            i64::try_from(figures.via_count).unwrap_or(i64::MAX) <= i64::from(min)
        })
    {
        return Some(BypassReason::NearMinimumLength { length, minimum });
    }
    // Guard 4: all vias are mandatory layer transitions.
    if all_vias_mandatory() {
        return Some(BypassReason::MandatoryVias);
    }
    None
}

/// An item of a via's connected set, as guard 4 sees it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContactKind {
    /// A pin with its first and last layer.
    Pin {
        /// `firstLayer()`.
        first_layer: i32,
        /// `lastLayer()`.
        last_layer: i32,
    },
    /// The via itself.
    SameVia,
    /// Another via.
    OtherVia,
    /// A trace.
    Trace,
    /// Anything else (a conduction area, ...).
    Other,
}

/// One via of the board for guard 4.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ViaContacts {
    /// `isUserFixed()`.
    pub user_fixed: bool,
    /// `netCount()`.
    pub net_count: usize,
    /// `getConnectedSet(firstNet, true)` in Java's iteration order.
    pub connected: Vec<ContactKind>,
}

/// The vias of `board` for guard 4, highest id first (Java `getVias()`).
/// The connected set is built only for unfixed vias with a net, the ones
/// Java reads it for.
pub fn via_contacts(board: &BasicBoard) -> Vec<ViaContacts> {
    board
        .vias()
        .into_iter()
        .filter_map(|id| {
            let via = board.item(id)?;
            let user_fixed = via.is_user_fixed();
            let net_count = via.net_count();
            let connected = if user_fixed || net_count == 0 {
                Vec::new()
            } else {
                board
                    .connected_set(id, via.net_number(0), true)
                    .into_iter()
                    .filter_map(|other| board.item(other).map(|item| (other, item)))
                    .map(|(other, item)| {
                        if item.is_pin() {
                            ContactKind::Pin {
                                first_layer: item.first_layer(),
                                last_layer: item.last_layer(),
                            }
                        } else if item.is_via() {
                            if other == id {
                                ContactKind::SameVia
                            } else {
                                ContactKind::OtherVia
                            }
                        } else if item.is_trace() {
                            ContactKind::Trace
                        } else {
                            ContactKind::Other
                        }
                    })
                    .collect()
            };
            Some(ViaContacts {
                user_fixed,
                net_count,
                connected,
            })
        })
        .collect()
}

/// Java `areAllViasMandatoryLayerTransitions`: false without vias; else
/// true when every unfixed via has a net and connects only traces and at
/// least two SMD pins, and those pins are not all on one layer.
pub fn all_vias_mandatory_layer_transitions(vias: &[ViaContacts]) -> bool {
    if vias.is_empty() {
        return false;
    }
    for via in vias {
        if via.user_fixed {
            continue;
        }
        if via.net_count == 0 {
            return false;
        }
        let mut smd_layers = Vec::new();
        for contact in &via.connected {
            match *contact {
                ContactKind::Pin {
                    first_layer,
                    last_layer,
                } if first_layer == last_layer => smd_layers.push(first_layer),
                ContactKind::SameVia | ContactKind::Trace => {}
                _ => return false,
            }
        }
        if smd_layers.len() < 2 {
            return false;
        }
        if smd_layers.iter().all(|layer| *layer == smd_layers[0]) {
            return false;
        }
    }
    true
}
