//! Drill pages and drills (A03).
//!
//! Port of `autoroute/drill/DrillPageArray.java`, `DrillPage.java`,
//! `ExpansionDrill.java` and `Via.getAutorouteDrillInfo` (`Via.java:205`)
//! at `a773a70ec`. Layer changes of the maze search go through drills:
//! candidate via locations that a page computes for the session's net by
//! cutting the obstacles of the autoroute tree out of the page and splitting
//! the rest into convex pieces. Each drill finds or completes one complete
//! room per layer.
//!
//! The page array covers the board's bounding box with rectangles of about
//! equal size, at most `max(5 * default via diameter, 10000)` wide
//! (`AutorouteEngine.java:89-91`). Java builds it when the engine is
//! created; the session builds it on first use, which gives the same pages
//! (the construction reads nothing the session changes).
//!
//! Pages live in the session in row-major order (`pages[j][i]` with `j` the
//! row), drills in an arena addressed by [`DrillId`]; neither is reused
//! within a session. The maze search state of both is in the session's flat
//! section array: a page has one section per board layer, a drill one per
//! layer of its span.
//!
//! Orders that decide results, each marked `// PARITY:`: the page order of
//! `overlapping_pages`, the tree order of a page's obstacles (with the
//! previous-obstacle rule), the order of the convex pieces, which is the
//! order of the page's drills, the pin chosen as a drill location (the last
//! in Java's descending id order), and the room scan of
//! `calculate_expansion_rooms`.
//!
//! Q09: when the control says the board may have trace obstacles that let
//! vias through (`vias_pass_trace_only_areas`, false on the parity path), a
//! layer on which no room can be completed at the drill's location because
//! only such obstacles cover it keeps no room (`None`): a via may pass the
//! layer, but the search does not expand onto it. The via is checked there
//! against the drill's shape (`layers.rs`, `check_drill_layer`).
//!
//! A page whose split into convex pieces is stopped keeps an empty drill
//! list for the session's net and is not recomputed, as in Java (the net
//! number and the empty list are set before the split).
//!
//! `Session::clear` drops the pages and the drills with the rooms. Java's
//! `clear` leaves the page array alone, so its drills then point to removed
//! rooms; the batch router discards the engine (one per connection), but the
//! fanout routes up to three connections on one engine, which
//! [`DrillCarry`] reproduces.

use freerouting_geometry::jmath::d2i;
use freerouting_geometry::{
    IntBox, Point, PolylineArea, PolylineShape, PolylineShapeOps, TileShape, TileShapeOps,
};

use super::{Room, RoomError, RoomId, SectionState, Session};
use crate::autoroute::java_ids;
use crate::board::items::ItemId;
use crate::board::search::{TreeEntry, TreeObject};

/// The smallest page width limit (`AutorouteEngine.java:90`).
pub const MIN_MAX_PAGE_WIDTH: i32 = 10000;

/// An expansion drill of the session's arena.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DrillId(pub u32);

/// A drill page, by its row-major index in the page array.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DrillPageId(pub u32);

/// A rectangle of the page array (Java `DrillPage`).
#[derive(Clone, Debug)]
pub struct DrillPage {
    /// The page's rectangle.
    pub shape: IntBox,
    /// The range of the page's sections (one per board layer).
    pub(crate) sections: (u32, u32),
    /// The drills, `None` until calculated or after an invalidation.
    drills: Option<Vec<DrillId>>,
    /// The net the drills were calculated for (`netNumber`), -1 at first.
    net: i32,
}

impl DrillPage {
    fn new(shape: IntBox) -> DrillPage {
        DrillPage {
            shape,
            sections: (0, 0),
            drills: None,
            net: -1,
        }
    }

    /// The drills calculated so far; `None` if not calculated or
    /// invalidated.
    pub fn drills(&self) -> Option<&[DrillId]> {
        self.drills.as_deref()
    }

    /// The net of the last drill calculation; -1 before the first.
    pub fn net_number(&self) -> i32 {
        self.net
    }

    /// Java `DrillPage.getId`: `31 * shape.getId() + netNumber`.
    pub fn java_id(&self) -> i32 {
        java_ids::drill_page_id(self.shape.get_id(), self.net)
    }

    /// Java `invalidate`: the drills are recalculated at the next call of
    /// `drills`.
    pub fn invalidate(&mut self) {
        self.drills = None;
    }
}

/// The pages covering the board (Java `DrillPageArray`).
#[derive(Clone, Debug)]
pub struct DrillPageArray {
    bounds: IntBox,
    column_count: i32,
    row_count: i32,
    page_width: i32,
    page_height: i32,
    /// Row-major: page `(i, j)` (column `i`, row `j`) is at
    /// `j * column_count + i`.
    pages: Vec<DrillPage>,
}

/// The drill pages and drills one Java `AutorouteEngine` keeps between the
/// connections it routes (A14). Java's `clear` (`AutorouteEngine.java:321-332`)
/// leaves `drillPageArray` alone and only the retained-database mode resets
/// it (`resetAllDoors`, `:692`), so a second `autorouteConnection` on the same
/// engine sees the pages with their drills (calculated for the same net,
/// so not recalculated, and pointing to the removed rooms of the first
/// search) and the occupied flags of their maze search elements. Only
/// `RoutingBoard.fanout` routes more than one connection per engine (the
/// plane attempt, the closest target, then all targets, `:1077-1126`); the
/// batch router creates an engine per attempt. Of the section states only
/// the occupied flags are kept: an occupied section is never popped again,
/// and a free one is overwritten when it is.
#[derive(Clone, Debug, Default)]
pub struct DrillCarry {
    pages: Option<DrillPageArray>,
    drills: Vec<ExpansionDrill>,
    /// The occupied flags; each page's and drill's `sections` range indexes
    /// this vector while carried.
    occupied: Vec<bool>,
}

