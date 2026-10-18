//! Q13: `router.power_layer_traces`, where a plane net's traces may lie on a
//! power layer.
//!
//! Java `7f56b062d` (Q01.7, after the pin) keeps every net off a power
//! layer except the nets with a plane (conduction area) there
//! ([`AutorouteControl::own_plane_layers`]): the maze search enters a room
//! on that layer only for those nets. The flag is per layer, not per
//! area, so a net whose plane is a small island may still run a trace
//! anywhere on the layer: from a through-hole pin outside the island to the
//! island's border, or across another net's pour. KiCad then refuses the
//! board or the pour slices around the trace (canary-pwr, 2026-10-17:
//! `+5V_PI` on `Power.Cu` from (106.66, 56.44) to the island's border at
//! (107.89, 54.99); the island is x 107.3–112.2, y 45.2–55.0).
//!
//! The setting chooses:
//!
//! - `layer` (the default, also when unset): Java's rule, a net with a
//!   plane on a power layer may route anywhere on that layer; profile
//!   `legacy`, and routes are byte-identical to Java's;
//! - `own_plane`: a trace of net N on a power layer may only lie inside the
//!   outline of one of N's own conduction areas on that layer. A deliberate
//!   divergence (`docs/DIVERGENCES.md`, "Autorouter");
//! - `none` (N05): no trace of any net on a power layer, as canary's
//!   validity check (`inner_tracks.py`) wants; a via still lands in its
//!   net's plane there. Under it a hop on a power layer is expanded only
//!   when it has no length (the entry segment and the connection point are
//!   one board point) and that point lies in one of N's planes there, which
//!   is a via landing in the plane: `own_plane`'s check with the extra
//!   condition that the hop lays no trace. A deliberate divergence
//!   (`docs/DIVERGENCES.md`, "Autorouter").
//!
//! **Where the rule acts.** Power layers are never active, so on a power
//! layer the maze search only expands from the room it entered to the
//! room's target doors (`expandToRoomDoors` returns after
//! `expandToTargetDoors`): every trace it can lay there is one straight
//! hop from the entry (a start item's door or a drill) to the nearest point
//! of a target item. Under `own_plane` such a hop is expanded only if the
//! entry segment and the connection point all lie in one convex piece of
//! one of N's conduction areas on that layer, in the board's own geometry
//! (a via landing in the plane is a hop whose points are all its centre).
//! The piece is convex, so the trace's centre line then lies inside the
//! area. A hop without length gets no exception: the search's target
//! shapes are enlarged by the clearance compensation, so a via whose
//! centre is just outside the plane meets it with no length, and the
//! path's insertion then lays a trace from the via into the plane. The room shapes are unchanged
//! (no clipping, no new obstacles), so the search's room geometry stays
//! Java's. A via through the layer is still allowed, as before. The trace
//! width is not checked: a centre line inside the outline may overhang it
//! by half a trace width at the border.
//!
//! [`AutorouteControl::own_plane_layers`]: crate::autoroute::control::AutorouteControl::own_plane_layers

use crate::board::BasicBoard;
use crate::board::items::ItemId;
use crate::geometry::Point;
use crate::settings::RouterSettings;
use freerouting_geometry::TileShapeOps;

/// The settings key, as the CLI and the JSON file name it.
pub const SETTINGS_KEY: &str = "router.power_layer_traces";

/// Where a plane net's traces may lie on a power layer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PowerLayerTraces {
    /// Java's rule: anywhere on a power layer where the net has a plane.
    #[default]
    Layer,
    /// Only inside the outline of one of the net's own planes there.
    OwnPlane,
    /// No trace on a power layer at all; vias still land in the net's
    /// own planes (`none`, N05).
    NoTraces,
}

impl PowerLayerTraces {
    /// The mode of a setting value; unset or empty is
    /// [`PowerLayerTraces::Layer`]. Fails on any other value, naming it.
    pub fn parse(value: Option<&str>) -> Result<Self, String> {
        match value.map(str::trim) {
            None | Some("") | Some("layer") => Ok(PowerLayerTraces::Layer),
            Some("own_plane") => Ok(PowerLayerTraces::OwnPlane),
            Some("none") => Ok(PowerLayerTraces::NoTraces),
            Some(other) => Err(format!(
                "{SETTINGS_KEY}: unknown value '{other}' (expected 'layer', 'own_plane' or 'none')"
            )),
        }
    }

    /// The mode `settings` ask for; an unknown value reads as the default
    /// ([`validate`] reports it).
    pub fn of(settings: &RouterSettings) -> Self {
        Self::parse(settings.power_layer_traces.as_deref()).unwrap_or_default()
    }
}

impl PowerLayerTraces {
    /// The setting's value (`layer`, `own_plane`, `none`).
    pub fn name(self) -> &'static str {
        match self {
            PowerLayerTraces::Layer => "layer",
            PowerLayerTraces::OwnPlane => "own_plane",
            PowerLayerTraces::NoTraces => "none",
        }
    }

    /// True if the maze search needs the net's planes per power layer
    /// ([`own_plane_areas`]) to judge a hop there.
    pub fn needs_plane_areas(self) -> bool {
        self != PowerLayerTraces::Layer
    }
}

