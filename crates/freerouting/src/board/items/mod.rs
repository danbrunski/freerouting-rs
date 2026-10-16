//! The items on a board: pins, vias, traces, areas and outlines.
//!
//! Port of `board.model.items` (and `board.model.structure.BoardOutline`,
//! `board.trace.PolylineTrace`). Java models the items as a class tree with
//! back-references to the board; here an [`Item`] holds the data common to
//! all items and an [`ItemKind`] with the data of its class. Everything that
//! needs the rest of the board (contacts, tree shapes, obstacle tests that
//! look at the library) is a method of [`crate::board::BasicBoard`] taking an
//! [`ItemId`].
//!
//! Java computes derived data (pin centers and shapes, absolute keepout
//! areas, drill layers) lazily and caches it in the item; the port computes
//! it when the item is created and whenever its placement changes.

use std::cmp::Ordering;
use std::fmt;

use freerouting_geometry::{
    Area, FloatPoint, IntBox, IntPoint, Point, Polyline, PolylineShape, PolylineShapeOps, Shape,
    TileShape, Vector,
};

use crate::board::structure::FixedState;
use crate::library::PadstackId;

/// The id of an item.
///
/// Java orders items by `Item.compareTo`, which compares ids in descending
/// order; `ItemId` orders the same way, so ordered collections of ids
/// (`BTreeSet<ItemId>`, the item list) iterate like Java's `TreeSet<Item>`:
/// highest id first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ItemId(pub i32);

impl Ord for ItemId {
    fn cmp(&self, other: &Self) -> Ordering {
        other.0.cmp(&self.0)
    }
}

impl PartialOrd for ItemId {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for ItemId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{}", self.0)
    }
}

/// The statistics category of an item.
///
/// Port of `board.model.items.BoardItemType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum BoardItemType {
    Pin,
    Via,
    Trace,
    ConductionArea,
    ViaObstacleArea,
    ComponentObstacleArea,
    ObstacleArea,
    ComponentOutline,
    BoardOutline,
    Other,
}

/// Where [`crate::board::BasicBoard::get_connection_items`] stops.
///
/// Port of `Item.StopConnectionOption`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StopConnectionOption {
    None,
    FanoutVia,
    Via,
}

/// The data of a pin or via that depends on its padstack and placement.
///
/// Port of the state of `board.model.items.DrillItem`.
#[derive(Clone, Debug)]
pub struct DrillData {
    pub center: IntPoint,
    pub padstack: PadstackId,
    /// The first board layer of the drill item.
    pub first_layer: i32,
    /// The last board layer of the drill item.
    pub last_layer: i32,
    /// The pad shape on board layer `first_layer + i`, for `i` in
    /// `0..tile_shape_count`. Its length is the padstack's layer span.
    pub shapes: Vec<Option<Shape>>,
}

impl DrillData {
    /// Java `DrillItem.shapeLayer`.
    pub fn shape_layer(&self, index: usize) -> i32 {
        let index = (index as i32).min(self.last_layer - self.first_layer);
        self.first_layer + index.max(0)
    }

    /// The pad shape with index `index`.
    pub fn shape(&self, index: usize) -> Option<&Shape> {
        self.shapes.get(index)?.as_ref()
    }

    /// The pad shape on board layer `layer`, if the item reaches it.
    pub fn shape_on_layer(&self, layer: i32) -> Option<&Shape> {
        if layer < self.first_layer || layer > self.last_layer {
            return None;
        }
        self.shape((layer - self.first_layer) as usize)
    }

    /// The union of the bounding boxes of the pad shapes.
    pub fn bounding_box(&self) -> IntBox {
        let mut result = IntBox::EMPTY;
        for shape in self.shapes.iter().flatten() {
            result = result.union_box(&shape.bounding_box());
        }
        result
    }

    /// Java `DrillItem.smallestRadius`.
    pub fn smallest_radius(&self) -> f64 {
        let c = self.center.to_float();
        let mut result = f64::MAX;
        for shape in self.shapes.iter().flatten() {
            result = result.min(shape.border_distance(c));
        }
        result
    }
}

/// A pin of a placed component.
///
/// Port of `board.model.items.Pin`.
#[derive(Clone, Debug)]
pub struct Pin {
    /// The index of the pin in the component's package.
    pub pin_index: usize,
    pub drill: DrillData,
    /// The pin this pin's net was swapped to (Java `changedTo`).
    pub changed_to: ItemId,
}

/// A via.
///
/// Port of `board.model.items.Via`.
#[derive(Clone, Debug)]
pub struct Via {
    pub attach_allowed: bool,
    pub is_escape_via: bool,
    pub escape_via_smd_layer: i32,
    pub drill: DrillData,
}

