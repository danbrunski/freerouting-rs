use freerouting_geometry::{FloatPoint, IntPoint, PlacementTransform, Point, Vector};

use crate::datastructures::UndoableObjects;
use crate::library::{LogicalPartId, PackageId};
use crate::util::CaseInsensitive;

/// The id of a component: its 1-based position in [`Components`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ComponentId(pub i32);

/// A placed or unplaced instance of a library package.
///
/// Port of `board.model.structure.Component`.
#[derive(Clone, Debug)]
pub struct Component {
    pub name: String,
    pub id: ComponentId,
    /// The position is locked.
    pub position_fixed: bool,
    package_front: PackageId,
    package_back: PackageId,
    part_number: Option<String>,
    exact_location: Option<FloatPoint>,
    rotation_in_degree: f64,
    logical_part: Option<LogicalPartId>,
    on_front: bool,
}

impl Component {
    #[allow(clippy::too_many_arguments)]
    fn new(
        name: String,
        exact_location: Option<FloatPoint>,
        rotation_in_degree: f64,
        on_front: bool,
        package_front: PackageId,
        package_back: PackageId,
        id: ComponentId,
        position_fixed: bool,
        part_number: Option<String>,
    ) -> Self {
        Component {
            name,
            id,
            position_fixed,
            package_front,
            package_back,
            part_number,
            exact_location,
            rotation_in_degree: normalize_degree(rotation_in_degree),
            logical_part: None,
            on_front,
        }
    }

    /// The location rounded to the board grid, or `None` if not placed.
    pub fn location(&self) -> Option<IntPoint> {
        self.exact_location.map(|p| p.round())
    }

    /// The unrounded location, or `None` if not placed.
    pub fn exact_location(&self) -> Option<FloatPoint> {
        self.exact_location
    }

    /// The transformation from package to board coordinates.
    ///
    /// # Panics
    /// If the component is not placed.
    pub fn placement_transform(&self, rotate_first: bool) -> PlacementTransform {
        PlacementTransform::new(
            self.exact_location.expect("placed component"),
            self.rotation_in_degree,
            !self.on_front,
            rotate_first,
        )
    }

    /// The rotation in degrees, in `[0, 360)`.
    pub fn rotation_in_degree(&self) -> f64 {
        self.rotation_in_degree
    }

    pub fn is_placed(&self) -> bool {
        self.exact_location.is_some()
    }

    pub fn placed_on_front(&self) -> bool {
        self.on_front
    }

    pub fn part_number(&self) -> Option<&str> {
        self.part_number.as_deref()
    }

    pub fn logical_part(&self) -> Option<LogicalPartId> {
        self.logical_part
    }

    pub fn set_logical_part(&mut self, part: Option<LogicalPartId>) {
        self.logical_part = part;
    }

    /// The package for the side the component is placed on.
    pub fn package(&self) -> PackageId {
        if self.on_front {
            self.package_front
        } else {
            self.package_back
        }
    }

    /// Moves the component by `vector`.
    pub fn translate_by(&mut self, vector: &Vector) {
        if let Some(location) = self.exact_location {
            let v = vector.to_float();
            self.exact_location = Some(FloatPoint::new(location.x + v.x, location.y + v.y));
        }
    }

    /// Turns the component by `factor` times 90 degrees around `pole`.
    pub fn turn_90_degree(&mut self, factor: i32, pole: IntPoint, flip_style_rotate_first: bool) {
        if factor == 0 {
            return;
        }
        let turn = if flip_style_rotate_first && !self.on_front {
            -factor
        } else {
            factor
        };
        self.rotation_in_degree =
            normalize_degree(self.rotation_in_degree + f64::from(turn) * 90.0);
        if let Some(location) = self.exact_location {
            self.exact_location =
                Some(location.turn_90_degree_around(factor, FloatPoint::from_int_point(pole)));
        }
    }

    /// Rotates the component by `angle_in_degree` around `pole`.
    pub fn rotate(&mut self, angle_in_degree: f64, pole: IntPoint, flip_style_rotate_first: bool) {
        if angle_in_degree == 0.0 {
            return;
        }
        if angle_in_degree % 90.0 == 0.0 {
            self.turn_90_degree(
                (angle_in_degree / 90.0) as i32,
                pole,
                flip_style_rotate_first,
            );
            return;
        }
        let turn_angle = if flip_style_rotate_first && !self.on_front {
            360.0 - angle_in_degree
        } else {
            angle_in_degree
        };
        self.rotation_in_degree = normalize_degree(self.rotation_in_degree + turn_angle);
        if let Some(location) = self.exact_location {
            self.exact_location = Some(location.rotate(
                angle_in_degree.to_radians(),
                FloatPoint::from_int_point(pole),
            ));
        }
    }

