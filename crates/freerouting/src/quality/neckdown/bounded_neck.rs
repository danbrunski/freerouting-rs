//! Q17.2 (Q02.3's remainder): the `bounded-neck` neck-down stage.
//!
//! Like Java's necked retry (`FreeroutingNeckDown`, `retryConnectionNecked`)
//! it runs only after the class-width attempt of a connection failed
//! (FAILED or INSERT_ERROR), at the same point and with the same ripup
//! cost: a new control for the net on the board as it is now, the
//! attempt's start and destination sets, its work limit and ripped set.
//! Where Java clamps every trace to `neck_width_um`, it reruns the maze
//! search with a [`WidthPolicy`]: the neck width in a room within D of a
//! pin of the connection, the class width elsewhere.
//!
//! **Neck zones.** The pins of the start set, then those of the
//! destination set (each in ascending item id), each give a zone: the
//! pin's box enlarged by D ([`bounded::room_within`]). D is
//! [`bounded::neck_distance`] of the pin's box, its component's courtyard
//! (the bounding box of the component's courtyard outlines, else of all its
//! outlines; none without one) and its pitch among the component's pins
//! ([`bounded::pitch`]); with `router.neck_down.path_mm` it is that length
//! instead (KRT's 2.5 mm arm). The zone's neck half width on a layer is
//! half of [`bounded::neck_width`] of the pin's neck-down half width
//! (floored by `router.min_trace_width_um`, as Java's room-local neck-down),
//! the narrowest class width on the layer and the board minimum, rounded up,
//! and never above the class half width. A room meeting several zones gets
//! the narrowest of their necks.
//!
//! **Insertion and the widening beyond D.** The locator places each corner
//! in its room's width and the inserter lays each segment at that width
//! (Q17.1). A room is near when its box meets a zone, and a free room next
//! to a pin field can reach far beyond the zone, so after the insert the
//! stage widens the pieces of the new narrow traces that lie outside every
//! zone back to the class width where the exact clearance check lets it
//! ([`widen_back::widen_traces`] with the zones), before the pull-tight.
//! With `router.neck_down.widen_back` the full widen-back (every piece,
//! tapers, the island rule) runs instead. This widening beyond D is the
//! one deviation from Q02.3's design, which inserted at the room's width
//! only.
//!
//! `router.neck_down.strict` turns it off, as it turns Java's retry off.

use std::collections::BTreeSet;

use freerouting_geometry::{IntBox, IntPoint, PolylineShapeOps, TileShape};

use super::bounded;
use super::widen_back::{self, WidenOptions};
use crate::autoroute::AttemptState;
use crate::autoroute::batch::connection::{
    attempt_with_widths, pull_tight_after_connection, router_control,
};
use crate::autoroute::control::AutorouteControl;
use crate::autoroute::stages::{
    ConnectionOutcome, ConnectionParams, NeckDownAttempt, NeckDownPolicy, RouteTask, StageCtx,
};
use crate::autoroute::width_policy::WidthPolicy;
use crate::board::RoutingBoard;
use crate::board::items::ItemId;
use crate::datastructures::IdGenerator;
use crate::settings::RouterSettings;

/// The registry id.
pub const POLICY_ID: &str = "bounded-neck";

/// The neck zone of one pin.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NeckZone {
    /// The pin.
    pub pin: ItemId,
    /// The pin's bounding box.
    pub pin_box: IntBox,
    /// The distance D, board units.
    pub distance: i32,
    /// The neck half width on each layer.
    pub neck_half_width: Vec<i32>,
}

impl NeckZone {
    /// The zone as a box `[x0, y0, x1, y1]` (the pin's box enlarged by D).
    pub fn area(&self) -> [i64; 4] {
        let d = i64::from(self.distance);
        [
            i64::from(self.pin_box.ll.x) - d,
            i64::from(self.pin_box.ll.y) - d,
            i64::from(self.pin_box.ur.x) + d,
            i64::from(self.pin_box.ur.y) + d,
        ]
    }
}