impl DrillPageArray {
    /// The page width limit of a board (`AutorouteEngine.java:89-90`):
    /// `max((int) (5 * defaultViaDiameter), 10000)`.
    pub fn max_page_width(default_via_diameter: f64) -> i32 {
        d2i(5.0 * default_via_diameter).max(MIN_MAX_PAGE_WIDTH)
    }

    /// Java `new DrillPageArray(board, maxPageWidth)` (`:34-61`) on the
    /// board box `bounds`. The last column and row end at the bounds. The
    /// sections of the pages are not allocated.
    pub fn new(bounds: IntBox, max_page_width: i32) -> DrillPageArray {
        let length = f64::from(bounds.ur.x) - f64::from(bounds.ll.x);
        let height = f64::from(bounds.ur.y) - f64::from(bounds.ll.y);
        let column_count = d2i((length / f64::from(max_page_width)).ceil());
        let row_count = d2i((height / f64::from(max_page_width)).ceil());
        let page_width = d2i((length / f64::from(column_count)).ceil());
        let page_height = d2i((height / f64::from(row_count)).ceil());
        let mut pages = Vec::with_capacity((row_count.max(0) * column_count.max(0)) as usize);
        for j in 0..row_count {
            for i in 0..column_count {
                // Java int arithmetic: the products stay below the board box.
                let ll_x = bounds.ll.x.wrapping_add(i.wrapping_mul(page_width));
                let ur_x = if i == column_count - 1 {
                    bounds.ur.x
                } else {
                    ll_x.wrapping_add(page_width)
                };
                let ll_y = bounds.ll.y.wrapping_add(j.wrapping_mul(page_height));
                let ur_y = if j == row_count - 1 {
                    bounds.ur.y
                } else {
                    ll_y.wrapping_add(page_height)
                };
                pages.push(DrillPage::new(IntBox::new(ll_x, ll_y, ur_x, ur_y)));
            }
        }
        DrillPageArray {
            bounds,
            column_count,
            row_count,
            page_width,
            page_height,
            pages,
        }
    }

    pub fn bounds(&self) -> IntBox {
        self.bounds
    }

    pub fn column_count(&self) -> i32 {
        self.column_count
    }

    pub fn row_count(&self) -> i32 {
        self.row_count
    }

    pub fn page_width(&self) -> i32 {
        self.page_width
    }

    pub fn page_height(&self) -> i32 {
        self.page_height
    }

    /// The pages in row-major order.
    pub fn pages(&self) -> &[DrillPage] {
        &self.pages
    }

    pub fn page(&self, id: DrillPageId) -> &DrillPage {
        &self.pages[id.0 as usize]
    }

    pub(crate) fn page_mut(&mut self, id: DrillPageId) -> &mut DrillPage {
        &mut self.pages[id.0 as usize]
    }

    /// The page at column `i` and row `j`.
    pub fn page_at(&self, i: i32, j: i32) -> Option<DrillPageId> {
        if i < 0 || j < 0 || i >= self.column_count || j >= self.row_count {
            return None;
        }
        Some(DrillPageId((j * self.column_count + i) as u32))
    }

    /// Java `overlappingPages` (`:76-98`): the pages with a 2-dimensional
    /// overlap with `shape`, row by row.
    ///
    /// The loop bounds are Java's: the first row is the floor of the
    /// shape box's lower edge in page units, and rows are taken while
    /// `j < max_j` with `max_j` the upper edge as an `f64`, so a box that
    /// ends exactly on a page boundary does not reach the next page. The
    /// division by a zero page size of an empty board gives no page, as in
    /// Java. A shape outside the bounds clips to the empty box, whose loop
    /// bounds are empty; a clipped box inside the bounds gives indexes in
    /// the array (`max_j <= row_count` because `page_height >= height /
    /// row_count`), so the range check of `page_at` never drops a page.
    pub fn overlapping_pages(&self, shape: &TileShape) -> Vec<DrillPageId> {
        let mut result = Vec::new();
        let shape_box = shape.bounding_box().intersection_box(&self.bounds);
        let page_height = f64::from(self.page_height);
        let page_width = f64::from(self.page_width);
        let min_j = d2i((f64::from(shape_box.ll.y - self.bounds.ll.y) / page_height).floor());
        let max_j = f64::from(shape_box.ur.y - self.bounds.ll.y) / page_height;
        let min_i = d2i((f64::from(shape_box.ll.x - self.bounds.ll.x) / page_width).floor());
        let max_i = f64::from(shape_box.ur.x - self.bounds.ll.x) / page_width;
        // PARITY: DrillPageArray.java:85-94, row-major, j < maxJ as doubles.
        let mut j = min_j;
        while f64::from(j) < max_j {
            let mut i = min_i;
            while f64::from(i) < max_i {
                if let Some(id) = self.page_at(i, j) {
                    let page = TileShape::IntBox(self.page(id).shape);
                    if shape.intersection(&page).dimension() > 1 {
                        result.push(id);
                    }
                }
                i += 1;
            }
            j += 1;
        }
        result
    }

    /// Java `invalidate(shape)`: the pages overlapping `shape` drop their
    /// drills.
    pub fn invalidate(&mut self, shape: &TileShape) {
        for id in self.overlapping_pages(shape) {
            self.page_mut(id).invalidate();
        }
    }
}

/// A layer change of the maze search (Java `ExpansionDrill`).
#[derive(Clone, Debug)]
pub struct ExpansionDrill {
    /// The shape of the drill.
    pub shape: TileShape,
    /// Where the via is tried.
    pub location: Point,
    pub first_layer: i32,
    pub last_layer: i32,
    /// The room of each layer from `first_layer` to `last_layer`
    /// (`roomArr`): complete free rooms for a page's drill, obstacle rooms
    /// for a via's drill.
    pub rooms: Vec<Option<RoomId>>,
    /// The range of the drill's sections (one per layer).
    pub(crate) sections: (u32, u32),
}