    /// Moves the component to the other board side, mirroring at the vertical
    /// line through `pole`.
    pub fn change_side(&mut self, pole: IntPoint, flip_style_rotate_first: bool) {
        if !flip_style_rotate_first {
            self.rotation_in_degree = (360.0 - self.rotation_in_degree) % 360.0;
        }
        self.on_front = !self.on_front;
        if let Some(location) = self.exact_location {
            self.exact_location = Some(FloatPoint::new(
                2.0 * f64::from(pole.x) - location.x,
                location.y,
            ));
        }
    }

    /// Takes the pose of `original`: location, rotation and side (Java
    /// `Component.restorePose`, B05.2), used to roll back a rejected
    /// transform.
    pub fn restore_pose(&mut self, original: &Component) {
        self.exact_location = original.exact_location;
        self.rotation_in_degree = original.rotation_in_degree;
        self.on_front = original.on_front;
    }

    /// The vector from the origin to the rounded location.
    pub fn translation(&self) -> Option<Vector> {
        self.location()
            .map(|l| Point::from(l).difference_by(&Point::from(IntPoint::new(0, 0))))
    }
}

fn normalize_degree(mut degree: f64) -> f64 {
    while degree >= 360.0 {
        degree -= 360.0;
    }
    while degree < 0.0 {
        degree += 360.0;
    }
    degree
}

/// The components of a board.
///
/// Port of `board.model.structure.Components`. Java keeps the components in an
/// array and in an undo list ordered by name without regard to case; here the
/// array holds the live components and the undo list holds copies that are
/// kept in step with them.
#[derive(Clone, Debug, Default)]
pub struct Components {
    components: Vec<Component>,
    undo_list: UndoableObjects<CaseInsensitive, Component>,
    flip_style_rotate_first: bool,
}

