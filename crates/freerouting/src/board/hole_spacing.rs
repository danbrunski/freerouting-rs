//! Q22.11: the board's minimum hole-to-hole distance between drills.
//!
//! Not Java. KiCad's DRC keeps any two drilled holes, of any net, at least
//! Board Setup's hole-to-hole distance apart (`m_HoleToHoleMin`, `.kicad_pro`
//! `min_hole_to_hole`, 0.25 mm by default; its report type is
//! `hole_to_hole`), measured from hole edge to hole edge. Copper clearance
//! keeps vias of different nets apart (with their annular rings, Q22.10); two
//! vias of one net, or a via next to a pad drill of its own net, have no
//! clearance between them at all.
//!
//! [`BoardRules::hole_to_hole`](crate::rules::BoardRules::hole_to_hole) holds
//! the distance in board units (0 is none, Java's board); the job sets it
//! from `router.board_minimums.hole_to_hole_um`
//! ([`crate::job::hole_to_hole`]). With a distance set:
//!
//! - the maze search drops a drill location whose via hole comes closer to
//!   another drill (`autoroute::session::layers`, `expand_to_other_layers`);
//! - the via insertion skips a via type whose hole does
//!   ([`TraceShover::check_via`](crate::shove::TraceShover::check_via));
//! - a via move (the via optimizer, a shoved via) is refused when the moved
//!   hole does ([`TraceShover::check_drill_item_move`](crate::shove::TraceShover::check_drill_item_move));
//! - Q20's output check reports a via that does as `hole_to_hole`
//!   ([`crate::drc::board_rules`]), and `repair` removes it.
//!
//! The `freerouting-krt` stages that insert vias themselves (the QFN and BGA
//! fanouts, the diff-pair legs) do not ask; the output check catches what
//! they place.
//!
//! A drill is a via's or a pin's on more than one layer (an SMD pad has
//! none), with the radius of its padstack
//! ([`Padstack::get_drill_radius`](crate::library::Padstack::get_drill_radius),
//! as the hole clearance takes it). The gap is the distance of the centres
//! less both radii; less than the minimum is a breach, with no tolerance.

use freerouting_geometry::{FloatPoint, IntBox, TileShape};

use super::BasicBoard;
use super::items::{Item, ItemId};

impl BasicBoard {
    /// The drill radius of a via or of a pin on more than one layer, or
    /// `None` for an SMD pad, another item or a drill without a radius.
    pub fn hole_radius(&self, item: &Item) -> Option<f64> {
        let drill = item.drill()?;
        if item.is_pin() && drill.first_layer == drill.last_layer {
            return None;
        }
        let radius = self
            .library
            .padstacks
            .get(drill.padstack)?
            .get_drill_radius();
        (radius > 0.0).then_some(radius)
    }

    /// The largest drill radius of the board's padstacks: how far beyond the
    /// minimum a hole query reaches.
    fn max_padstack_drill_radius(&self) -> f64 {
        self.library
            .padstacks
            .iter()
            .map(|p| p.get_drill_radius())
            .filter(|r| r.is_finite())
            .fold(0.0, f64::max)
    }

    /// Every drill a hole of `radius` at `center` comes closer to than the
    /// board's hole-to-hole distance
    /// ([`BoardRules::hole_to_hole`](crate::rules::BoardRules::hole_to_hole)),
    /// skipping `ignore`: the item and the gap between the hole edges, in
    /// board units, in ascending item id order. Empty without a distance.
    pub fn hole_to_hole_breaches(
        &self,
        center: FloatPoint,
        radius: f64,
        ignore: &[ItemId],
    ) -> Vec<(ItemId, f64)> {
        let min = f64::from(self.rules.hole_to_hole());
        if min <= 0.0 {
            return Vec::new();
        }
        let reach = (radius + min + self.max_padstack_drill_radius()).ceil() + 1.0;
        let clamp = |v: f64| v.clamp(f64::from(-i32::MAX), f64::from(i32::MAX)) as i32;
        let region = TileShape::IntBox(IntBox::new(
            clamp((center.x - reach).floor()),
            clamp((center.y - reach).floor()),
            clamp((center.x + reach).ceil()),
            clamp((center.y + reach).ceil()),
        ));
        // A BTreeSet: ascending ids; the leaves' bounds hold their drills'
        // centres, so every drill within reach is among them.
        self.search_trees()
            .default_tree()
            .items_near_unmetered(&region)
            .into_iter()
            .filter(|id| !ignore.contains(id))
            .filter_map(|id| {
                let other = self.item_ref(id);
                let other_radius = self.hole_radius(other)?;
                let other_center = other.drill()?.center.to_float();
                let gap = center.distance(other_center) - radius - other_radius;
                (gap < min).then_some((id, gap))
            })
            .collect()
    }

    /// The closest of [`hole_to_hole_breaches`](Self::hole_to_hole_breaches)
    /// (a tie keeps the lower id), or `None`.
    pub fn hole_to_hole_breach(
        &self,
        center: FloatPoint,
        radius: f64,
        ignore: &[ItemId],
    ) -> Option<(ItemId, f64)> {
        self.hole_to_hole_breaches(center, radius, ignore)
            .into_iter()
            .fold(None, |best: Option<(ItemId, f64)>, b| match best {
                Some(a) if a.1 <= b.1 => Some(a),
                _ => Some(b),
            })
    }
}