/// The `bounded-neck` width policy of one connection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BoundedWidths {
    /// The class half width on each layer (the control's).
    pub class_half_width: Vec<i32>,
    /// The zones, start-set pins first.
    pub zones: Vec<NeckZone>,
}

impl WidthPolicy for BoundedWidths {
    fn half_width(&self, room: Option<&TileShape>, layer: usize) -> i32 {
        let class = self.class_half_width[layer];
        let room_box = room.map(|r| r.bounding_box());
        self.zones
            .iter()
            .filter(|z| {
                room_box
                    .as_ref()
                    .is_none_or(|b| bounded::room_within(b, &z.pin_box, z.distance))
            })
            .map(|z| z.neck_half_width[layer])
            .fold(class, i32::min)
    }
}

impl BoundedWidths {
    /// The policy of a connection from `start` to `dest` with `control`
    /// (its class widths), `path` the fixed D in board units if set.
    pub fn new(
        board: &RoutingBoard,
        control: &AutorouteControl,
        start: &BTreeSet<ItemId>,
        dest: &BTreeSet<ItemId>,
        path: Option<i32>,
    ) -> BoundedWidths {
        let class_min = class_min_widths(board, control.layer_count);
        let board_min = control.min_trace_half_width.saturating_mul(2);
        let zones = start
            .iter()
            .chain(dest.iter())
            .filter(|id| board.item(**id).is_some_and(|i| i.is_pin()))
            .filter_map(|&pin| zone(board, control, pin, path, &class_min, board_min))
            .collect();
        BoundedWidths {
            class_half_width: control.trace_half_width.clone(),
            zones,
        }
    }

    /// True if some zone necks an active layer below its class width.
    pub fn narrows_somewhere(&self, control: &AutorouteControl) -> bool {
        self.zones.iter().any(|z| {
            (0..control.layer_count)
                .any(|l| control.layer_active[l] && z.neck_half_width[l] < self.class_half_width[l])
        })
    }
}

/// The narrowest class width on each layer over the board's nets.
fn class_min_widths(board: &RoutingBoard, layers: usize) -> Vec<i32> {
    let nets: Vec<i32> = (1..=board.rules.nets.max_net_number())
        .filter(|n| board.rules.nets.get(*n).is_some())
        .collect();
    (0..layers)
        .map(|l| {
            nets.iter()
                .map(|n| board.rules.trace_half_width(*n, l as i32).saturating_mul(2))
                .min()
                .unwrap_or(0)
        })
        .collect()
}

/// The courtyard box of component `component`: its courtyard outlines,
/// else all its outlines; `None` without one.
///
/// An outline whose box is not bounded (K15.1: a placed shape whose rounded
/// corners are not convex, `router.java_quirks.placed_shape_convex` unset) is
/// left out, as the board box leaves it out; the job's warning from
/// [`expand_bounding_box_to_include_all_items`](crate::board::BasicBoard::expand_bounding_box_to_include_all_items)
/// already names it. Without the guard its box would make the neck distance
/// `i32::MAX`, so the zone would cover the board.
fn courtyard(board: &RoutingBoard, component: i32) -> Option<IntBox> {
    let outlines: Vec<(bool, IntBox)> = board
        .items()
        .filter(|i| i.component_id() == component)
        .filter_map(|i| {
            i.as_component_outline()
                .map(|o| (o.is_courtyard, i.bounding_box()))
        })
        .filter(|(_, b)| !b.has_infinite_coordinate())
        .collect();
    let any_courtyard = outlines.iter().any(|(c, _)| *c);
    outlines
        .into_iter()
        .filter(|(c, _)| *c || !any_courtyard)
        .map(|(_, b)| b)
        .reduce(|a, b| a.union_box(&b))
}

