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
}