impl ExpansionDrill {
    /// Java `ExpansionDrill.getId`: `31 * (31 * location.getId() +
    /// firstLayer) + lastLayer`.
    pub fn java_id(&self) -> i32 {
        java_ids::expansion_drill_id(self.location.get_id(), self.first_layer, self.last_layer)
    }
}

impl Session<'_> {
    /// The drill page array, built on first use with the sections of its
    /// pages.
    pub fn drill_pages(&mut self) -> &DrillPageArray {
        self.drill_pages_mut()
    }

    /// The drill page array if it was built.
    pub fn built_drill_pages(&self) -> Option<&DrillPageArray> {
        self.drill_pages.as_ref()
    }

    fn drill_pages_mut(&mut self) -> &mut DrillPageArray {
        if self.drill_pages.is_none() {
            let board = &*self.board;
            let diameter = board.rules.default_via_diameter(&board.library.padstacks);
            let mut array = DrillPageArray::new(
                board.bounding_box(),
                DrillPageArray::max_page_width(diameter),
            );
            let layers = board.layer_count();
            for page in &mut array.pages {
                page.sections = self.allocate_section_range(layers);
            }
            self.drill_pages = Some(array);
        }
        self.drill_pages
            .as_mut()
            .expect("the drill page array was just built")
    }

    /// Appends `count` fresh sections to the flat section array.
    fn allocate_section_range(&mut self, count: usize) -> (u32, u32) {
        let start = self.sections.len() as u32;
        self.sections
            .resize_with(self.sections.len() + count, SectionState::default);
        (start, count as u32)
    }

    fn section_range(&self, (start, len): (u32, u32)) -> &[SectionState] {
        &self.sections[start as usize..(start + len) as usize]
    }

    fn reset_section_range(&mut self, (start, len): (u32, u32)) {
        for section in &mut self.sections[start as usize..(start + len) as usize] {
            section.reset();
        }
    }

    /// A drill of the arena.
    pub fn drill(&self, id: DrillId) -> &ExpansionDrill {
        &self.drills[id.0 as usize]
    }

    /// The number of drills created so far (the arena keeps every drill).
    pub fn drill_count(&self) -> usize {
        self.drills.len()
    }

    /// The sections of a drill, one per layer of its span.
    pub fn drill_sections(&self, id: DrillId) -> &[SectionState] {
        self.section_range(self.drill(id).sections)
    }

    /// The sections of a drill, mutable.
    pub fn drill_sections_mut(&mut self, id: DrillId) -> &mut [SectionState] {
        let (start, len) = self.drill(id).sections;
        &mut self.sections[start as usize..(start + len) as usize]
    }

    /// The sections of a page, one per board layer.
    pub fn page_sections(&mut self, page: DrillPageId) -> &[SectionState] {
        let range = self.drill_pages_mut().page(page).sections;
        self.section_range(range)
    }

    /// The sections of a page, mutable.
    pub fn page_sections_mut(&mut self, page: DrillPageId) -> &mut [SectionState] {
        let (start, len) = self.drill_pages_mut().page(page).sections;
        &mut self.sections[start as usize..(start + len) as usize]
    }

    /// Java `new ExpansionDrill(shape, location, firstLayer, lastLayer)`:
    /// a drill without rooms, with one fresh section per layer.
    pub fn new_drill(
        &mut self,
        shape: TileShape,
        location: Point,
        first_layer: i32,
        last_layer: i32,
    ) -> DrillId {
        let layer_count = (last_layer - first_layer + 1).max(0) as usize;
        let sections = self.allocate_section_range(layer_count);
        let id = DrillId(self.drills.len() as u32);
        self.drills.push(ExpansionDrill {
            shape,
            location,
            first_layer,
            last_layer,
            rooms: vec![None; layer_count],
            sections,
        });
        id
    }

    /// Java `ExpansionDrill.reset`: the drill's sections.
    pub fn reset_drill(&mut self, id: DrillId) {
        let range = self.drill(id).sections;
        self.reset_section_range(range);
    }

    /// Java `DrillPage.reset` (`:158-170`): the page's drills, then its own
    /// sections.
    pub fn reset_drill_page(&mut self, page: DrillPageId) {
        let array = self.drill_pages_mut();
        let range = array.page(page).sections;
        let drills = array.page(page).drills.clone().unwrap_or_default();
        for drill in drills {
            self.reset_drill(drill);
        }
        self.reset_section_range(range);
    }

    /// Java `DrillPageArray.reset`: every page, row by row. Nothing to do
    /// before the array is built.
    pub fn reset_drill_pages(&mut self) {
        let count = self.drill_pages.as_ref().map_or(0, |a| a.pages.len());
        for page in 0..count {
            self.reset_drill_page(DrillPageId(page as u32));
        }
    }

    /// Java `DrillPage.invalidate` of one page.
    pub fn invalidate_drill_page(&mut self, page: DrillPageId) {
        self.drill_pages_mut().page_mut(page).invalidate();
    }

    /// Java `DrillPageArray.overlappingPages` on the session's array.
    pub fn overlapping_drill_pages(&mut self, shape: &TileShape) -> Vec<DrillPageId> {
        self.drill_pages_mut().overlapping_pages(shape)
    }

    /// Drops the pages and the drills with their sections (part of
    /// `clear`).
    pub(crate) fn clear_drills(&mut self) {
        self.drill_pages = None;
        self.drills.clear();
    }

    /// Takes the page array and the drills out of the session for the next
    /// connection of the same engine (see [`DrillCarry`]). The session keeps
    /// none.
    pub(crate) fn take_drill_carry(&mut self) -> DrillCarry {
        let Some(mut pages) = self.drill_pages.take() else {
            self.drills.clear();
            return DrillCarry::default();
        };
        let mut occupied = Vec::new();
        let mut keep = |range: &mut (u32, u32), sections: &[SectionState]| {
            let start = occupied.len() as u32;
            occupied.extend(
                sections[range.0 as usize..(range.0 + range.1) as usize]
                    .iter()
                    .map(|s| s.occupied),
            );
            *range = (start, range.1);
        };
        for page in &mut pages.pages {
            keep(&mut page.sections, &self.sections);
        }
        let mut drills = std::mem::take(&mut self.drills);
        for drill in &mut drills {
            keep(&mut drill.sections, &self.sections);
        }
        DrillCarry {
            pages: Some(pages),
            drills,
            occupied,
        }
    }

    /// Puts `carry` into a new session, before its search: the pages and
    /// drills get sections in this session with their occupied flags, and
    /// the drills lose their rooms, which belonged to the session that
    /// computed them.
    pub(crate) fn install_drill_carry(&mut self, carry: DrillCarry) {
        let DrillCarry {
            pages,
            mut drills,
            occupied,
        } = carry;
        let Some(mut pages) = pages else {
            return;
        };
        let place = |range: &mut (u32, u32), session: &mut Self| {
            let fresh = session.allocate_section_range(range.1 as usize);
            for k in 0..range.1 {
                session.sections[(fresh.0 + k) as usize].occupied =
                    occupied[(range.0 + k) as usize];
            }
            *range = fresh;
        };
        for page in &mut pages.pages {
            place(&mut page.sections, self);
        }
        for drill in &mut drills {
            place(&mut drill.sections, self);
            // PARITY(quirk): Java's `roomArr` keeps the removed rooms of the
            // earlier search, which no room of this one is identical to.
            drill.rooms.iter_mut().for_each(|room| *room = None);
        }
        self.drill_pages = Some(pages);
        self.drills = drills;
    }

    /// Java `DrillPage.getDrills(engine, attachSmd)` (`:63-135`): the drills
    /// of `page` for the session's net, calculated when the page has none
    /// yet or was calculated for another net. With `attach_smd`, own-net
    /// SMD pins are no obstacles and a drill over one is placed at its
    /// centre.
    pub fn page_drills(
        &mut self,
        page: DrillPageId,
        attach_smd: bool,
    ) -> Result<&[DrillId], RoomError> {
        let net = self.net;
        let current = self.drill_pages_mut().page(page);
        if current.drills.is_none() || current.net != net {
            let page_box = current.shape;
            {
                let page = self.drill_pages_mut().page_mut(page);
                page.net = net;
                page.drills = Some(Vec::new());
            }
            let drills = self.calculate_page_drills(page_box, attach_smd)?;
            self.drill_pages_mut().page_mut(page).drills = Some(drills);
        }
        Ok(self
            .drill_pages
            .as_ref()
            .and_then(|a| a.page(page).drills.as_deref())
            .unwrap_or(&[]))
    }

    /// The body of `getDrills` after the cache check: the page minus the
    /// obstacles, split into convex pieces, one drill per piece that finds
    /// its rooms.
    fn calculate_page_drills(
        &mut self,
        page_box: IntBox,
        attach_smd: bool,
    ) -> Result<Vec<DrillId>, RoomError> {
        let page_shape = TileShape::IntBox(page_box);
        let holes = self.page_cutouts(&page_shape, attach_smd)?;
        let area = PolylineArea::new(
            PolylineShape::from(page_shape),
            holes.into_iter().map(PolylineShape::from).collect(),
        );
        let Some(drill_shapes) = area.split_to_convex(self.stop) else {
            // A stopped or failed split leaves the page empty for this net.
            return Ok(Vec::new());
        };
        let first_layer = 0;
        let last_layer = self.board.layer_count() as i32 - 1;
        let mut drills = Vec::new();
        // PARITY: DrillPage.java:110-132, the drills in the order of the
        // convex pieces.
        for shape in drill_shapes {
            let mut location = None;
            if attach_smd {
                location = self
                    .pin_center_in_drill(&shape, first_layer)
                    .or_else(|| self.pin_center_in_drill(&shape, last_layer));
            }
            let location =
                location.unwrap_or_else(|| Point::Int(shape.centre_of_gravity().round()));
            let drill = self.new_drill(shape, location, first_layer, last_layer);
            if self.calculate_expansion_rooms(drill)? {
                drills.push(drill);
            }
        }
        Ok(drills)
    }

    /// The obstacle cutouts of a page (`DrillPage.java:70-98`), in the
    /// autoroute tree's order. An obstacle is cut out only if the previous
    /// obstacle's shape does not contain it (vias have the same shape on
    /// every layer), and every obstacle that is not skipped becomes the
    /// previous one.
    fn page_cutouts(
        &self,
        page_shape: &TileShape,
        attach_smd: bool,
    ) -> Result<Vec<TileShape>, RoomError> {
        let mut entries: Vec<TreeEntry> = Vec::new();
        let tree = self.tree();
        tree.overlapping_tree_entries(page_shape, -1, &[], &self.board.items, &mut entries);
        let ctx = self.board.shape_context();
        let mut cutouts = Vec::new();
        let mut prev_obstacle = TileShape::IntBox(IntBox::EMPTY);
        // PARITY: DrillPage.java:75-97, tree order and the previous-shape rule.
        for entry in entries {
            let Some(item) = self.board.item(entry.item) else {
                continue;
            };
            // `Item.isDrillable`, with the area policy (Q01) on conduction areas.
            if self.board.is_drillable_with_policy(item, self.net) {
                continue;
            }
            if attach_smd && item.pin_drill_allowed() && item.contains_net(self.net) {
                continue;
            }
            let obstacle =
                tree.tree_shape(item, entry.shape_index, &ctx)
                    .ok_or(RoomError::NullPointer {
                        at: "DrillPage.getDrills",
                    })?;
            if !prev_obstacle.contains_shape(&obstacle) {
                let cutout = obstacle.intersection(page_shape);
                if cutout.dimension() == 2 {
                    cutouts.push(cutout);
                }
            }
            prev_obstacle = obstacle;
        }
        Ok(cutouts)
    }

    /// Java `DrillPage.calcPinCenterInDrill` (`:49-61`): the centre of a
    /// drillable pin on `layer` inside `drill_shape`. Of several, the last
    /// in Java's `TreeSet<Item>` order wins, which is the lowest id.
    fn pin_center_in_drill(&self, drill_shape: &TileShape, layer: i32) -> Option<Point> {
        let mut result = None;
        // PARITY: DrillPage.java:52-58, descending ids, the last match wins.
        for id in self.board.overlapping_objects(drill_shape, layer) {
            let Some(item) = self.board.item(id) else {
                continue;
            };
            if !item.pin_drill_allowed() {
                continue;
            }
            let Some(drill) = item.drill() else {
                continue;
            };
            let center = Point::Int(drill.center);
            if drill_shape.contains_inside(&center) {
                result = Some(center);
            }
        }
        result
    }

    /// Java `ExpansionDrill.calculateExpansionRooms` (`:55-92`): finds the
    /// complete room at the drill's location on every layer of its span, or
    /// completes a new one there. False if a layer gives no single room
    /// (an obstacle at the location in the compensated tree).
    ///
    /// The tree is queried once before the layer loop. On each layer the
    /// remaining objects are scanned in `TreeObject` order (rooms first, by
    /// descending id): items are dropped as the scan passes them, and the
    /// first room on the layer is taken and dropped. Rooms completed for an
    /// earlier layer are not in the list.
    pub fn calculate_expansion_rooms(&mut self, drill: DrillId) -> Result<bool, RoomError> {
        let (location, first_layer, last_layer) = {
            let d = self.drill(drill);
            (d.location.clone(), d.first_layer, d.last_layer)
        };
        let search_shape = TileShape::IntBox(TileShape::get_instance_point(&location));
        let mut overlaps: Vec<TreeObject> = self
            .tree()
            .overlapping_objects(&search_shape, -1, &[], &self.board.items)
            .into_iter()
            .collect();
        for layer in first_layer..=last_layer {
            let mut found = self.take_room_on_layer(&mut overlaps, layer)?;
            if found.is_none() {
                let room = self.push_room(Room::Incomplete {
                    shape: None,
                    contained: Some(search_shape.clone()),
                    layer,
                    doors: Vec::new(),
                });
                let new_rooms = self.complete_expansion_room(room)?;
                if new_rooms.is_empty()
                    && self.control.vias_pass_trace_only_areas
                    && self.only_via_passable_obstacles_at(&search_shape, layer)
                {
                    // Q09: the via passes this layer; no room on it.
                    continue;
                }
                if new_rooms.len() != 1 {
                    // An obstacle in the compensated tree at the location.
                    return Ok(false);
                }
                found = Some(new_rooms[0]);
            }
            self.drills[drill.0 as usize].rooms[(layer - first_layer) as usize] = found;
        }
        Ok(true)
    }

    /// True if every trace obstacle of the session's net in the autoroute
    /// tree at `shape` on `layer` lets the net's vias through, and there is
    /// at least one (Q09).
    fn only_via_passable_obstacles_at(&self, shape: &TileShape, layer: i32) -> bool {
        let mut passable = false;
        for object in self
            .tree()
            .overlapping_objects(shape, layer, &[], &self.board.items)
        {
            let TreeObject::Item(id) = object else {
                continue;
            };
            let Some(item) = self.board.item(id) else {
                continue;
            };
            if !self.board.is_trace_obstacle(item, self.net) {
                continue;
            }
            if !self.board.lets_vias_through(item, self.net) {
                return false;
            }
            passable = true;
        }
        passable
    }

    /// One layer of the scan in `calculateExpansionRooms` (`:62-75`).
    fn take_room_on_layer(
        &self,
        overlaps: &mut Vec<TreeObject>,
        layer: i32,
    ) -> Result<Option<RoomId>, RoomError> {
        let mut index = 0;
        // PARITY: ExpansionDrill.java:63-74, non-rooms are removed as the
        // iterator passes them; the first room on the layer is taken.
        while index < overlaps.len() {
            let TreeObject::Room(tree_room) = overlaps[index] else {
                overlaps.remove(index);
                continue;
            };
            let on_layer = self.tree().room(tree_room).map(|r| r.layer);
            if on_layer == Some(layer) {
                overlaps.remove(index);
                return self
                    .room_of_tree_id(tree_room)
                    .map(Some)
                    .ok_or(RoomError::NullPointer {
                        at: "ExpansionDrill.calculateExpansionRooms",
                    });
            }
            index += 1;
        }
        Ok(None)
    }

    /// Java `Via.getAutorouteDrillInfo` (`Via.java:205-218`): the drill of
    /// via `via`, created on first use at the via's centre with the via's
    /// obstacle room on each of its layers. Cached in the via's autoroute
    /// data, so `clear` drops it as Java's `clearAutorouteInfo` does. `None`
    /// if `via` is not a via.
    pub fn via_drill(&mut self, via: ItemId) -> Result<Option<DrillId>, RoomError> {
        if let Some(drill) = self.item_info(via).and_then(|info| info.drill) {
            return Ok(Some(drill));
        }
        let Some((center, first_layer, last_layer)) = self
            .board
            .item(via)
            .filter(|item| item.is_via())
            .and_then(|item| item.drill())
            .map(|d| (Point::Int(d.center), d.first_layer, d.last_layer))
        else {
            return Ok(None);
        };
        // Java gets the autoroute info first, so it exists from here on.
        self.item_info_mut(via);
        let shape = TileShape::IntBox(TileShape::get_instance_point(&center));
        let drill = self.new_drill(shape, center, first_layer, last_layer);
        self.item_info_mut(via).drill = Some(drill);
        for index in 0..(last_layer - first_layer + 1).max(0) as usize {
            let room = self.obstacle_room(via, index)?;
            self.drills[drill.0 as usize].rooms[index] = room;
        }
        Ok(Some(drill))
    }
}

