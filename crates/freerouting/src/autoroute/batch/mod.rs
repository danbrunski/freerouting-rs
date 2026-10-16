//! The batch pipeline's Freerouting stages: the decision logic of
//! `autoroute.pipeline` that needs no router (A10), and later the pass
//! runner, fanout and batch loop that drive it (A11-A13).
//!
//! - [`control`]: `FreeroutingPassController`, the stop and restore rules
//!   of `AutorouteBatchLoop.run`;
//! - [`ordering`]: `FreeroutingOrdering`, `getAutorouteItems` and the
//!   pass reordering with its seeded shuffle;
//! - [`ripup_policy`]: `FreeroutingRipupPolicy`, the ripup cost schedule,
//!   the strict-DRC rule and the repeated-failure ripup;
//! - [`fanout_rules`]: the fanout's component and pin order, retry and
//!   pass-loop stop rules (`BatchFanout`);
//! - [`report`]: the unrouted report, the last-mile corridor and blocker
//!   test, and the routable-layer check.
//!
//! Java line numbers are at `a773a70ec`.

pub mod control;
pub mod fanout_rules;
pub mod ordering;
pub mod report;
pub mod ripup_policy;

/// The minimum number of passes before the stagnation rules and the
/// periodic restore check apply (`BatchAutorouter.STOP_AT_PASS_MINIMUM`).
pub const STOP_AT_PASS_MINIMUM: u32 = 8;
/// The periodic restore check runs every this many passes
/// (`STOP_AT_PASS_MODULO`).
pub const STOP_AT_PASS_MODULO: u32 = 4;
/// Passes without a meaningful improvement before the router stops
/// (`STAGNATION_PASS_LIMIT`).
pub const STAGNATION_PASS_LIMIT: u32 = 10;
/// No-improvement passes before the one-time fanout recovery
/// (`FANOUT_RECOVERY_STAGNATION_PASSES`).
pub const FANOUT_RECOVERY_STAGNATION_PASSES: u32 = 3;
/// The most incomplete connections the last-mile ripup handles
/// (`LAST_MILE_INCOMPLETE_LIMIT`).
pub const LAST_MILE_INCOMPLETE_LIMIT: usize = 8;
/// No-improvement passes before a last-mile ripup
/// (`LAST_MILE_STAGNATION_PASSES`).
pub const LAST_MILE_STAGNATION_PASSES: u32 = 3;
/// Last-mile ripups per run (`LAST_MILE_MAX_ATTEMPTS`).
pub const LAST_MILE_MAX_ATTEMPTS: u32 = 2;
/// The smallest score gain that counts as an improvement, added in `f32`
/// (`STAGNATION_SCORE_THRESHOLD = 0.5F`).
pub const STAGNATION_SCORE_THRESHOLD: f32 = 0.5;
/// Restores per history board (`MAXIMUM_TRIES_ON_THE_SAME_BOARD`).
pub const MAXIMUM_TRIES_ON_THE_SAME_BOARD: i32 = 3;
/// The lowest rank a restored board may have (`BOARD_RANK_LIMIT =
/// BoardHistory.MAX_HISTORY_SIZE`).
pub const BOARD_RANK_LIMIT: usize = crate::autoroute::history::MAX_HISTORY_SIZE;