impl Components {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a component at the exact location `exact_location` (unplaced if
    /// `None`) and returns its id.
    #[allow(clippy::too_many_arguments)]
    pub fn add_precise(
        &mut self,
        name: impl Into<String>,
        exact_location: Option<FloatPoint>,
        rotation_in_degree: f64,
        on_front: bool,
        package_front: PackageId,
        package_back: PackageId,
        position_fixed: bool,
        part_number: Option<String>,
    ) -> ComponentId {
        let id = ComponentId(self.components.len() as i32 + 1);
        let component = Component::new(
            name.into(),
            exact_location,
            rotation_in_degree,
            on_front,
            package_front,
            package_back,
            id,
            position_fixed,
            part_number,
        );
        self.undo_list
            .insert(CaseInsensitive(component.name.clone()), component.clone());
        self.components.push(component);
        id
    }

    /// Adds a component named `Component#<id>` with the same package on both
    /// sides.
    pub fn add_unnamed(
        &mut self,
        location: Option<IntPoint>,
        rotation: f64,
        on_front: bool,
        package: PackageId,
    ) -> ComponentId {
        let name = format!("Component#{}", self.components.len() + 1);
        self.add_precise(
            name,
            location.map(FloatPoint::from_int_point),
            rotation,
            on_front,
            package,
            package,
            false,
            None,
        )
    }

    /// The component named `name` (exact match).
    pub fn get_by_name(&self, name: &str) -> Option<&Component> {
        self.components.iter().find(|c| c.name == name)
    }

    /// The component with id `id`.
    pub fn get(&self, id: ComponentId) -> Option<&Component> {
        if id.0 <= 0 {
            return None;
        }
        self.components.get(id.0 as usize - 1)
    }

    pub(crate) fn get_mut(&mut self, id: ComponentId) -> Option<&mut Component> {
        if id.0 <= 0 {
            return None;
        }
        self.components.get_mut(id.0 as usize - 1)
    }

    /// The number of components.
    pub fn count(&self) -> usize {
        self.components.len()
    }

    /// All components in id order.
    pub fn iter(&self) -> impl Iterator<Item = &Component> {
        self.components.iter()
    }

    pub fn generate_snapshot(&mut self) {
        self.undo_list.generate_snapshot();
    }

    /// Undoes the component changes of the last snapshot. Returns the ids of
    /// the components that moved.
    pub fn undo(&mut self) -> Option<Vec<ComponentId>> {
        self.undo_list.undo()?;
        Some(self.restore_from_undo_list())
    }

    /// Redoes the component changes undone last. Returns the ids of the
    /// components that moved.
    pub fn redo(&mut self) -> Option<Vec<ComponentId>> {
        self.undo_list.redo()?;
        Some(self.restore_from_undo_list())
    }

    pub fn pop_snapshot(&mut self) -> bool {
        self.undo_list.pop_snapshot()
    }

    fn restore_from_undo_list(&mut self) -> Vec<ComponentId> {
        let mut moved = Vec::new();
        for (_, component) in self.undo_list.iter() {
            let index = component.id.0 as usize - 1;
            if index < self.components.len() {
                self.components[index] = component.clone();
                moved.push(component.id);
            }
        }
        moved
    }

    fn change(&mut self, id: ComponentId, f: impl Fn(&mut Component, bool)) {
        let rotate_first = self.flip_style_rotate_first;
        let Some(component) = self.get_mut(id) else {
            return;
        };
        f(component, rotate_first);
        let key = CaseInsensitive(component.name.clone());
        let updated = component.clone();
        self.undo_list.save_for_undo(&key);
        if let Some(stored) = self.undo_list.get_mut(&key) {
            *stored = updated;
        }
    }

    /// Moves the component by `vector`.
    pub fn move_by(&mut self, id: ComponentId, vector: &Vector) {
        self.change(id, |c, _| c.translate_by(vector));
    }

    /// Turns the component by `factor` times 90 degrees around `pole`.
    pub fn turn_90_degree(&mut self, id: ComponentId, factor: i32, pole: IntPoint) {
        self.change(id, |c, rotate_first| {
            c.turn_90_degree(factor, pole, rotate_first)
        });
    }

    /// Rotates the component by `rotation_in_degree` around `pole`.
    pub fn rotate(&mut self, id: ComponentId, rotation_in_degree: f64, pole: IntPoint) {
        self.change(id, |c, rotate_first| {
            c.rotate(rotation_in_degree, pole, rotate_first)
        });
    }

    /// Moves the component to the other side of the board.
    pub fn change_side(&mut self, id: ComponentId, pole: IntPoint) {
        self.change(id, |c, rotate_first| c.change_side(pole, rotate_first));
    }

    /// Adds a copy of component `original` moved by `translation`, with the
    /// package `package` on both sides, named `Component#<n>` (Java
    /// `Components.copy`, B05.2). `None` if `original` does not exist or is
    /// not placed (Java throws there).
    pub fn copy(
        &mut self,
        original: ComponentId,
        translation: &Vector,
        package: PackageId,
    ) -> Option<ComponentId> {
        let original = self.get(original)?;
        let location = original.exact_location()?;
        let offset = translation.to_float();
        let translated = FloatPoint::new(location.x + offset.x, location.y + offset.y);
        let (rotation, on_front, part_number) = (
            original.rotation_in_degree(),
            original.placed_on_front(),
            original.part_number().map(str::to_string),
        );
        let name = format!("Component#{}", self.components.len() + 1);
        Some(self.add_precise(
            name,
            Some(translated),
            rotation,
            on_front,
            package,
            package,
            false,
            part_number,
        ))
    }

    /// Gives component `id` the pose of `original` (Java
    /// `component.restorePose(original)`). Like Java, which changes the
    /// object its undo list holds, it takes no undo snapshot.
    pub fn restore_pose(&mut self, id: ComponentId, original: &Component) {
        let Some(component) = self.get_mut(id) else {
            return;
        };
        component.restore_pose(original);
        let key = CaseInsensitive(component.name.clone());
        let updated = component.clone();
        if let Some(stored) = self.undo_list.get_mut(&key) {
            *stored = updated;
        }
    }

    /// Whether the components on the back side are rotated before they are
    /// mirrored.
    pub fn flip_style_rotate_first(&self) -> bool {
        self.flip_style_rotate_first
    }

    pub fn set_flip_style_rotate_first(&mut self, value: bool) {
        self.flip_style_rotate_first = value;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotation_is_normalized_and_location_rounded() {
        let mut components = Components::new();
        let id = components.add_precise(
            "R1",
            Some(FloatPoint::new(10.4, -3.6)),
            -90.0,
            true,
            PackageId(1),
            PackageId(2),
            false,
            None,
        );
        let c = components.get(id).unwrap();
        assert_eq!(c.rotation_in_degree(), 270.0);
        assert_eq!(c.location(), Some(IntPoint::new(10, -4)));
        assert_eq!(c.package(), PackageId(1));
        assert!(components.get_by_name("r1").is_none());
    }

    #[test]
    fn moves_can_be_undone() {
        let mut components = Components::new();
        let id = components.add_precise(
            "U1",
            Some(FloatPoint::new(0.0, 0.0)),
            0.0,
            true,
            PackageId(1),
            PackageId(1),
            false,
            None,
        );
        components.generate_snapshot();
        components.move_by(id, &Vector::get_instance(5, 7));
        assert_eq!(
            components.get(id).unwrap().location(),
            Some(IntPoint::new(5, 7))
        );
        components.undo().unwrap();
        assert_eq!(
            components.get(id).unwrap().location(),
            Some(IntPoint::new(0, 0))
        );
        components.redo().unwrap();
        assert_eq!(
            components.get(id).unwrap().location(),
            Some(IntPoint::new(5, 7))
        );
    }

    #[test]
    fn change_side_mirrors_rotation() {
        let mut components = Components::new();
        let id = components.add_precise(
            "U2",
            Some(FloatPoint::new(10.0, 0.0)),
            30.0,
            true,
            PackageId(1),
            PackageId(2),
            false,
            None,
        );
        components.change_side(id, IntPoint::new(0, 0));
        let c = components.get(id).unwrap();
        assert!(!c.placed_on_front());
        assert_eq!(c.rotation_in_degree(), 330.0);
        assert_eq!(c.location(), Some(IntPoint::new(-10, 0)));
        assert_eq!(c.package(), PackageId(2));
    }
}
