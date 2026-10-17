//! The optimizer exclusion (X09.3): Freerouting's batch optimizer (O06)
//! must not pull a routed pair apart.
//!
//! **How the optimizer is told.** Every trace and via the stage commits
//! for a pair is user fixed ([`super::legs`]). That state is the
//! exclusion set the optimizer already reads: a user-fixed trace is shove
//! fixed, so it is never a candidate (`optimize::batch::candidates`), a
//! user-fixed via is skipped likewise, and a user-fixed item is not
//! routable, so no other candidate's reroute rips or shoves it. The same
//! state keeps the routing passes' ripup off the pair. The checklist's
//! "stage-to-optimizer hint" is therefore the board's fixed state: no new
//! pipeline channel and no engine change (see the X09 report for the
//! alternative, a hint the pipeline passes, which only matters once the
//! pair tracks stop being user fixed).
//!
//! **What this module gives** is that set made explicit, for reports and
//! tests: [`exclusion`] lists the pair items the optimizer leaves alone,
//! and [`PairCopper`] is a pair's copper, compared before and after a
//! stage.
//!
//! Side effect, unchanged from X08: the SES writer marks the user-fixed
//! pair wires `protect` (KiCad imports them locked).

use std::collections::BTreeSet;

use freerouting::board::BasicBoard;
use freerouting::board::items::ItemId;

use super::model::Pair;

/// The user-fixed traces and vias of `pairs`' nets: what the batch
/// optimizer leaves alone ([module docs](self)), in id order.
pub fn exclusion(board: &BasicBoard, pairs: &[Pair]) -> BTreeSet<ItemId> {
    board
        .items()
        .filter(|i| (i.is_trace() || i.is_via()) && i.is_user_fixed())
        .filter(|i| {
            pairs
                .iter()
                .any(|p| i.contains_net(p.p) || i.contains_net(p.n))
        })
        .map(|i| i.id())
        .collect()
}

/// A trace of [`PairCopper`]: net, layer, half width, corners.
pub type TraceCopper = (i32, i32, i32, Vec<(f64, f64)>);

/// One pair's copper: its traces (layer, half width, corners) and vias
/// (centre, first and last layer), sorted, in exact board units (rational
/// corners as their float approximation).
#[derive(Clone, Debug, PartialEq, PartialOrd)]
pub struct PairCopper {
    pub traces: Vec<TraceCopper>,
    pub vias: Vec<(i32, i32, i32, (i32, i32))>,
}

impl PairCopper {
    /// The copper of `pair` on `board`.
    pub fn of(board: &BasicBoard, pair: &Pair) -> PairCopper {
        let mut traces = Vec::new();
        let mut vias = Vec::new();
        for item in board
            .items()
            .filter(|i| i.contains_net(pair.p) || i.contains_net(pair.n))
        {
            let net = item.net_number(0);
            if let Some(t) = item.as_trace() {
                let corners = t
                    .polyline
                    .corners()
                    .iter()
                    .map(|c| {
                        let f = c.to_float();
                        (f.x, f.y)
                    })
                    .collect();
                traces.push((net, t.layer, t.half_width, corners));
            } else if item.is_via()
                && let Some(d) = item.drill()
            {
                vias.push((net, d.first_layer, d.last_layer, (d.center.x, d.center.y)));
            }
        }
        traces.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        vias.sort_unstable();
        PairCopper { traces, vias }
    }
}
