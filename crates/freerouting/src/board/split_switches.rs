//! The board's switches of the cutouts' convex split (N06: `router.java_quirks`
//! `octagon_box_cutout_branch3`, `cutout_hole_crosses_border`,
//! `simplex_octagon_half_corner` and the inert `simplex_prev_division_line`).
//!
//! Every `Area` split of the board takes [`JavaQuirks::split`] of
//! [`BasicBoard::java_quirks`]: the search trees' shapes of areas with holes and
//! of the outline's generated keepout ([`ShapeContext::split_quirks`]), the
//! outline's piece count, `check_shape`, `overlapping_items`, the spring-over
//! shape of an area, the interactive hole and the plane adjustment after reading.
//! With every switch `true` each split is Java's.
//!
//! The reader builds the board with its final switches before it inserts an
//! item ([`BasicBoard::new_with_java_quirks`]), so the trees hold the switched
//! pieces from the start. When the switches change on a board that has items,
//! [`BasicBoard::set_java_quirks`] reinserts, in item-list order, exactly the
//! items whose pieces change: the trees then hold the same pieces as a board
//! built with the new switches, though their leaves may sit elsewhere in the
//! tree (a full build inserts in a different order).
//!
//! Tree order is part of the routing result (D5), so with a split switch
//! `false` a board switched after reading can route differently from one read
//! with the switches. The CLI reads with the job's switches
//! ([`prepare_job`](crate::job::prepare::prepare_job)'s first merge), and so
//! does the GUI: every load path (open, drop, reload, a session, the `-de`
//! front-end mode, a `.kicad_pcb`) reads with the switches of the profile plus
//! the user's saved settings and the front-end flags
//! (`freerouting_gui::session::LoadSettings`), so both give the same tree
//! order. Only a switch changed after the load (the settings dialog, a rules
//! file that sets `java_quirks`) takes the switched-after-read path:
//! `prepare_board_for_routing` calls [`BasicBoard::set_java_quirks`] and the
//! board can route differently from one read with the final switches. N06's
//! measurements use the CLI path. With every split switch `true` nothing is
//! reinserted, and the paths are the same.
//!
//! [`ShapeContext::split_quirks`]: crate::board::search::ShapeContext::split_quirks

use freerouting_geometry::{Area, SplitQuirks, TileShape};

use crate::board::BasicBoard;
use crate::board::items::{ItemId, ItemKind};
use crate::settings::JavaQuirks;

impl BasicBoard {
    /// The switches of the Java bugs kept for parity (N06,
    /// `router.java_quirks.*`); [`JavaQuirks::JAVA`] unless a job set them.
    pub fn java_quirks(&self) -> JavaQuirks {
        self.java_quirks
    }

    /// Sets the switches of the Java bugs kept for parity (N06). The job sets
    /// them from the settings before it reads a session or routes.
    ///
    /// When the cutouts' split switches ([`JavaQuirks::split`]) change, the items
    /// whose convex pieces change are reinserted into the search trees in
    /// item-list order, and the outline's keepout piece count is retaken (see the
    /// module docs). Nothing else is touched, so a value with the same split
    /// switches changes no tree.
    pub fn set_java_quirks(&mut self, quirks: JavaQuirks) {
        let old = self.java_quirks.split();
        self.java_quirks = quirks;
        let new = quirks.split();
        if old != new {
            self.resplit_items(old, new);
        }
    }

    /// Reinserts the items whose pieces differ between the split switches `old`
    /// and `new`.
    fn resplit_items(&mut self, old: SplitQuirks, new: SplitQuirks) {
        let ids: Vec<ItemId> = self.items.keys().copied().collect();
        for id in ids {
            let changed = match self.items.get(&id).map(|i| &i.kind) {
                Some(ItemKind::Area(area)) => pieces_differ(area.area(), old, new),
                Some(ItemKind::BoardOutline(_)) => self.resplit_outline(id, new),
                _ => false,
            };
            if changed && self.items.get(&id).is_some_and(|i| i.on_the_board) {
                self.search_trees.remove(id);
                self.insert_into_trees(id);
                // Its shapes changed, so its nets' incomplete counts are stale.
                self.note_changed(id);
            }
        }
    }

    /// Retakes outline `id`'s keepout piece count under `quirks`; true if its
    /// pieces changed.
    fn resplit_outline(&mut self, id: ItemId, quirks: SplitQuirks) -> bool {
        let pieces = |board: &BasicBoard| {
            board
                .items
                .get(&id)
                .and_then(|i| i.as_board_outline())
                .and_then(|o| o.keepout_pieces())
        };
        let before = pieces(self);
        let Some(ItemKind::BoardOutline(outline)) = self.items.get_mut(&id).map(|i| &mut i.kind)
        else {
            return false;
        };
        let refreshed = outline.refresh_keepout_split(quirks);
        refreshed && !same_pieces(&before, &pieces(self))
    }

