//! The width policy of the maze search (Q17.1, Q02.3's hook).
//!
//! Java's search reads one trace half width per layer from its
//! `AutorouteControl` (`trace_half_width`, `compensated_trace_half_width`)
//! wherever it decides whether a trace fits: the door checks
//! (`door_is_small`, `next_room_is_thick`, the door sections), the
//! destination and start neck-downs of `expand_to_room_doors`, the drill
//! and via checks, the ripup and shove rooms, the path locator and the
//! inserter. A [`WidthPolicy`] lets a caller give each room its own half
//! width instead. A session without one (the default, and every Java
//! parity caller) reads the control's value directly, with no call and no
//! arithmetic, so routing is unchanged; [`FreeroutingWidths`] is that rule
//! written as a policy (`tests/autoroute_width_policy.rs` routes with both
//! and compares the boards).
//!
//! **The compensated width** of a room is the policy's half width plus the
//! control's clearance compensation on the layer
//! (`compensated_trace_half_width - trace_half_width`), so a policy only
//! names trace widths.
//!
//! **Which room.** Each site passes the shape of the room it reads the
//! width for, as the shape is at that moment: an incomplete room (before
//! `complete_neighbours`) passes its incomplete shape, which contains the
//! room it completes to, and `None` when it is the whole plane. The
//! locator records the half width of the room each corner was placed in,
//! and the inserter lays each segment at the width of the room it ends in
//! ([`ResultItem::half_widths`](super::path::ResultItem::half_widths)).

use freerouting_geometry::TileShape;

use super::control::AutorouteControl;

/// The trace half width of the maze search in each room.
pub trait WidthPolicy {
    /// The trace half width, board units, of a trace in a room with shape
    /// `room` (`None`: an incomplete room on the whole plane) on layer
    /// index `layer`.
    fn half_width(&self, room: Option<&TileShape>, layer: usize) -> i32;
}

/// Freerouting's rule: the control's half width in every room. A session
/// without a policy behaves exactly like one with this.
#[derive(Clone, Copy, Debug)]
pub struct FreeroutingWidths<'a> {
    /// The control whose widths it returns.
    pub control: &'a AutorouteControl,
}

impl WidthPolicy for FreeroutingWidths<'_> {
    #[inline]
    fn half_width(&self, _room: Option<&TileShape>, layer: usize) -> i32 {
        self.control.trace_half_width[layer]
    }
}

/// The trace half width on `layer` of a room with shape `room`: the
/// policy's, else the control's.
#[inline]
pub(crate) fn trace_half_width(
    policy: Option<&dyn WidthPolicy>,
    control: &AutorouteControl,
    room: Option<&TileShape>,
    layer: usize,
) -> i32 {
    match policy {
        None => control.trace_half_width[layer],
        Some(p) => p.half_width(room, layer),
    }
}

/// The compensated trace half width on `layer` of a room with shape
/// `room`: the policy's half width plus the control's compensation, else
/// the control's compensated half width.
#[inline]
pub(crate) fn compensated_half_width(
    policy: Option<&dyn WidthPolicy>,
    control: &AutorouteControl,
    room: Option<&TileShape>,
    layer: usize,
) -> i32 {
    match policy {
        None => control.compensated_trace_half_width[layer],
        Some(p) => {
            let compensation =
                control.compensated_trace_half_width[layer] - control.trace_half_width[layer];
            p.half_width(room, layer) + compensation
        }
    }
}