/// A trace whose geometry is a polyline.
///
/// Port of `board.model.items.Trace` and `board.trace.PolylineTrace`.
#[derive(Clone, Debug)]
pub struct PolylineTrace {
    pub polyline: Polyline,
    pub layer: i32,
    pub half_width: i32,
}

impl PolylineTrace {
    /// The intersection of the first two lines.
    pub fn first_corner(&self) -> Point {
        self.polyline.corner(0)
    }

    /// The intersection of the last two lines.
    pub fn last_corner(&self) -> Point {
        self.polyline.corner(self.polyline.lines().len() - 2)
    }

    /// The number of corners: lines minus one.
    pub fn corner_count(&self) -> usize {
        self.polyline.lines().len().saturating_sub(1)
    }

    /// The approximate length.
    pub fn length(&self) -> f64 {
        self.polyline.length_approx()
    }

    /// The bounding box of the polyline, enlarged by the half width.
    pub fn bounding_box(&self) -> IntBox {
        self.polyline
            .bounding_box()
            .offset(f64::from(self.half_width))
    }

    /// The number of segments: lines minus two.
    pub fn tile_shape_count(&self) -> usize {
        self.polyline.lines().len().saturating_sub(2)
    }
}

/// The class of an obstacle or conduction area.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AreaKind {
    /// `ObstacleArea`: a keepout for traces and vias.
    Obstacle,
    /// `ViaObstacleArea`: a keepout for vias.
    ViaObstacle,
    /// `ComponentObstacleArea`: a keepout for component placement.
    ComponentObstacle,
    /// `ConductionArea`: copper of a net.
    Conduction { is_obstacle: bool, is_filled: bool },
}

/// A keepout or a conduction area.
///
/// Port of `board.model.items.ObstacleArea` and its subclasses
/// `ViaObstacleArea`, `ComponentObstacleArea` and `ConductionArea`.
#[derive(Clone, Debug)]
pub struct ObstacleArea {
    pub kind: AreaKind,
    pub name: Option<String>,
    relative_area: Area,
    layer: i32,
    translation: Vector,
    rotation_in_degree: f64,
    side_changed: bool,
    /// The area on the board (Java `precalculatedAbsoluteArea`).
    area: Area,
}

impl ObstacleArea {
    pub(crate) fn new(
        kind: AreaKind,
        relative_area: Area,
        layer: i32,
        translation: Vector,
        rotation_in_degree: f64,
        side_changed: bool,
        name: Option<String>,
    ) -> Self {
        ObstacleArea {
            kind,
            name,
            area: relative_area.clone(),
            relative_area,
            layer,
            translation,
            rotation_in_degree,
            side_changed,
        }
    }

    /// The area in board coordinates.
    pub fn area(&self) -> &Area {
        &self.area
    }

    /// The area relative to the translation and rotation.
    pub fn relative_area(&self) -> &Area {
        &self.relative_area
    }

    pub fn layer(&self) -> i32 {
        self.layer
    }

    pub fn translation(&self) -> &Vector {
        &self.translation
    }

    pub fn rotation_in_degree(&self) -> f64 {
        self.rotation_in_degree
    }

    pub fn side_changed(&self) -> bool {
        self.side_changed
    }

    /// True for a `ConductionArea`.
    pub fn is_conduction_area(&self) -> bool {
        matches!(self.kind, AreaKind::Conduction { .. })
    }

    /// Java `ConductionArea.getIsObstacle`; false for other areas.
    pub fn conduction_is_obstacle(&self) -> bool {
        matches!(
            self.kind,
            AreaKind::Conduction {
                is_obstacle: true,
                ..
            }
        )
    }

    /// Recomputes the absolute area of an area that belongs to no component.
    pub(crate) fn update_area(&mut self, flip_style_rotate_first: bool) {
        self.area = transform_free_area(
            &self.relative_area,
            self.side_changed,
            self.rotation_in_degree,
            &self.translation,
            flip_style_rotate_first,
        );
    }

    /// Sets the absolute area of an area that belongs to a component.
    pub(crate) fn set_area(&mut self, area: Area) {
        self.area = area;
    }

    pub fn translate_by(&mut self, vector: &Vector) {
        self.translation = self.translation.add(vector);
    }

    pub fn turn_90_degree(&mut self, factor: i32, pole: IntPoint) {
        self.rotation_in_degree =
            normalize_degree(self.rotation_in_degree + f64::from(factor * 90));
        let rel_location = Point::ZERO.translate_by(&self.translation);
        self.translation = rel_location
            .turn_90_degree(factor, &Point::Int(pole))
            .difference_by(&Point::ZERO);
    }

