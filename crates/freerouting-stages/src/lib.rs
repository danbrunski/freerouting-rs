//! The full stage registry of the binaries (N05.1).
//!
//! `freerouting-krt` depends on the engine (`freerouting`), so the engine can
//! not register the KRT stages itself: a dependency back would be a cycle.
//! This crate sits above both and is the one place that knows which stage
//! crates exist. The CLI and the GUI depend on the engine and on this crate
//! and nothing else, so adding a stage crate (a new X-series family) changes
//! [`registry`] and no binary. See "Crate dependencies" in
//! `docs/development/engine.md`.
//!
//! Registering a stage selects nothing: a job runs the stages its settings
//! name, and with no KRT key set the registry gives the same session as
//! [`StageRegistry::builtin`] byte for byte.

use std::sync::Arc;

use freerouting::autoroute::stages::StageRegistry;

/// Freerouting's built-in stages plus every stage crate's.
pub fn registry() -> StageRegistry {
    let mut registry = StageRegistry::builtin();
    freerouting_krt::register(&mut registry);
    registry
}

/// [`registry`] behind an `Arc`, as `JobContext::registry` takes it.
pub fn shared_registry() -> Arc<StageRegistry> {
    Arc::new(registry())
}
