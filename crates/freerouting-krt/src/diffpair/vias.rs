//! Coupled via pairs (X09.1): a pair whose ends sit on different layers
//! changes layer through two vias side by side.
//!
//! **The via.** The net's via rule (P's net class), its first via whose
//! padstack spans both layers; its radius is the padstack's largest half
//! width on those layers, its clearance class the via info's.
//!
//! **Spacing.** The via centres are `s` apart, perpendicular to the
//! centreline at the layer change, with
//! `s = max(D, 2·r + c_vv, 2·(r + hw + c_tv) − D)`: `D = 2·hw + g` the
//! P–N centre distance (the checklist's `w + g`), `r` the via radius,
//! `c_vv` the via-to-via clearance and `c_tv` the clearance between the
//! pair's tracks and the via (the third term keeps a via clear of the
//! partner's track running into the other via; the checklist names only
//! the first two). The centres are integer: each is moved from the
//! centreline point by the same integer vector, the rounded normal
//! lengthened in half units until it is at least `s/2` long, so the
//! centres are never closer than `s`.
//!
//! **The search** ([`super::centreline::route_sections`]) treats the layer
//! change as one via of compound radius `ceil(s/2) + r + 1` (half the
//! spacing plus the via radius, plus one unit for the rounding) with the
//! via's clearance class, so the room it keeps around the via point holds
//! both barrels and their clearance to every foreign item. Its padstack
//! and via info exist only on the search's scratch board.
//!
//! **The normal** at a layer change is the left normal of the bisector of
//! the incoming and the outgoing centreline directions (perpendicular to
//! the centreline when it runs straight through); P's via is on P's side.
//! Each track runs from its offset end straight to its via (KRT's approach
//! and exit tracks), on both layers.
//!
//! **D8.** A via whose centre lies in a pad of any net on a layer the via
//! spans is refused ([`via_in_pad`]); the attempt then fails.
//!
//! GND return vias (`router.diff_pairs.return_vias`) are not placed: the
//! checklist makes them optional and off by default, and no fixture or
//! measurement asks for them (see the X09 report).
//!
//! The idea of collinear via pairs with approach and exit tracks is
//! KiCadRoutingTools' (`rust_router/src/pose_router.rs`
//! `diff_pair_via_spacing`, `docs/differential-pairs.md` at `0486e799`);
//! the spacing rule, the compound via and the integer construction are
//! written here.

use freerouting::autoroute::control::AutorouteControl;
use freerouting::board::BasicBoard;
use freerouting::board::items::ItemId;
use freerouting::geometry::{IntPoint, TileShapeOps};
use freerouting::library::PadstackId;
use freerouting::settings::board::BoardRouterSettings;

use super::centreline::{CompoundVia, net_clearance_class};
use super::model::Pair;

/// The via of a layer-changing pair and its spacing ([module docs](self)).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ViaSpec {
    /// The real via's padstack.
    pub padstack: PadstackId,
    /// Its clearance class.
    pub clearance_class: i32,
    /// Its radius, board units.
    pub radius: i32,
    /// The distance between the two via centres, board units.
    pub spacing: i32,
    /// The padstack's layers.
    pub from_layer: i32,
    pub to_layer: i32,
}

impl ViaSpec {
    /// The compound via the centreline search uses ([module docs](self)).
    pub fn compound(&self) -> CompoundVia {
        CompoundVia {
            radius: (self.spacing - self.spacing / 2) + self.radius + 1,
            from_layer: self.from_layer,
            to_layer: self.to_layer,
            clearance_class: self.clearance_class,
        }
    }
}

/// The via of `pair` between layers `a` and `b` ([module docs](self)).
pub fn via_spec(
    board: &BasicBoard,
    pair: &Pair,
    a: i32,
    b: i32,
    settings: &BoardRouterSettings,
) -> Result<ViaSpec, String> {
    let control = AutorouteControl::for_net_default(board, pair.p, settings)
        .map_err(|e| format!("via rule: {e}"))?;
    let (lo, hi) = (a.min(b), a.max(b));
    let rules = &board.rules;
    let found = control.via_rule_vias(rules).iter().find_map(|id| {
        let info = rules.via_infos.get(*id);
        let padstack = board.library.padstacks.get(info.padstack)?;
        (padstack.from_layer() <= lo && padstack.to_layer() >= hi).then_some((info, padstack))
    });
    let (info, padstack) = found
        .ok_or_else(|| format!("the pair's via rule has no via from layer {lo} to layer {hi}"))?;
    let (from_layer, to_layer) = (padstack.from_layer(), padstack.to_layer());
    let radius = (from_layer..=to_layer)
        .filter_map(|l| padstack.get_shape(l))
        .map(|s| 0.5 * s.max_width())
        .fold(0.0_f64, f64::max)
        .ceil() as i32;
    let vcc = info.clearance_class;
    let tcc = net_clearance_class(board, pair.p);
    let matrix = &rules.clearance_matrix;
    let c_vv = (from_layer..=to_layer)
        .map(|l| matrix.get_value(vcc, vcc, l, false))
        .max()
        .unwrap_or(0);
    let c_tv = [a, b]
        .iter()
        .map(|l| matrix.get_value(tcc, vcc, *l, false))
        .max()
        .unwrap_or(0);
    let hw = pair.half_width(a).max(pair.half_width(b));
    let d_max = pair.centre_distance(a).max(pair.centre_distance(b));
    let d_min = pair.centre_distance(a).min(pair.centre_distance(b));
    let spacing = d_max
        .max(2 * radius + c_vv)
        .max(2 * (radius + hw + c_tv) - d_min);
    Ok(ViaSpec {
        padstack: info.padstack,
        clearance_class: vcc,
        radius,
        spacing,
        from_layer,
        to_layer,
    })
}