    pub fn rotate_approx(
        &mut self,
        angle_in_degree: f64,
        pole: FloatPoint,
        flip_style_rotate_first: bool,
    ) {
        let mut turn_angle = angle_in_degree;
        if self.side_changed && flip_style_rotate_first {
            turn_angle = 360.0 - angle_in_degree;
        }
        self.rotation_in_degree = normalize_degree(self.rotation_in_degree + turn_angle);
        let new_translation = self
            .translation
            .to_float()
            .rotate(angle_in_degree.to_radians(), pole);
        self.translation = Point::Int(new_translation.round()).difference_by(&Point::ZERO);
    }

    pub fn change_placement_side(&mut self, pole: IntPoint, layer_count: usize) {
        self.side_changed = !self.side_changed;
        self.layer = layer_count as i32 - self.layer - 1;
        let rel_location = Point::ZERO.translate_by(&self.translation);
        self.translation = rel_location
            .mirror_vertical(&Point::Int(pole))
            .difference_by(&Point::ZERO);
    }

    pub(crate) fn set_kind(&mut self, kind: AreaKind) {
        self.kind = kind;
    }
}

/// An outline line of a component (courtyard, fabrication or silkscreen).
///
/// Port of `board.model.items.ComponentOutline`.
#[derive(Clone, Debug)]
pub struct ComponentOutline {
    relative_area: Area,
    is_front: bool,
    translation: Vector,
    rotation_in_degree: f64,
    pub is_courtyard: bool,
    pub is_fabrication: bool,
    pub is_closed: bool,
    layer_count: usize,
    area: Area,
}

impl ComponentOutline {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        relative_area: Area,
        is_front: bool,
        translation: Vector,
        rotation_in_degree: f64,
        is_courtyard: bool,
        is_fabrication: bool,
        is_closed: bool,
        layer_count: usize,
    ) -> Self {
        ComponentOutline {
            area: relative_area.clone(),
            relative_area,
            is_front,
            translation,
            rotation_in_degree,
            is_courtyard,
            is_fabrication,
            is_closed,
            layer_count,
        }
    }

    pub fn is_front(&self) -> bool {
        self.is_front
    }

    /// The outer layer of the component's side.
    pub fn layer(&self) -> i32 {
        if self.is_front {
            0
        } else {
            self.layer_count as i32 - 1
        }
    }

    /// The outline in board coordinates.
    pub fn area(&self) -> &Area {
        &self.area
    }

    pub fn relative_area(&self) -> &Area {
        &self.relative_area
    }

    pub fn translation(&self) -> &Vector {
        &self.translation
    }

    pub fn rotation_in_degree(&self) -> f64 {
        self.rotation_in_degree
    }

    pub(crate) fn update_area(&mut self, flip_style_rotate_first: bool) {
        self.area = transform_free_area(
            &self.relative_area,
            !self.is_front,
            self.rotation_in_degree,
            &self.translation,
            flip_style_rotate_first,
        );
    }

    pub(crate) fn set_area(&mut self, area: Area) {
        self.area = area;
    }

    pub fn translate_by(&mut self, vector: &Vector) {
        self.translation = self.translation.add(vector);
    }

    pub fn turn_90_degree(&mut self, factor: i32, pole: IntPoint) {
        self.rotation_in_degree =
            normalize_degree(self.rotation_in_degree + f64::from(factor * 90));
        let rel_location = Point::ZERO.translate_by(&self.translation);
        self.translation = rel_location
            .turn_90_degree(factor, &Point::Int(pole))
            .difference_by(&Point::ZERO);
    }

    pub fn rotate_approx(
        &mut self,
        angle_in_degree: f64,
        pole: FloatPoint,
        flip_style_rotate_first: bool,
    ) {
        let mut turn_angle = angle_in_degree;
        if !self.is_front && flip_style_rotate_first {
            turn_angle = 360.0 - angle_in_degree;
        }
        self.rotation_in_degree = normalize_degree(self.rotation_in_degree + turn_angle);
        let new_translation = self
            .translation
            .to_float()
            .rotate(angle_in_degree.to_radians(), pole);
        self.translation = Point::Int(new_translation.round()).difference_by(&Point::ZERO);
    }

    pub fn change_placement_side(&mut self, pole: IntPoint) {
        self.is_front = !self.is_front;
        let rel_location = Point::ZERO.translate_by(&self.translation);
        self.translation = rel_location
            .mirror_vertical(&Point::Int(pole))
            .difference_by(&Point::ZERO);
    }
}