/// The zone of `pin`, `None` for a pin without a drill.
fn zone(
    board: &RoutingBoard,
    control: &AutorouteControl,
    pin: ItemId,
    path: Option<i32>,
    class_min: &[i32],
    board_min: i32,
) -> Option<NeckZone> {
    let item = board.item(pin)?;
    let center = item.drill()?.center;
    let pin_box = item.bounding_box();
    let component = item.component_id();
    let distance = match path {
        Some(d) => d,
        None => {
            let (courtyard, others) = if component > 0 {
                let others: Vec<IntPoint> = board
                    .component_pins(component)
                    .into_iter()
                    .filter_map(|p| board.item(p).and_then(|i| i.drill()).map(|d| d.center))
                    .collect();
                (courtyard(board, component), others)
            } else {
                (None, Vec::new())
            };
            bounded::neck_distance(
                &pin_box,
                courtyard.as_ref(),
                bounded::pitch(center, &others),
            )
        }
    };
    let neck_half_width = (0..control.layer_count)
        .map(|l| {
            let pin_neck = control.neckdown_half_width(board, pin, l as i32);
            let width = bounded::neck_width(pin_neck, class_min[l], board_min);
            let half = i32::try_from((i64::from(width) + 1) / 2).unwrap_or(i32::MAX);
            half.min(control.trace_half_width[l])
        })
        .collect();
    Some(NeckZone {
        pin,
        pin_box,
        distance,
        neck_half_width,
    })
}

/// The `bounded-neck` stage.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BoundedNeck {
    /// `router.neck_down.path_mm`.
    pub path_mm: Option<f64>,
    /// `router.neck_down.widen_back`.
    pub widen_back: bool,
    /// `router.neck_down.strict`.
    pub strict: bool,
}

impl BoundedNeck {
    /// The stage with the settings of a job.
    pub fn from_settings(settings: &RouterSettings) -> BoundedNeck {
        BoundedNeck {
            path_mm: settings
                .neck_down
                .path_mm
                .filter(|d| d.is_finite() && *d >= 0.0),
            widen_back: widen_back_on(settings),
            strict: super::ledger::is_strict(settings),
        }
    }
}

/// `router.neck_down.widen_back`.
pub fn widen_back_on(settings: &RouterSettings) -> bool {
    settings.neck_down.widen_back == Some(true)
}

/// The traces of net `net` with an id from `first_new` on that are
/// narrower than `control`'s class width on their layer, ascending id.
pub fn new_narrow_traces(
    board: &RoutingBoard,
    net: i32,
    first_new: i32,
    control: &AutorouteControl,
) -> Vec<ItemId> {
    let mut ids: Vec<ItemId> = board
        .items()
        .filter(|i| i.id().0 >= first_new && i.contains_net(net))
        .filter(|i| {
            i.as_trace().is_some_and(|t| {
                usize::try_from(t.layer)
                    .ok()
                    .and_then(|l| control.trace_half_width.get(l))
                    .is_some_and(|w| t.half_width < *w)
            })
        })
        .map(|i| i.id())
        .collect();
    ids.sort_unstable();
    ids
}

/// The widening after a neck-down retry routed `net` with `control`: the
/// new narrow traces from `first_new` on, widened outside `zones` (or
/// everywhere, with tapers, when `taper`).
pub fn widen_after_retry(
    board: &mut RoutingBoard,
    net: i32,
    first_new: i32,
    control: &AutorouteControl,
    zones: Vec<[i64; 4]>,
    taper: bool,
) -> widen_back::WidenReport {
    let ids = new_narrow_traces(board, net, first_new, control);
    let options = WidenOptions::on_board(board, taper, if taper { Vec::new() } else { zones });
    let class = control.trace_half_width.clone();
    let target = move |layer: i32| {
        usize::try_from(layer)
            .ok()
            .and_then(|l| class.get(l).copied())
            .unwrap_or(0)
    };
    widen_back::widen_traces(board, &ids, &target, &options)
}

