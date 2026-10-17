//! Congestion-aware ripup (Q05,
//! `docs/roadmap/tasks/archive/Q05-congestion-aware-ripup.md`).
//!
//! Q05 instruments first and builds costs only if the instrument and Q06
//! show congestion. What exists so far:
//! - [`rip_stats`] (Q05.1): the `rip-stats` diagnostic
//!   (`router.special.diagnostics=rip-stats`), which records per net how
//!   often the pass loop ripped it, how much copper it lost and how often it
//!   was routed again. Report only: it changes no route.
//!
//! The history lattice, the `history-cost` policy and the `victim-cost`
//! policy (Q05.2–Q05.4) are not built: Q05's preregistered rule (more than
//! half of the unrouted connections `blocked_by` routed nets, or one net
//! above 25 % of the torn length, on at least three boards) did not trigger
//! them on the 2026-10-17 runs (Q05.1's note in the archived checklist).

pub mod rip_stats;