/// The outline of the board.
///
/// Port of `board.model.structure.BoardOutline`.
#[derive(Clone, Debug)]
pub struct BoardOutline {
    shapes: Vec<PolylineShape>,
    /// The area between the board's bounding box and the outline, when the
    /// keepout outside the outline is generated.
    keepout_area: Option<Area>,
    keepout_outside_outline: bool,
    /// The number of convex pieces of the keepout area.
    keepout_piece_count: usize,
    layer_count: usize,
}

impl BoardOutline {
    /// The half width of the outline lines.
    pub const HALF_WIDTH: i32 = 100;

    pub(crate) fn new(shapes: Vec<PolylineShape>, layer_count: usize) -> Self {
        BoardOutline {
            shapes,
            keepout_area: None,
            keepout_outside_outline: false,
            keepout_piece_count: 0,
            layer_count,
        }
    }

    /// The number of outline shapes.
    pub fn shape_count(&self) -> usize {
        self.shapes.len()
    }

    /// The outline shape `index`.
    pub fn shape(&self, index: usize) -> Option<&PolylineShape> {
        self.shapes.get(index)
    }

    /// All outline shapes.
    pub fn shapes(&self) -> &[PolylineShape] {
        &self.shapes
    }

    /// The total number of border lines of all shapes.
    pub fn line_count(&self) -> usize {
        self.shapes.iter().map(|s| s.border_line_count()).sum()
    }

    /// Whether the area outside the outline is a keepout.
    pub fn keepout_outside_outline_generated(&self) -> bool {
        self.keepout_outside_outline
    }

    /// The keepout area between `bounding_box` and the outline.
    pub fn keepout_area(&self) -> Option<&Area> {
        self.keepout_area.as_ref()
    }

    /// Java `generateKeepoutOutside` without the tree update, which the
    /// board does.
    pub(crate) fn set_keepout_outside(&mut self, value: bool, bounding_box: IntBox) {
        self.keepout_outside_outline = value;
        if value && self.keepout_area.is_none() {
            let area = Area::PolylineArea(freerouting_geometry::PolylineArea::new(
                PolylineShape::from(bounding_box),
                self.shapes.clone(),
            ));
            self.keepout_piece_count = area.split_to_convex().map_or(0, |pieces| pieces.len());
            self.keepout_area = Some(area);
        }
    }

    /// Java `BoardOutline.tileShapeCount`.
    pub fn tile_shape_count(&self) -> usize {
        if self.keepout_outside_outline {
            self.keepout_piece_count * self.layer_count
        } else {
            self.line_count() * self.layer_count
        }
    }

    /// Java `BoardOutline.shapeLayer`.
    pub fn shape_layer(&self, index: usize) -> i32 {
        (index * self.layer_count)
            .checked_div(self.tile_shape_count())
            .map_or(0, |layer| layer as i32)
    }

    /// The union of the bounding boxes of the shapes.
    pub fn bounding_box(&self) -> IntBox {
        let mut result = IntBox::EMPTY;
        for shape in &self.shapes {
            result = result.union_box(&shape.bounding_box());
        }
        result
    }

    /// True if a shape contains `point`.
    pub fn contains(&self, point: &Point) -> bool {
        self.shapes.iter().any(|s| s.contains(point))
    }

    /// True if a shape contains `point`.
    pub fn contains_float(&self, point: FloatPoint) -> bool {
        self.shapes.iter().any(|s| s.contains_float(point))
    }

    pub fn layer_count(&self) -> usize {
        self.layer_count
    }

    /// Java `BoardOutline.minimumPinGap`: the smallest gap, in board units,
    /// between the copper of a pin and the outline lines, over `pin_shapes`
    /// (the pad shape of every pin on every layer from its first to its
    /// last). 0 if a pad corner is not inside the outline, `+∞` without
    /// shapes.
    ///
    /// Pad corners are sampled, so the result is exact for polygonal pads
    /// and never larger than the true gap for round pads, which are
    /// measured from their bounding box.
    pub fn minimum_pin_gap<'a>(&self, pin_shapes: impl IntoIterator<Item = &'a Shape>) -> f64 {
        let mut result = f64::INFINITY;
        for shape in pin_shapes {
            result = result.min(self.corner_gap(shape));
            if result <= 0.0 {
                return 0.0;
            }
        }
        result
    }

    /// Java `BoardOutline.cornerGap`.
    fn corner_gap(&self, pad_shape: &Shape) -> f64 {
        let mut result = f64::INFINITY;
        if pad_shape.is_empty() || !pad_shape.is_bounded() {
            return result;
        }
        match pad_shape {
            Shape::Polyline(polyline) => {
                for c in 0..polyline.border_line_count() {
                    result = result.min(self.distance_to_outline(polyline.corner_approx(c)));
                }
            }
            other => {
                let bounds = other.bounding_box();
                for c in 0..4 {
                    result = result.min(self.distance_to_outline(bounds.corner(c).to_float()));
                }
            }
        }
        result
    }

    /// Java `BoardOutline.distanceToOutline`: the distance to the outline
    /// lines, 0 for points that are not inside the outline.
    fn distance_to_outline(&self, point: FloatPoint) -> f64 {
        if !self.contains_float(point) {
            return 0.0;
        }
        let mut result = f64::INFINITY;
        for shape in &self.shapes {
            let corner_count = shape.border_line_count();
            for i in 0..corner_count {
                let from = shape.corner(i).to_float();
                let to = shape.corner((i + 1) % corner_count).to_float();
                result = result
                    .min(freerouting_geometry::FloatLine::new(from, to).segment_distance(point));
            }
        }
        result
    }
}

