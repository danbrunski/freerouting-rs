//! The Q series' stage registrations, called by
//! [`StageRegistry::builtin`](crate::autoroute::stages::StageRegistry::builtin).
//! Each checklist appends its own registration call to [`register`].

use crate::autoroute::stages::StageRegistry;

/// Registers the Q stages in `registry`.
pub fn register(registry: &mut StageRegistry) {
    // Q06: the `verdicts` diagnostic (`router.special.diagnostics`).
    registry
        .diagnostics
        .insert(super::verdict::DIAGNOSTIC_ID, |s| {
            Box::new(super::verdict::VerdictDiagnostics::from_settings(s))
        });
    // Q05.1: the `rip-stats` diagnostic (`router.special.diagnostics`).
    registry
        .diagnostics
        .insert(super::congestion::rip_stats::DIAGNOSTIC_ID, |s| {
            Box::new(super::congestion::rip_stats::RipStats::from_settings(s))
        });
    // Q02: the `widths` diagnostic (`router.special.diagnostics`, also
    // added by `router.neck_down.strict`).
    registry
        .diagnostics
        .insert(super::neckdown::DIAGNOSTIC_ID, |s| {
            Box::new(super::neckdown::WidthDiagnostics::from_settings(s))
        });
    // Q07: the pass-1 orderings (`router.autorouter.ordering`).
    registry
        .ordering
        .insert(super::ordering::conflict_rounds::ORDERING_ID, |_| {
            Box::new(super::ordering::conflict_rounds::ordering())
        });
    registry
        .ordering
        .insert(super::ordering::inside_out::ORDERING_ID, |_| {
            Box::new(super::ordering::inside_out::ordering())
        });
}