    /// Brings an outline's keepout piece count to the board's split switches
    /// before the outline enters the trees (an undo can restore an outline
    /// whose count was taken under other switches).
    pub(crate) fn sync_outline_split(&mut self, id: ItemId) {
        let split = self.java_quirks.split();
        let stale = self
            .items
            .get(&id)
            .and_then(|i| i.as_board_outline())
            .is_some_and(|o| o.keepout_split() != split);
        if stale
            && let Some(ItemKind::BoardOutline(outline)) =
                self.items.get_mut(&id).map(|i| &mut i.kind)
        {
            outline.refresh_keepout_split(split);
        }
    }
}

/// True if `area` has holes whose cutouts give other pieces under `old` and
/// `new`.
fn pieces_differ(area: &Area, old: SplitQuirks, new: SplitQuirks) -> bool {
    let Area::PolylineArea(a) = area else {
        return false;
    };
    if a.get_holes().is_empty() {
        return false;
    }
    !same_pieces(
        &a.split_to_convex_with(None, old),
        &a.split_to_convex_with(None, new),
    )
}

/// True if both splits failed, or both gave the same pieces in the same order.
pub(crate) fn same_pieces(a: &Option<Vec<TileShape>>, b: &Option<Vec<TileShape>>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => a.len() == b.len() && a.iter().zip(b).all(|(x, y)| same_tile(x, y)),
        _ => false,
    }
}

fn same_tile(a: &TileShape, b: &TileShape) -> bool {
    match (a, b) {
        (TileShape::IntBox(x), TileShape::IntBox(y)) => x == y,
        (TileShape::IntOctagon(x), TileShape::IntOctagon(y)) => x == y,
        (TileShape::Simplex(x), TileShape::Simplex(y)) => x.lines() == y.lines(),
        _ => false,
    }
}

/// Shapes and boards for the unit tests of the split sites (the integration tests
/// are `tests/n06_area_splits.rs`).
#[cfg(test)]
pub(crate) mod test_support {
    use freerouting_geometry::{
        Area, FloatPoint, IntBox, IntPoint, Line, PolylineArea, PolylineShape, Simplex, TileShape,
        TileShapeOps,
    };

    use crate::board::RoutingBoard;
    use crate::board::structure::{Layer, LayerStructure};
    use crate::rules::{BoardRules, ClearanceMatrix};
    use crate::settings::JavaQuirks;

    /// The four split switches `false`, every other switch Java's.
    pub(crate) fn split_fixed() -> JavaQuirks {
        [
            "simplex_prev_division_line",
            "octagon_box_cutout_branch3",
            "cutout_hole_crosses_border",
            "simplex_octagon_half_corner",
        ]
        .into_iter()
        .fold(JavaQuirks::JAVA, |q, id| q.with(id, false).unwrap())
    }

    fn line(ax: i32, ay: i32, bx: i32, by: i32) -> Line {
        Line::new(IntPoint::new(ax, ay), IntPoint::new(bx, by))
    }

    /// A box minus a triangle crossing its bottom and left sides
    /// (`java_breaks_the_invariant_outside_the_two_rows`): Java's split loses about
    /// half the rest.
    pub(crate) fn border_area() -> Area {
        let page = TileShape::IntBox(IntBox::new(-77823, -69103, 64463, 7809));
        let triangle = TileShape::Simplex(Simplex::new(vec![
            line(-44154, -109768, 10312, -28116),
            line(10312, -28116, -82325, -30905),
            line(-82325, -30905, -44154, -109768),
        ]));
        Area::PolylineArea(PolylineArea::new(
            PolylineShape::from(page),
            vec![PolylineShape::from(triangle)],
        ))
    }

    /// The total area of the pieces of `area` under `quirks`.
    pub(crate) fn pieces_total(area: &Area, quirks: JavaQuirks) -> f64 {
        area.split_to_convex_with(None, quirks.split())
            .expect("split")
            .iter()
            .map(TileShapeOps::area)
            .sum()
    }

    /// A point inside a piece under [`split_fixed`] that no piece of Java's split
    /// covers.
    pub(crate) fn point_java_loses(area: &Area) -> FloatPoint {
        let java = area.split_to_convex().unwrap();
        let fixed = area
            .split_to_convex_with(None, split_fixed().split())
            .unwrap();
        fixed
            .iter()
            .map(freerouting_geometry::PolylineShapeOps::centre_of_gravity)
            .find(|c| !java.iter().any(|j| j.contains_float(*c)))
            .expect("Java covers every fixed piece's centre")
    }

    /// A two-layer board (signal layers) with outline `bounds`, net 1 and `quirks`.
    pub(crate) fn board(bounds: IntBox, quirks: JavaQuirks) -> RoutingBoard {
        let layers = LayerStructure::new(vec![Layer::new("Top", true), Layer::new("Bottom", true)]);
        let matrix = ClearanceMatrix::default_instance(layers.count(), 10);
        let mut rules = BoardRules::new(layers.clone(), matrix);
        rules.create_default_net_class();
        let mut board = RoutingBoard::new_with_java_quirks(
            bounds,
            layers,
            vec![PolylineShape::Tile(TileShape::IntBox(bounds))],
            0,
            rules,
            Default::default(),
            quirks,
        );
        let class = board.rules.default_net_class();
        board.rules.nets.add("A", 1, false, class);
        board
    }
}