/// The class-specific data of an item.
#[derive(Clone, Debug)]
pub enum ItemKind {
    Pin(Pin),
    Via(Via),
    Trace(PolylineTrace),
    /// An `ObstacleArea`, `ViaObstacleArea`, `ComponentObstacleArea` or
    /// `ConductionArea`, told apart by [`ObstacleArea::kind`].
    Area(ObstacleArea),
    ComponentOutline(ComponentOutline),
    BoardOutline(BoardOutline),
}

/// An item on the board.
///
/// Port of `board.model.items.Item`.
#[derive(Clone, Debug)]
pub struct Item {
    id: ItemId,
    pub(crate) net_numbers: Vec<i32>,
    pub(crate) clearance_class: i32,
    pub(crate) component_id: i32,
    pub(crate) fixed_state: FixedState,
    pub(crate) on_the_board: bool,
    pub kind: ItemKind,
}

impl Item {
    pub(crate) fn new(
        id: ItemId,
        net_numbers: Vec<i32>,
        clearance_class: i32,
        component_id: i32,
        fixed_state: FixedState,
        kind: ItemKind,
    ) -> Self {
        Item {
            id,
            net_numbers,
            clearance_class,
            component_id,
            fixed_state,
            on_the_board: false,
            kind,
        }
    }

    pub fn id(&self) -> ItemId {
        self.id
    }

    /// The nets of the item.
    pub fn net_numbers(&self) -> &[i32] {
        &self.net_numbers
    }

    pub fn net_count(&self) -> usize {
        self.net_numbers.len()
    }

    pub fn net_number(&self, index: usize) -> i32 {
        self.net_numbers[index]
    }

    pub fn clearance_class(&self) -> i32 {
        self.clearance_class
    }

    /// The component the item belongs to, or 0.
    pub fn component_id(&self) -> i32 {
        self.component_id
    }

    pub fn fixed_state(&self) -> FixedState {
        self.fixed_state
    }

    pub fn is_on_the_board(&self) -> bool {
        self.on_the_board
    }

    /// The statistics category.
    pub fn board_item_type(&self) -> BoardItemType {
        match &self.kind {
            ItemKind::Pin(_) => BoardItemType::Pin,
            ItemKind::Via(_) => BoardItemType::Via,
            ItemKind::Trace(_) => BoardItemType::Trace,
            ItemKind::Area(area) => match area.kind {
                AreaKind::Conduction { .. } => BoardItemType::ConductionArea,
                AreaKind::ViaObstacle => BoardItemType::ViaObstacleArea,
                AreaKind::ComponentObstacle => BoardItemType::ComponentObstacleArea,
                AreaKind::Obstacle => BoardItemType::ObstacleArea,
            },
            ItemKind::ComponentOutline(_) => BoardItemType::ComponentOutline,
            ItemKind::BoardOutline(_) => BoardItemType::BoardOutline,
        }
    }

    pub fn as_pin(&self) -> Option<&Pin> {
        match &self.kind {
            ItemKind::Pin(pin) => Some(pin),
            _ => None,
        }
    }

    pub fn as_via(&self) -> Option<&Via> {
        match &self.kind {
            ItemKind::Via(via) => Some(via),
            _ => None,
        }
    }

    pub fn as_trace(&self) -> Option<&PolylineTrace> {
        match &self.kind {
            ItemKind::Trace(trace) => Some(trace),
            _ => None,
        }
    }

    /// The area of an `ObstacleArea` or any subclass.
    pub fn as_area(&self) -> Option<&ObstacleArea> {
        match &self.kind {
            ItemKind::Area(area) => Some(area),
            _ => None,
        }
    }

    /// The area of a `ConductionArea`.
    pub fn as_conduction_area(&self) -> Option<&ObstacleArea> {
        self.as_area().filter(|a| a.is_conduction_area())
    }

