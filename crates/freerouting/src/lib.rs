//! PCB autorouting engine: board model, design rules, DRC, autorouter and
//! optimizer.
//!
//! Port of the headless routing core of Freerouting (`board`, `rules`, `drc`,
//! `autoroute`, `core.scoring`). See `docs/PORT_PLAN.md` for the module map.

pub use freerouting_dsn as dsn;
pub use freerouting_geometry as geometry;
