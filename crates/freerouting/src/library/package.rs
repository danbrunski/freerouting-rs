use freerouting_geometry::{Area, FloatPoint, IntPoint, Point, Shape, Vector};

use super::PadstackId;
use crate::util::equals_ignore_case;

/// The id of a package: its 1-based position in [`Packages`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PackageId(pub usize);

/// A pin of a library package.
///
/// Port of `core.library.Package.Pin`.
#[derive(Clone, Debug)]
pub struct PackagePin {
    pub name: String,
    pub padstack_id: PadstackId,
    /// The location relative to the package origin, rounded.
    pub relative_location: Vector,
    /// The pin rotation in degrees.
    pub rotation_in_degree: f64,
    exact_relative_location: FloatPoint,
}

impl PackagePin {
    /// A pin at the exact relative location `relative_location`.
    pub fn new(
        name: impl Into<String>,
        padstack_id: PadstackId,
        relative_location: FloatPoint,
        rotation_in_degree: f64,
    ) -> Self {
        let rounded: IntPoint = relative_location.round();
        PackagePin {
            name: name.into(),
            padstack_id,
            relative_location: Point::from(rounded).difference_by(&Point::from(IntPoint::new(0, 0))),
            rotation_in_degree,
            exact_relative_location: relative_location,
        }
    }

    /// The unrounded relative location.
    pub fn exact_relative_location(&self) -> FloatPoint {
        self.exact_relative_location
    }
}

/// A keepout of a library package, in package coordinates.
///
/// Port of `core.library.Package.Keepout`.
#[derive(Clone, Debug)]
pub struct PackageKeepout {
    pub name: String,
    pub area: Area,
    /// The layer, or -1 for all signal layers.
    pub layer: i32,
}

/// A footprint: pins, outline and keepouts.
///
/// Port of `core.library.Package`.
#[derive(Clone, Debug)]
pub struct Package {
    pub name: String,
    pub id: PackageId,
    pins: Vec<PackagePin>,
    /// The outline shapes; an entry is `None` where the DSN shape could not be
    /// read. `None` as a whole for packages created without outline.
    pub outline: Option<Vec<Option<Shape>>>,
    /// The DSN line width of each outline shape, 0 for area shapes.
    pub outline_widths: Option<Vec<f64>>,
    /// Whether each outline shape is closed.
    pub outline_is_closed: Option<Vec<bool>>,
    pub keepouts: Vec<PackageKeepout>,
    pub via_keepouts: Vec<PackageKeepout>,
    pub place_keepouts: Vec<PackageKeepout>,
    /// The side the package is defined for.
    pub is_front: bool,
}

impl Package {
    /// The pin with index `index`.
    pub fn get_pin(&self, index: usize) -> Option<&PackagePin> {
        self.pins.get(index)
    }

    /// The index of the pin named `name` (exact match), or `None`.
    pub fn get_pin_index(&self, name: &str) -> Option<usize> {
        self.pins.iter().position(|pin| pin.name == name)
    }

    /// The number of pins.
    pub fn pin_count(&self) -> usize {
        self.pins.len()
    }

    /// All pins.
    pub fn pins(&self) -> &[PackagePin] {
        &self.pins
    }
}

/// The packages of a board.
///
/// Port of `core.library.Packages`.
#[derive(Clone, Debug, Default)]
pub struct Packages {
    packages: Vec<Package>,
}

impl Packages {
    pub fn new() -> Self {
        Self::default()
    }

    /// The package named `name` (any case) for side `is_front`; the package
    /// for the other side if there is none; then the same lookups with a
    /// trailing `::<digits>` removed from `name`.
    pub fn get_by_name(&self, name: &str, is_front: bool) -> Option<&Package> {
        if let Some(found) = self.lookup(name, is_front) {
            return Some(found);
        }
        let base_name = strip_numeric_suffix(name);
        if !equals_ignore_case(base_name, name) {
            return self.lookup(base_name, is_front);
        }
        None
    }