    pub fn as_component_outline(&self) -> Option<&ComponentOutline> {
        match &self.kind {
            ItemKind::ComponentOutline(outline) => Some(outline),
            _ => None,
        }
    }

    pub fn as_board_outline(&self) -> Option<&BoardOutline> {
        match &self.kind {
            ItemKind::BoardOutline(outline) => Some(outline),
            _ => None,
        }
    }

    /// The drill data of a pin or via.
    pub fn drill(&self) -> Option<&DrillData> {
        match &self.kind {
            ItemKind::Pin(pin) => Some(&pin.drill),
            ItemKind::Via(via) => Some(&via.drill),
            _ => None,
        }
    }

    pub fn drill_mut(&mut self) -> Option<&mut DrillData> {
        match &mut self.kind {
            ItemKind::Pin(pin) => Some(&mut pin.drill),
            ItemKind::Via(via) => Some(&mut via.drill),
            _ => None,
        }
    }

    pub fn is_pin(&self) -> bool {
        matches!(self.kind, ItemKind::Pin(_))
    }

    pub fn is_via(&self) -> bool {
        matches!(self.kind, ItemKind::Via(_))
    }

    pub fn is_trace(&self) -> bool {
        matches!(self.kind, ItemKind::Trace(_))
    }

    pub fn is_conduction_area(&self) -> bool {
        self.as_conduction_area().is_some()
    }

    /// Java `instanceof DrillItem`.
    pub fn is_drill_item(&self) -> bool {
        self.drill().is_some()
    }

    /// Java `instanceof Connectable`: pins, vias, traces and conduction areas.
    pub fn is_connectable_kind(&self) -> bool {
        self.is_drill_item() || self.is_trace() || self.is_conduction_area()
    }

    /// True if the item belongs to net `net_number` (which must be positive).
    pub fn contains_net(&self, net_number: i32) -> bool {
        net_number > 0 && self.net_numbers.contains(&net_number)
    }

    /// Java `Item.isObstacle(int)` (the `SearchTreeObject` version).
    pub fn is_obstacle_for_net(&self, net_number: i32) -> bool {
        !self.contains_net(net_number)
    }

    /// True if the items have a net in common.
    pub fn shares_net(&self, other: &Item) -> bool {
        self.shares_net_no(&other.net_numbers)
    }

    /// True if the item has a net of `net_numbers`.
    pub fn shares_net_no(&self, net_numbers: &[i32]) -> bool {
        self.net_numbers.iter().any(|n| net_numbers.contains(n))
    }

    /// True if all nets are normal nets.
    pub fn nets_normal(&self) -> bool {
        self.net_numbers
            .iter()
            .all(|n| crate::rules::Nets::is_normal_net_number(*n))
    }

    /// True if the items have the same nets.
    pub fn nets_equal(&self, other: &Item) -> bool {
        self.nets_equal_no(&other.net_numbers)
    }

    /// True if the item has exactly the nets `net_numbers`.
    pub fn nets_equal_no(&self, net_numbers: &[i32]) -> bool {
        self.net_numbers.len() == net_numbers.len()
            && net_numbers.iter().all(|n| self.contains_net(*n))
    }

    /// The first layer of the item.
    pub fn first_layer(&self) -> i32 {
        match &self.kind {
            ItemKind::Pin(pin) => pin.drill.first_layer,
            ItemKind::Via(via) => via.drill.first_layer,
            ItemKind::Trace(trace) => trace.layer,
            ItemKind::Area(area) => area.layer,
            ItemKind::ComponentOutline(outline) => outline.layer(),
            ItemKind::BoardOutline(_) => 0,
        }
    }

    /// The last layer of the item.
    pub fn last_layer(&self) -> i32 {
        match &self.kind {
            ItemKind::Pin(pin) => pin.drill.last_layer,
            ItemKind::Via(via) => via.drill.last_layer,
            ItemKind::Trace(trace) => trace.layer,
            ItemKind::Area(area) => area.layer,
            ItemKind::ComponentOutline(outline) => outline.layer(),
            ItemKind::BoardOutline(outline) => outline.layer_count as i32 - 1,
        }
    }

    pub fn is_on_layer(&self, layer: i32) -> bool {
        match &self.kind {
            ItemKind::BoardOutline(_) => true,
            _ => layer >= self.first_layer() && layer <= self.last_layer(),
        }
    }

    /// The layer of the tile shape with index `index`.
    pub fn shape_layer(&self, index: usize) -> i32 {
        match &self.kind {
            ItemKind::Pin(pin) => pin.drill.shape_layer(index),
            ItemKind::Via(via) => via.drill.shape_layer(index),
            ItemKind::Trace(trace) => trace.layer,
            ItemKind::Area(area) => area.layer,
            ItemKind::ComponentOutline(outline) => outline.layer(),
            ItemKind::BoardOutline(outline) => outline.shape_layer(index),
        }
    }