/// Whether the maze search may expand a hop on a power layer whose entry
/// segment and connection point are `points`, for a net whose planes on
/// that layer are `areas`. Always true under `layer`; under `own_plane` the
/// points must lie in one convex piece of one of `areas`
/// ([`points_in_one_piece`]); under `none` they must also be one point, so
/// the hop lays no trace (a via landing in the plane).
pub fn hop_allowed(
    mode: PowerLayerTraces,
    board: &BasicBoard,
    areas: &[ItemId],
    points: &[Point],
) -> bool {
    match mode {
        PowerLayerTraces::Layer => true,
        PowerLayerTraces::OwnPlane => points_in_one_piece(board, areas, points),
        PowerLayerTraces::NoTraces => {
            points.windows(2).all(|w| w[0] == w[1]) && points_in_one_piece(board, areas, points)
        }
    }
}

/// Checks the setting's value (the job's load sequence calls it).
pub fn validate(settings: &RouterSettings) -> Result<PowerLayerTraces, String> {
    PowerLayerTraces::parse(settings.power_layer_traces.as_deref())
}

/// Per layer, the conduction areas of net `net_number` on that layer if it
/// is a power layer (in item order); empty on signal layers.
pub fn own_plane_areas(board: &BasicBoard, net_number: i32) -> Vec<Vec<ItemId>> {
    let mut own = vec![Vec::new(); board.layer_count()];
    for id in board.conduction_areas() {
        let Some(area) = board.item(id) else {
            continue;
        };
        let index = area.first_layer() as usize;
        if !board.layer_structure.layers[index].is_signal && area.contains_net(net_number) {
            own[index].push(id);
        }
    }
    own
}

/// True if all `points` lie in one convex piece of one of `areas` (border
/// included), so the straight lines between them lie inside that area.
pub fn points_in_one_piece(board: &BasicBoard, areas: &[ItemId], points: &[Point]) -> bool {
    areas.iter().any(|&id| {
        let Some(area) = board.item(id) else {
            return false;
        };
        (0..board.tile_shape_count(area)).any(|k| {
            board
                .tile_shape(area, k)
                .is_some_and(|piece| points.iter().all(|p| piece.contains(p)))
        })
    })
}

/// Where a trace on a power layer lies relative to its net's planes there
/// (the check of Q13.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PowerLayerTrace {
    /// Its net has no plane on that layer (Q01.7's breach).
    Foreign,
    /// Its net has a plane there, but the trace leaves it (Q13's breach).
    OutsideOwnPlane,
}

/// The traces of `board` on power layers that break Q01.7's rule
/// ([`PowerLayerTrace::Foreign`]) or Q13's ([`PowerLayerTrace::OutsideOwnPlane`]),
/// in item order. A trace is inside its own plane when its corners and the
/// middles of its segments all lie in one of its net's conduction areas on
/// its layer (border included). That samples the segments, so a segment
/// that leaves a non-convex area between its ends and its middle is not
/// seen; it finds every trace with an end or a middle outside the plane,
/// which is every case Java's rule produces on canary-pwr.
pub fn power_layer_breaches(board: &BasicBoard) -> Vec<(ItemId, PowerLayerTrace)> {
    let planes: Vec<ItemId> = board.conduction_areas();
    let mut out = Vec::new();
    for item in board.items() {
        let Some(trace) = item.as_trace() else {
            continue;
        };
        let layer = trace.layer;
        if board.layer_structure.layers[layer as usize].is_signal {
            continue;
        }
        let nets = item.net_numbers();
        let own: Vec<&crate::geometry::Area> = planes
            .iter()
            .filter_map(|&id| board.item(id))
            .filter(|plane| {
                plane.first_layer() == layer && nets.iter().any(|n| plane.contains_net(*n))
            })
            .filter_map(|plane| plane.as_conduction_area().map(|a| a.area()))
            .collect();
        if own.is_empty() {
            out.push((item.id(), PowerLayerTrace::Foreign));
            continue;
        }
        let corners = trace.polyline.corner_approx_arr();
        let mut points = corners.clone();
        points.extend(corners.windows(2).map(|w| w[0].middle_point(w[1])));
        let inside = own
            .iter()
            .any(|area| points.iter().all(|p| area.contains_float(*p)));
        if !inside {
            out.push((item.id(), PowerLayerTrace::OutsideOwnPlane));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_accepts_the_three_values_and_refuses_others() {
        assert_eq!(PowerLayerTraces::parse(None), Ok(PowerLayerTraces::Layer));
        assert_eq!(
            PowerLayerTraces::parse(Some("")),
            Ok(PowerLayerTraces::Layer)
        );
        assert_eq!(
            PowerLayerTraces::parse(Some("layer")),
            Ok(PowerLayerTraces::Layer)
        );
        assert_eq!(
            PowerLayerTraces::parse(Some(" own_plane ")),
            Ok(PowerLayerTraces::OwnPlane)
        );
        assert_eq!(
            PowerLayerTraces::parse(Some("none")),
            Ok(PowerLayerTraces::NoTraces)
        );
        for mode in [
            PowerLayerTraces::Layer,
            PowerLayerTraces::OwnPlane,
            PowerLayerTraces::NoTraces,
        ] {
            assert_eq!(PowerLayerTraces::parse(Some(mode.name())), Ok(mode));
        }
        let err = PowerLayerTraces::parse(Some("island")).unwrap_err();
        assert!(
            err.contains(SETTINGS_KEY) && err.contains("'island'"),
            "{err}"
        );
    }
}