    fn lookup(&self, name: &str, is_front: bool) -> Option<&Package> {
        let mut other_side = None;
        for package in &self.packages {
            if equals_ignore_case(&package.name, name) {
                if package.is_front == is_front {
                    return Some(package);
                }
                other_side = Some(package);
            }
        }
        other_side
    }

    /// The package with id `id`.
    pub fn get(&self, id: PackageId) -> Option<&Package> {
        if id.0 == 0 {
            return None;
        }
        self.packages.get(id.0 - 1)
    }

    /// The number of packages.
    pub fn count(&self) -> usize {
        self.packages.len()
    }

    /// All packages in id order.
    pub fn iter(&self) -> impl Iterator<Item = &Package> {
        self.packages.iter()
    }

    /// Appends a package and returns its id.
    #[allow(clippy::too_many_arguments)]
    pub fn add(
        &mut self,
        name: impl Into<String>,
        pins: Vec<PackagePin>,
        outline: Option<Vec<Option<Shape>>>,
        outline_widths: Option<Vec<f64>>,
        outline_is_closed: Option<Vec<bool>>,
        keepouts: Vec<PackageKeepout>,
        via_keepouts: Vec<PackageKeepout>,
        place_keepouts: Vec<PackageKeepout>,
        is_front: bool,
    ) -> PackageId {
        let id = PackageId(self.packages.len() + 1);
        self.packages.push(Package {
            name: name.into(),
            id,
            pins,
            outline,
            outline_widths,
            outline_is_closed,
            keepouts,
            via_keepouts,
            place_keepouts,
            is_front,
        });
        id
    }

    /// Appends a front side package named `Package#<id>` without outline and
    /// keepouts.
    pub fn add_unnamed(&mut self, pins: Vec<PackagePin>) -> PackageId {
        let name = format!("Package#{}", self.packages.len() + 1);
        self.add(
            name,
            pins,
            None,
            None,
            None,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            true,
        )
    }
}

/// Removes a trailing `::<digits>` (Java `replaceFirst("::\\d+$", "")`).
fn strip_numeric_suffix(name: &str) -> &str {
    let digits = name.len() - name.trim_end_matches(|c: char| c.is_ascii_digit()).len();
    if digits == 0 {
        return name;
    }
    let before_digits = &name[..name.len() - digits];
    before_digits.strip_suffix("::").unwrap_or(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn add(packages: &mut Packages, name: &str, is_front: bool) -> PackageId {
        packages.add(
            name,
            Vec::new(),
            None,
            None,
            None,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            is_front,
        )
    }

    #[test]
    fn lookup_prefers_the_same_side() {
        let mut packages = Packages::new();
        let front = add(&mut packages, "SO8", true);
        let back = add(&mut packages, "so8", false);
        assert_eq!(packages.get_by_name("SO8", true).unwrap().id, front);
        assert_eq!(packages.get_by_name("SO8", false).unwrap().id, back);
        let only = add(&mut packages, "R0603", true);
        assert_eq!(packages.get_by_name("r0603", false).unwrap().id, only);
    }

    #[test]
    fn lookup_falls_back_to_the_base_name() {
        let mut packages = Packages::new();
        let base = add(&mut packages, "Cap", true);
        assert_eq!(packages.get_by_name("Cap::2", true).unwrap().id, base);
        assert!(packages.get_by_name("Cap:2", true).is_none());
        assert_eq!(strip_numeric_suffix("A::12"), "A");
        assert_eq!(strip_numeric_suffix("A::"), "A::");
    }

    #[test]
    fn pin_rounds_relative_location() {
        let pin = PackagePin::new("1", PadstackId(1), FloatPoint::new(1.6, -2.4), 90.0);
        assert_eq!(pin.relative_location, Vector::get_instance(2, -2));
        assert_eq!(pin.exact_relative_location(), FloatPoint::new(1.6, -2.4));
    }
}