    /// True if the layer ranges of the items overlap.
    pub fn shares_layer(&self, other: &Item) -> bool {
        self.first_layer().max(other.first_layer()) <= self.last_layer().min(other.last_layer())
    }

    /// The first layer both items are on, or -1.
    pub fn first_common_layer(&self, other: &Item) -> i32 {
        let max_first = self.first_layer().max(other.first_layer());
        let min_last = self.last_layer().min(other.last_layer());
        if max_first > min_last { -1 } else { max_first }
    }

    /// The last layer both items are on, or -1.
    pub fn last_common_layer(&self, other: &Item) -> i32 {
        let max_first = self.first_layer().max(other.first_layer());
        let min_last = self.last_layer().min(other.last_layer());
        if max_first > min_last { -1 } else { min_last }
    }

    /// The bounding box of the item.
    pub fn bounding_box(&self) -> IntBox {
        match &self.kind {
            ItemKind::Pin(pin) => pin.drill.bounding_box(),
            ItemKind::Via(via) => via.drill.bounding_box(),
            ItemKind::Trace(trace) => trace.bounding_box(),
            ItemKind::Area(area) => area.area.bounding_box(),
            ItemKind::ComponentOutline(outline) => outline.area.bounding_box(),
            ItemKind::BoardOutline(outline) => outline.bounding_box(),
        }
    }

    /// True if the fixed state is at least `USER_FIXED`.
    pub fn is_user_fixed(&self) -> bool {
        self.fixed_state >= FixedState::UserFixed
    }

    /// Java `Item.isRoutable`: unfixed traces and vias with a net.
    pub fn is_routable(&self) -> bool {
        match &self.kind {
            ItemKind::Trace(_) | ItemKind::Via(_) => {
                !self.is_user_fixed() && !self.net_numbers.is_empty()
            }
            _ => false,
        }
    }

    /// Java `Item.isConnectable`.
    pub fn is_connectable(&self) -> bool {
        self.is_connectable_kind() && !self.net_numbers.is_empty()
    }

    /// Java `Item.isDrillable(int)`.
    pub fn is_drillable(&self, net_number: i32) -> bool {
        match &self.kind {
            ItemKind::Trace(_) => self.contains_net(net_number),
            ItemKind::Area(ObstacleArea {
                kind: AreaKind::Conduction { is_obstacle, .. },
                ..
            }) => !is_obstacle || self.contains_net(net_number),
            _ => false,
        }
    }

    /// Java `Item.unfix`: everything but system-fixed items becomes unfixed.
    pub(crate) fn unfix(&mut self) {
        if self.fixed_state != FixedState::SystemFixed {
            self.fixed_state = FixedState::Unfixed;
        }
    }

    /// Java `Item.removeFromNet`: removes the last occurrence of
    /// `net_number`.
    pub(crate) fn remove_from_net(&mut self, net_number: i32) -> bool {
        match self.net_numbers.iter().rposition(|n| *n == net_number) {
            Some(index) => {
                self.net_numbers.remove(index);
                true
            }
            None => false,
        }
    }

    /// The point where the normal contact of the two items is, if any.
    ///
    /// Java `Item.normalContactPoint`: only drill items and traces have
    /// normal contact points with each other.
    pub fn normal_contact_point(&self, other: &Item) -> Option<Point> {
        match (&self.kind, &other.kind) {
            // Java dispatches to `other.normalContactPoint(this)`, which answers
            // from the other trace's corners.
            (ItemKind::Trace(a), ItemKind::Trace(b)) => trace_trace_contact_point(b, a),
            (ItemKind::Trace(t), _) => other
                .drill()
                .and_then(|d| drill_trace_contact_point(other, d, self, t)),
            (_, ItemKind::Trace(t)) => self
                .drill()
                .and_then(|d| drill_trace_contact_point(self, d, other, t)),
            _ => {
                let (d1, d2) = (self.drill()?, other.drill()?);
                if self.shares_layer(other) && d1.center == d2.center {
                    Some(Point::Int(d1.center))
                } else {
                    None
                }
            }
        }
    }

    /// Java `Item.toString`.
    pub fn type_name(&self) -> &'static str {
        match &self.kind {
            ItemKind::Pin(_) => "pin",
            ItemKind::Via(_) => "via",
            ItemKind::Trace(_) => "polylinetrace",
            ItemKind::Area(area) => match area.kind {
                AreaKind::Obstacle => "obstaclearea",
                AreaKind::ViaObstacle => "viaobstaclearea",
                AreaKind::ComponentObstacle => "componentobstaclearea",
                AreaKind::Conduction { .. } => "conductionarea",
            },
            ItemKind::ComponentOutline(_) => "componentoutline",
            ItemKind::BoardOutline(_) => "boardoutline",
        }
    }
}