fn unit(a: IntPoint, b: IntPoint) -> Option<(f64, f64)> {
    let dx = f64::from(b.x) - f64::from(a.x);
    let dy = f64::from(b.y) - f64::from(a.y);
    let len = dx.hypot(dy);
    (len > 0.0).then(|| (dx / len, dy / len))
}

/// The P and N via centres at the layer change `at`, between the incoming
/// segment `incoming` and the outgoing segment `outgoing` of the
/// centreline, with P on the left of the direction of travel when `p_left`
/// ([module docs](self)).
pub fn via_pair(
    spec: &ViaSpec,
    at: IntPoint,
    incoming: (IntPoint, IntPoint),
    outgoing: (IntPoint, IntPoint),
    p_left: bool,
) -> Result<(IntPoint, IntPoint), String> {
    let (ix, iy) = unit(incoming.0, incoming.1).ok_or("the centreline ends in a point")?;
    let (ox, oy) = unit(outgoing.0, outgoing.1).ok_or("the centreline starts in a point")?;
    let (bx, by) = (ix + ox, iy + oy);
    let len = bx.hypot(by);
    if len < 1e-6 {
        return Err("the centreline turns back at the layer change".to_string());
    }
    // The left normal of the bisector.
    let (nx, ny) = (-by / len, bx / len);
    let half = f64::from(spec.spacing) / 2.0;
    let mut dist = half;
    let (vx, vy) = loop {
        let vx = (nx * dist).round() as i64;
        let vy = (ny * dist).round() as i64;
        if ((vx * vx + vy * vy) as f64).sqrt() >= half {
            break (vx, vy);
        }
        dist += 0.5;
    };
    let shifted = |sign: i64| -> Result<IntPoint, String> {
        let x = i32::try_from(i64::from(at.x) + sign * vx).map_err(|_| "via off the range")?;
        let y = i32::try_from(i64::from(at.y) + sign * vy).map_err(|_| "via off the range")?;
        Ok(IntPoint::new(x, y))
    };
    let (left, right) = (shifted(1)?, shifted(-1)?);
    Ok(if p_left { (left, right) } else { (right, left) })
}

/// A pin whose pad contains `centre` on a layer of `from_layer..=to_layer`
/// (D8: never a via in a pad), the lowest id first.
pub fn via_in_pad(
    board: &BasicBoard,
    centre: IntPoint,
    from_layer: i32,
    to_layer: i32,
) -> Option<ItemId> {
    let point = centre.to_float();
    board
        .items()
        .filter(|i| i.is_pin())
        .filter(|pin| {
            (0..board.tile_shape_count(pin)).any(|index| {
                let layer = pin.shape_layer(index);
                (from_layer..=to_layer).contains(&layer)
                    && board
                        .tile_shape(pin, index)
                        .is_some_and(|s| s.contains_float(point))
            })
        })
        .map(|pin| pin.id())
        .min()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(spacing: i32) -> ViaSpec {
        ViaSpec {
            padstack: PadstackId(1),
            clearance_class: 1,
            radius: 3000,
            spacing,
            from_layer: 0,
            to_layer: 1,
        }
    }

    fn dist(a: IntPoint, b: IntPoint) -> f64 {
        a.distance(b)
    }

    #[test]
    fn a_straight_change_puts_the_vias_across_the_centreline() {
        let at = IntPoint::new(10_000, 5_000);
        let east = (IntPoint::new(0, 5_000), at);
        let on = (at, IntPoint::new(20_000, 5_000));
        let (p, n) = via_pair(&spec(7_501), at, east, on, true).unwrap();
        // Left of east is +y.
        assert_eq!(p, IntPoint::new(10_000, 5_000 + 3_751));
        assert_eq!(n, IntPoint::new(10_000, 5_000 - 3_751));
        assert!(dist(p, n) >= 7_501.0);
        let (p2, n2) = via_pair(&spec(7_501), at, east, on, false).unwrap();
        assert_eq!((p2, n2), (n, p));
    }

    #[test]
    fn a_diagonal_change_keeps_at_least_the_spacing() {
        let at = IntPoint::new(0, 0);
        let incoming = (IntPoint::new(-1_000, -1_000), at);
        let outgoing = (at, IntPoint::new(0, 1_000));
        for s in [7_500, 7_501, 3_333, 1] {
            let (p, n) = via_pair(&spec(s), at, incoming, outgoing, true).unwrap();
            let d = dist(p, n);
            assert!(d >= f64::from(s), "spacing {s}: {d}");
            assert!(d <= f64::from(s) + 3.0, "spacing {s}: {d}");
            // Symmetric about the change point.
            assert_eq!(p.x + n.x, 0);
            assert_eq!(p.y + n.y, 0);
        }
    }

    #[test]
    fn a_u_turn_is_refused() {
        let at = IntPoint::new(0, 0);
        let incoming = (IntPoint::new(-1_000, 0), at);
        let outgoing = (at, IntPoint::new(-1_000, 0));
        assert!(via_pair(&spec(7_500), at, incoming, outgoing, true).is_err());
    }
}
