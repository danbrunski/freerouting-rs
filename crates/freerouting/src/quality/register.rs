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
    // Q17.2: the `bounded-neck` neck-down policy (`router.special.neck_down`).
    registry
        .neck_down
        .insert(super::neckdown::bounded_neck::POLICY_ID, |s| {
            Box::new(super::neckdown::bounded_neck::BoundedNeck::from_settings(s))
        });
    // Q04.1: the `quantum-stop` pass controller
    // (`router.autorouter.pass_controller`).
    registry
        .pass_controller
        .insert(super::stop_rule::CONTROLLER_ID, |s| {
            Box::new(super::stop_rule::QuantumStop::from_settings(s))
        });
    // Q11: the `transactional-ripup` connection router
    // (`router.autorouter.connection_router`).
    registry
        .connection_router
        .insert(super::transaction::ROUTER_ID, |s| {
            Box::new(super::transaction::TransactionalRipup::from_settings(s))
        });
    // Q23.3: the restarts from the best board (`router.autorouter.restarts_from_best`)
    // have no registry slot: like Q10's sweep they run from the batch loop's
    // finish (`quality::restart::after_loop`), with a new pass controller of
    // the job's kind from this registry.
}