fn trace_trace_contact_point(this: &PolylineTrace, other: &PolylineTrace) -> Option<Point> {
    if this.layer != other.layer {
        return None;
    }
    let (first, last) = (this.first_corner(), this.last_corner());
    let (other_first, other_last) = (other.first_corner(), other.last_corner());
    let at_first = first == other_first || first == other_last;
    let at_last = last == other_first || last == other_last;
    if !(at_first || at_last) || (at_first && at_last) {
        None
    } else if at_first {
        Some(first)
    } else {
        Some(last)
    }
}

fn drill_trace_contact_point(
    drill_item: &Item,
    drill: &DrillData,
    trace_item: &Item,
    trace: &PolylineTrace,
) -> Option<Point> {
    if !drill_item.shares_layer(trace_item) {
        return None;
    }
    let center = Point::Int(drill.center);
    if center == trace.first_corner() || center == trace.last_corner() {
        Some(center)
    } else {
        None
    }
}

/// Adds `angle` to a rotation and brings the result into `[0, 360)` the way
/// Java's `while` loops do.
fn normalize_degree(mut value: f64) -> f64 {
    while value >= 360.0 {
        value -= 360.0;
    }
    while value < 0.0 {
        value += 360.0;
    }
    value
}

/// The absolute area of an area that belongs to no component: Java
/// `ObstacleArea.getArea` for `componentId <= 0`.
fn transform_free_area(
    relative_area: &Area,
    side_changed: bool,
    rotation_in_degree: f64,
    translation: &Vector,
    flip_style_rotate_first: bool,
) -> Area {
    let mut turned = relative_area.clone();
    if side_changed && !flip_style_rotate_first {
        turned = turned.mirror_vertical(IntPoint::new(0, 0));
    }
    if rotation_in_degree != 0.0 {
        if rotation_in_degree % 90.0 == 0.0 {
            turned = turned.turn_90_degree(
                freerouting_geometry::jmath::d2i(rotation_in_degree) / 90,
                IntPoint::new(0, 0),
            );
        } else {
            turned = turned.rotate_approx(rotation_in_degree.to_radians(), FloatPoint::ZERO);
        }
    }
    if side_changed && flip_style_rotate_first {
        turned = turned.mirror_vertical(IntPoint::new(0, 0));
    }
    turned.translate_by(translation)
}

/// Java `Pin.getBasePinName`: the pin name without a sub-pad suffix
/// (`@n`, `#n`, `_n`, `-n`).
pub fn base_pin_name(pin_name: &str) -> &str {
    if let Some(at) = pin_name.find('@') {
        return if at > 0 { &pin_name[..at] } else { "@" };
    }
    if let Some(hash) = pin_name.find('#') {
        return if hash > 0 { &pin_name[..hash] } else { "#" };
    }
    for separator in ['_', '-'] {
        if let Some(last) = pin_name.rfind(separator)
            && last > 0
            && last < pin_name.len() - 1
        {
            let suffix = &pin_name[last + 1..];
            if suffix.chars().all(|c| c.is_ascii_digit() || c.is_numeric()) {
                return &pin_name[..last];
            }
        }
    }
    pin_name
}

/// Java `TileShape.getInstance(Point)` as a tile shape.
pub(crate) fn point_tile(point: &Point) -> TileShape {
    TileShape::IntBox(TileShape::get_instance_point(point))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn item_ids_order_descending() {
        let set: BTreeSet<ItemId> = [ItemId(3), ItemId(10), ItemId(1)].into_iter().collect();
        let order: Vec<i32> = set.iter().map(|id| id.0).collect();
        assert_eq!(order, vec![10, 3, 1]);
    }

    #[test]
    fn base_pin_names() {
        assert_eq!(base_pin_name("1@2"), "1");
        assert_eq!(base_pin_name("@2"), "@");
        assert_eq!(base_pin_name("A#3"), "A");
        assert_eq!(base_pin_name("pad_1_1"), "pad_1");
        assert_eq!(base_pin_name("1-1"), "1");
        assert_eq!(base_pin_name("1-"), "1-");
        assert_eq!(base_pin_name("GND"), "GND");
    }

    #[test]
    fn degree_normalization() {
        assert_eq!(normalize_degree(450.0), 90.0);
        assert_eq!(normalize_degree(-90.0), 270.0);
    }
}