impl NeckDownPolicy for BoundedNeck {
    fn id(&self) -> &'static str {
        POLICY_ID
    }

    fn retry(
        &mut self,
        board: &mut RoutingBoard,
        task: &RouteTask,
        attempt_in: NeckDownAttempt<'_>,
        _failed: &ConnectionOutcome,
        p: &ConnectionParams,
        ctx: &mut StageCtx,
    ) -> Option<ConnectionOutcome> {
        if self.strict {
            return None;
        }
        // As `FreeroutingNeckDown`: a new control on the board as it is now.
        let control = router_control(board, task, p, ctx)?;
        let path = self.path_mm.map(|mm| {
            crate::geometry::jmath::round_to_i32(widen_back::um_to_units(board, mm * 1000.0))
        });
        let policy = BoundedWidths::new(board, &control, attempt_in.start, attempt_in.dest, path);
        if !policy.narrows_somewhere(&control) {
            return None;
        }
        let first_new = board
            .communication
            .id_generator
            .max_generated_id()
            .wrapping_add(1);
        let result = attempt_with_widths(
            board,
            attempt_in.start,
            attempt_in.dest,
            &control,
            attempt_in.work_limit,
            ctx,
            attempt_in.ripped,
            attempt_in.ripped_costs,
            Some(&policy),
        );
        if result.state != AttemptState::Routed {
            return None;
        }
        let zones = policy.zones.iter().map(NeckZone::area).collect();
        widen_after_retry(board, task.net, first_new, &control, zones, self.widen_back);
        pull_tight_after_connection(board, p.pull_tight_accuracy, &control.trace_costs);
        Some(ConnectionOutcome::new(result.state, result.details))
    }
}

#[cfg(test)]
mod tests {
    use super::courtyard;
    use crate::board::RoutingBoard;
    use crate::board::items::ItemId;
    use crate::io::specctra::{BoardReadResult, read_board_from_str};

    /// Bug 1's repro (KiCad 10.0.7's DSN export, `tests/data/k15/`): C65's
    /// F.Fab circle places to an unbounded simplex (K15.1).
    fn c65_board() -> (RoutingBoard, i32) {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/data/k15/c65-nm-position.kicad10.dsn"
        );
        let text = std::fs::read_to_string(path).expect("fixture");
        let BoardReadResult::Success { board, .. } = read_board_from_str(&text) else {
            panic!("the repro reads");
        };
        let c65 = board.components.get_by_name("C65").expect("C65").id.0;
        (*board, c65)
    }

    fn outlines(board: &RoutingBoard, component: i32) -> Vec<(ItemId, bool, bool)> {
        board
            .items()
            .filter(|i| i.component_id() == component)
            .filter_map(|i| {
                i.as_component_outline().map(|o| {
                    (
                        i.id(),
                        o.is_courtyard,
                        i.bounding_box().has_infinite_coordinate(),
                    )
                })
            })
            .collect()
    }

    /// The fallback to all outlines (a component without a courtyard) leaves
    /// the unbounded outline out.
    #[test]
    fn the_courtyard_fallback_leaves_an_unbounded_outline_out() {
        let (mut board, c65) = c65_board();
        let all = outlines(&board, c65);
        let unbounded: Vec<ItemId> = all.iter().filter(|o| o.2).map(|o| o.0).collect();
        assert_eq!(unbounded.len(), 1, "{all:?}");
        // With its courtyard C65 never reaches the fallback.
        let with_courtyard = courtyard(&board, c65).expect("a courtyard");
        assert!(!with_courtyard.has_infinite_coordinate());
        // Keep the unbounded outline and one bounded outline that is not the
        // courtyard: the fallback takes the bounded one's box only.
        let kept = all
            .iter()
            .find(|o| !o.1 && !o.2)
            .map(|o| o.0)
            .expect("a bounded outline");
        for (id, _, _) in &all {
            if *id != unbounded[0] && *id != kept {
                assert!(board.remove_item_checked(*id, true), "{id:?}");
            }
        }
        assert_eq!(outlines(&board, c65).len(), 2);
        let kept_box = board.item(kept).expect("kept").bounding_box();
        assert_eq!(courtyard(&board, c65), Some(kept_box));
        // Only the unbounded outline left: no courtyard box at all.
        assert!(board.remove_item_checked(kept, true));
        assert_eq!(courtyard(&board, c65), None);
    }
}