#[cfg(test)]
mod tests {
    //! Unit tests (A03.1, A03.3, A03.4) on a small synthetic board: SMD pins
    //! of net `A` at `U1` and `U2`, a pin of net `B` at `U3`, and a via of
    //! net `A`, 45 degree routing. Coordinates are in board units (10 per
    //! um).

    use std::cell::Cell;

    use freerouting_geometry::{IntPoint, Stoppable};

    use super::*;
    use crate::autoroute::control::AutorouteControl;
    use crate::autoroute::session::TreeRoomId;
    use crate::board::RoutingBoard;
    use crate::job::prepare::{JobInput, RulesChoice, prepare_job};
    use crate::settings::ResolveContext;

    const BOARD: &str = r#"(pcb unit-drills
  (parser (string_quote ") (space_in_quoted_tokens on))
  (resolution um 10)
  (unit um)
  (structure
    (layer F.Cu (type signal) (property (index 0)))
    (layer B.Cu (type signal) (property (index 1)))
    (boundary (path pcb 0  0 0  10000 0  10000 4000  0 4000  0 0))
    (via V600)
    (rule (width 250) (clearance 200))
  )
  (placement
    (component SMD (place U1 2000 1500 front 0) (place U2 8000 1500 front 0)
      (place U3 5000 1500 front 0))
  )
  (library
    (image SMD (pin SmdPad 1 0 0))
    (padstack SmdPad (shape (rect F.Cu -400 -400 400 400)) (attach off))
    (padstack V600 (shape (circle F.Cu 600)) (shape (circle B.Cu 600)) (attach off))
  )
  (network
    (net A (pins U1-1 U2-1))
    (net B (pins U3-1))
    (class kicad_default "" A B (circuit (use_via V600)))
  )
  (wiring
    (via V600 5000 3000 (net A) (type route))
  )
)
"#;

    struct Fixture {
        board: RoutingBoard,
        control: AutorouteControl,
    }

    fn fixture() -> Fixture {
        let mut input = JobInput::new(BOARD.as_bytes().to_vec(), "unit-drills");
        input.rules = RulesChoice::None;
        let prepared = prepare_job(&input, &ResolveContext { cores: 1 }).expect("the board loads");
        let board = prepared.board;
        let net = board
            .rules
            .nets
            .get_by_name("A", 1)
            .expect("net A")
            .net_number;
        let settings = &prepared.board_settings;
        let control = AutorouteControl::for_net(
            &board,
            net,
            settings,
            settings.via_costs(),
            settings.trace_costs(),
        )
        .expect("a control");
        Fixture { board, control }
    }

    fn page_boxes(array: &DrillPageArray, ids: &[DrillPageId]) -> Vec<IntBox> {
        ids.iter().map(|id| array.page(*id).shape).collect()
    }

    fn tile(llx: i32, lly: i32, urx: i32, ury: i32) -> TileShape {
        TileShape::IntBox(IntBox::new(llx, lly, urx, ury))
    }

    #[test]
    fn page_widths_follow_the_via_diameter_with_a_floor_of_10000() {
        assert_eq!(DrillPageArray::max_page_width(6000.0), 30000);
        assert_eq!(DrillPageArray::max_page_width(1999.9), 10000);
        assert_eq!(DrillPageArray::max_page_width(0.0), 10000);
    }

    #[test]
    fn pages_of_a_board_whose_width_is_no_multiple_of_the_page_width() {
        let array = DrillPageArray::new(IntBox::new(0, 0, 25000, 12000), 10000);
        // 25000 / 10000 gives 3 columns of ceil(25000 / 3) = 8334; the last
        // one ends at the board box.
        assert_eq!(
            (
                array.column_count(),
                array.row_count(),
                array.page_width(),
                array.page_height()
            ),
            (3, 2, 8334, 6000)
        );
        let boxes: Vec<IntBox> = array.pages().iter().map(|p| p.shape).collect();
        assert_eq!(
            boxes,
            [
                IntBox::new(0, 0, 8334, 6000),
                IntBox::new(8334, 0, 16668, 6000),
                IntBox::new(16668, 0, 25000, 6000),
                IntBox::new(0, 6000, 8334, 12000),
                IntBox::new(8334, 6000, 16668, 12000),
                IntBox::new(16668, 6000, 25000, 12000),
            ]
        );
        assert!(array.pages().iter().all(|p| p.net_number() == -1));
        assert!(array.pages().iter().all(|p| p.drills().is_none()));
    }

    #[test]
    fn a_shape_ending_on_a_page_boundary_does_not_reach_the_next_page() {
        let array = DrillPageArray::new(IntBox::new(0, 0, 25000, 12000), 10000);
        // max_i = 8334 / 8334 = 1.0 and max_j = 6000 / 6000 = 1.0: the
        // loops stop before column 1 and row 1.
        let ids = array.overlapping_pages(&tile(100, 100, 8334, 6000));
        assert_eq!(ids, [DrillPageId(0)]);
        // Starting on the boundary starts in the next column.
        let ids = array.overlapping_pages(&tile(8334, 100, 9000, 200));
        assert_eq!(ids, [DrillPageId(1)]);
        // Across both boundaries: row by row.
        let ids = array.overlapping_pages(&tile(8000, 5000, 9000, 7000));
        assert_eq!(
            page_boxes(&array, &ids),
            [
                IntBox::new(0, 0, 8334, 6000),
                IntBox::new(8334, 0, 16668, 6000),
                IntBox::new(0, 6000, 8334, 12000),
                IntBox::new(8334, 6000, 16668, 12000),
            ]
        );
        // A shape reaching past the board box is clipped to it.
        let ids = array.overlapping_pages(&tile(20000, 11000, 40000, 40000));
        assert_eq!(ids, [DrillPageId(5)]);
        // A shape outside the board box overlaps no page.
        assert!(
            array
                .overlapping_pages(&tile(30000, 0, 40000, 100))
                .is_empty()
        );
    }

    #[test]
    fn an_empty_board_has_no_pages() {
        let array = DrillPageArray::new(IntBox::new(0, 0, 0, 0), 10000);
        assert!(array.pages().is_empty());
        assert!(array.overlapping_pages(&tile(0, 0, 10, 10)).is_empty());
    }

    /// A complete free room in the session's list and the tree (what
    /// `add_complete_room` does without the neighbour calculation).
    fn tree_room(s: &mut Session<'_>, id: i32, shape: TileShape, layer: i32) -> RoomId {
        let room = s.detached_free_room(shape.clone(), layer, id);
        s.complete.push(room);
        s.by_java_id.insert(id, room);
        s.board
            .insert_room(s.tree_class, TreeRoomId(id), shape, layer);
        room
    }

    #[test]
    fn a_drill_takes_the_first_room_of_each_layer_in_tree_object_order() {
        let mut f = fixture();
        let mut s = Session::new(&mut f.board, &f.control, None, None);
        let around = || tile(60000, 34000, 70000, 38000);
        // Rooms come first in descending id: room 7 (layer 1) is scanned
        // before the wanted room 5 on layer 0, and room 5 before room 3.
        let low = tree_room(&mut s, 3, around(), 0);
        let wanted = tree_room(&mut s, 5, around(), 0);
        let other_layer = tree_room(&mut s, 7, around(), 1);
        let location = Point::Int(IntPoint::new(65000, 36000));
        let shape = TileShape::IntBox(TileShape::get_instance_point(&location));
        let drill = s.new_drill(shape, location, 0, 1);
        assert!(s.calculate_expansion_rooms(drill).expect("rooms"));
        assert_eq!(s.drill(drill).rooms, [Some(wanted), Some(other_layer)]);
        assert_ne!(s.drill(drill).rooms[0], Some(low));
        assert_eq!(s.room_count(), 0, "no room was completed");
        assert_eq!(s.drill_sections(drill).len(), 2);
    }

    #[test]
    fn a_drill_completes_a_room_where_a_layer_has_none() {
        let mut f = fixture();
        let mut s = Session::new(&mut f.board, &f.control, None, None);
        let on_layer_1 = tree_room(&mut s, 9, tile(60000, 34000, 70000, 38000), 1);
        let location = Point::Int(IntPoint::new(65000, 36000));
        let shape = TileShape::IntBox(TileShape::get_instance_point(&location));
        let drill = s.new_drill(shape, location, 0, 1);
        assert!(s.calculate_expansion_rooms(drill).expect("rooms"));
        let rooms = s.drill(drill).rooms.clone();
        assert_eq!(rooms[1], Some(on_layer_1));
        let completed = rooms[0].expect("a room on layer 0");
        let Room::CompleteFree { layer, shape, .. } = s.room(completed) else {
            panic!("a complete free room");
        };
        assert_eq!(*layer, 0);
        assert!(shape.contains(&Point::Int(IntPoint::new(65000, 36000))));
        assert!(s.complete_rooms().contains(&completed));
    }

    #[test]
    fn a_drill_on_a_foreign_pin_fails() {
        let mut f = fixture();
        let mut s = Session::new(&mut f.board, &f.control, None, None);
        // The centre of U3 (net B) on F.Cu.
        let location = Point::Int(IntPoint::new(50000, 15000));
        let shape = TileShape::IntBox(TileShape::get_instance_point(&location));
        let drill = s.new_drill(shape, location, 0, 1);
        assert!(!s.calculate_expansion_rooms(drill).expect("no error"));
    }

    fn all_page_drills(s: &mut Session<'_>, attach_smd: bool) -> Vec<Vec<DrillId>> {
        let count = s.drill_pages().pages().len();
        (0..count)
            .map(|page| {
                s.page_drills(DrillPageId(page as u32), attach_smd)
                    .expect("drills")
                    .to_vec()
            })
            .collect()
    }

    #[test]
    fn page_drills_avoid_obstacles_and_attach_to_own_smd_pins() {
        let mut f = fixture();
        let mut s = Session::new(&mut f.board, &f.control, None, None);
        // Via V600 is 6000 wide: pages of at most 30000.
        let array = s.drill_pages();
        assert_eq!((array.column_count(), array.row_count()), (4, 2));
        let u1_center = Point::Int(IntPoint::new(20000, 15000));
        let plain = all_page_drills(&mut s, false);
        assert!(plain.iter().all(|drills| !drills.is_empty()));
        for &drill in plain.iter().flatten() {
            let d = s.drill(drill);
            assert_eq!((d.first_layer, d.last_layer), (0, 1));
            assert!(d.rooms.iter().all(Option::is_some));
            assert_ne!(d.location, u1_center, "U1 is an obstacle");
        }
        // Another net forces a recalculation: the pages now keep U1 out of
        // the obstacles and place a drill on its centre.
        s.net = -2;
        let _ = all_page_drills(&mut s, true);
        s.net = f.control.net_number;
        let attached = all_page_drills(&mut s, true);
        let on_u1 = attached[0]
            .iter()
            .filter(|d| s.drill(**d).location == u1_center)
            .count();
        assert_eq!(on_u1, 1);
        assert_eq!(
            s.drill_pages().page(DrillPageId(0)).net_number(),
            f.control.net_number
        );
    }

    #[test]
    fn page_drills_are_cached_until_invalidated() {
        let mut f = fixture();
        let mut s = Session::new(&mut f.board, &f.control, None, None);
        let first = s
            .page_drills(DrillPageId(0), false)
            .expect("drills")
            .to_vec();
        let count = s.drill_count();
        let again = s
            .page_drills(DrillPageId(0), false)
            .expect("drills")
            .to_vec();
        assert_eq!(first, again);
        assert_eq!(s.drill_count(), count, "nothing recalculated");
        // A shape over page 0 only invalidates page 0.
        let _ = s.page_drills(DrillPageId(1), false).expect("drills");
        s.invalidate_drill_pages(&tile(1000, 1000, 2000, 2000));
        let pages = s.built_drill_pages().expect("built");
        assert!(pages.page(DrillPageId(0)).drills().is_none());
        assert!(pages.page(DrillPageId(1)).drills().is_some());
        let recalculated = s
            .page_drills(DrillPageId(0), false)
            .expect("drills")
            .to_vec();
        // New drills; the rooms of the first calculation are in the tree
        // now, so a location whose completion failed may find its rooms.
        assert!(recalculated.len() >= first.len());
        assert!(recalculated.iter().all(|d| d.0 as usize >= count));
        // `clear` drops the pages with the rooms.
        s.clear();
        assert!(s.built_drill_pages().is_none());
        assert_eq!(s.drill_count(), 0);
    }

    #[test]
    fn a_carried_engine_keeps_its_drills_and_occupied_sections() {
        let mut f = fixture();
        let mut s = Session::new(&mut f.board, &f.control, None, None);
        let drills = s
            .page_drills(DrillPageId(0), false)
            .expect("drills")
            .to_vec();
        s.drill_sections_mut(drills[0])[1].occupied = true;
        let count = s.drill_count();
        let carry = s.take_drill_carry();
        assert!(s.built_drill_pages().is_none(), "the session keeps none");
        s.clear();
        drop(s);
        let mut next = Session::new(&mut f.board, &f.control, None, None);
        next.install_drill_carry(carry);
        let again = next
            .page_drills(DrillPageId(0), false)
            .expect("drills")
            .to_vec();
        // The same net: page 0 is not recalculated.
        assert_eq!(again, drills);
        assert_eq!(next.drill_count(), count);
        let flags: Vec<bool> = next
            .drill_sections(drills[0])
            .iter()
            .map(|s| s.occupied)
            .collect();
        assert_eq!(flags, [false, true]);
        assert!(next.drill(drills[0]).rooms.iter().all(Option::is_none));
        drop(next);
        // An engine that never built its pages carries nothing.
        let mut s = Session::new(&mut f.board, &f.control, None, None);
        let carry = s.take_drill_carry();
        s.install_drill_carry(carry);
        assert!(s.built_drill_pages().is_none());
    }

    struct AlwaysStop(Cell<bool>);

    impl Stoppable for AlwaysStop {
        fn request_stop(&self) {
            self.0.set(true);
        }
        fn is_stop_requested(&self) -> bool {
            self.0.get()
        }
    }

    #[test]
    fn a_stopped_split_leaves_the_page_empty_for_the_net() {
        let mut f = fixture();
        let stop = AlwaysStop(Cell::new(true));
        let mut s = Session::new(&mut f.board, &f.control, Some(&stop), None);
        // Page 0 holds U1, so its split has a hole to cut and checks the stop.
        assert!(
            s.page_drills(DrillPageId(0), false)
                .expect("drills")
                .is_empty()
        );
        stop.0.set(false);
        assert!(
            s.page_drills(DrillPageId(0), false)
                .expect("drills")
                .is_empty(),
            "the page is not recalculated for the same net"
        );
        let page = s.drill_pages().page(DrillPageId(0));
        assert_eq!(page.net_number(), f.control.net_number);
        assert_eq!(page.drills(), Some(&[][..]));
    }

    #[test]
    fn resetting_a_page_resets_its_drills_and_its_sections() {
        let mut f = fixture();
        let mut s = Session::new(&mut f.board, &f.control, None, None);
        let drills = s
            .page_drills(DrillPageId(0), false)
            .expect("drills")
            .to_vec();
        assert_eq!(s.page_sections(DrillPageId(0)).len(), 2);
        s.page_sections_mut(DrillPageId(0))[1].occupied = true;
        for &drill in &drills {
            s.drill_sections_mut(drill)[0].occupied = true;
        }
        let other = s.page_drills(DrillPageId(1), false).expect("drills")[0];
        s.drill_sections_mut(other)[1].occupied = true;
        s.reset_drill_page(DrillPageId(0));
        assert!(s.page_sections(DrillPageId(0)).iter().all(|x| !x.occupied));
        for &drill in &drills {
            assert!(s.drill_sections(drill).iter().all(|x| !x.occupied));
        }
        assert!(s.drill_sections(other)[1].occupied, "page 1 untouched");
        s.reset_drill_pages();
        assert!(!s.drill_sections(other)[1].occupied);
    }

    #[test]
    fn a_via_drill_is_created_once_and_dropped_with_the_autoroute_data() {
        let mut f = fixture();
        let via = f
            .board
            .items()
            .find(|item| item.is_via())
            .expect("the via")
            .id();
        let pin = f
            .board
            .items()
            .find(|item| item.is_pin())
            .expect("a pin")
            .id();
        let mut s = Session::new(&mut f.board, &f.control, None, None);
        assert!(s.item_info(via).is_none(), "created on demand");
        let drill = s.via_drill(via).expect("drill").expect("a via");
        assert_eq!(s.via_drill(via).expect("drill"), Some(drill), "cached");
        let d = s.drill(drill).clone();
        assert_eq!(d.location, Point::Int(IntPoint::new(50000, 30000)));
        assert_eq!((d.first_layer, d.last_layer), (0, 1));
        assert_eq!(
            d.shape.bounding_box(),
            TileShape::get_instance_point(&d.location)
        );
        for (index, room) in d.rooms.iter().enumerate() {
            let room = room.expect("an obstacle room per layer");
            assert_eq!(s.obstacle_room(via, index).expect("room"), Some(room));
            assert!(matches!(s.room(room), Room::Obstacle { item, .. } if *item == via));
        }
        assert_eq!(s.via_drill(pin).expect("no error"), None, "not a via");
        s.drill_sections_mut(drill)[0].occupied = true;
        s.reset_drill(drill);
        assert!(!s.drill_sections(drill)[0].occupied);
        s.clear();
        assert!(s.item_info(via).is_none());
        let again = s.via_drill(via).expect("drill").expect("a via");
        assert_eq!(s.drill_count(), 1, "a new drill after clear");
        assert_eq!(s.drill(again).location, d.location);
    }
}
