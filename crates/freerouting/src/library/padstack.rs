use std::cell::OnceCell;

use freerouting_geometry::{ConvexShape, Direction, PolylineShapeOps, TileShape};

/// The id of a padstack: its 1-based position in [`Padstacks`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PadstackId(pub usize);

/// The pad shapes of a pin or via on each layer.
///
/// Port of `core.library.Padstack`. A layer without a pad has no shape.
#[derive(Clone, Debug)]
pub struct Padstack {
    pub name: String,
    pub id: PadstackId,
    /// Whether vias may be attached to the pad (DSN `attach`).
    pub attach_allowed: bool,
    /// Whether the padstack keeps its layers when its component is placed on
    /// the back side.
    pub placed_absolute: bool,
    shapes: Vec<Option<ConvexShape>>,
    auxiliary_shapes: Vec<Option<Vec<ConvexShape>>>,
    /// The padstack had a non-convex shape that was decomposed.
    pub has_non_convex_geometry: bool,
    /// A drill hole without copper.
    pub hole_only: bool,
    cached_drill_radius: OnceCell<f64>,
}

impl Padstack {
    pub(crate) fn new(
        name: String,
        id: PadstackId,
        shapes: Vec<Option<ConvexShape>>,
        auxiliary_shapes: Vec<Option<Vec<ConvexShape>>>,
        attach_allowed: bool,
        placed_absolute: bool,
    ) -> Self {
        Padstack {
            name,
            id,
            attach_allowed,
            placed_absolute,
            shapes,
            auxiliary_shapes,
            has_non_convex_geometry: false,
            hole_only: false,
            cached_drill_radius: OnceCell::new(),
        }
    }

    /// The pad shape on `layer`, or `None` if there is none or `layer` is out
    /// of range.
    pub fn get_shape(&self, layer: i32) -> Option<&ConvexShape> {
        if layer < 0 {
            return None;
        }
        self.shapes.get(layer as usize)?.as_ref()
    }

    /// The extra convex pieces of a non-convex pad on `layer`.
    pub fn get_auxiliary_shapes(&self, layer: i32) -> Option<&[ConvexShape]> {
        if layer < 0 {
            return None;
        }
        self.auxiliary_shapes
            .get(layer as usize)?
            .as_ref()
            .map(Vec::as_slice)
    }

    /// True if some layer has auxiliary shapes.
    pub fn has_auxiliary_shapes(&self) -> bool {
        self.auxiliary_shapes
            .iter()
            .any(|aux| aux.as_ref().is_some_and(|a| !a.is_empty()))
    }

    /// True if the padstack had non-convex geometry.
    pub fn has_non_convex_geometry(&self) -> bool {
        self.has_non_convex_geometry || self.has_auxiliary_shapes()
    }

    /// The first layer with a pad, or the layer count if there is none.
    pub fn from_layer(&self) -> i32 {
        self.shapes
            .iter()
            .position(Option::is_some)
            .unwrap_or(self.shapes.len()) as i32
    }

    /// The last layer with a pad, or -1 if there is none.
    pub fn to_layer(&self) -> i32 {
        self.shapes
            .iter()
            .rposition(Option::is_some)
            .map_or(-1, |i| i as i32)
    }

    /// The number of board layers the padstack was made for.
    pub fn board_layer_count(&self) -> usize {
        self.shapes.len()
    }

    /// The drill radius: taken from a KiCad style name `..._<outer>:<drill>_...`
    /// relative to the actual pad size, otherwise 45 % of the smallest pad
    /// radius.
    pub fn get_drill_radius(&self) -> f64 {
        *self.cached_drill_radius.get_or_init(|| {
            self.drill_radius_from_name()
                .unwrap_or_else(|| self.smallest_radius() * 0.45)
        })
    }

    fn drill_radius_from_name(&self) -> Option<f64> {
        let name = &self.name;
        let colon_index = name.find(':')?;
        let after_colon = &name[colon_index + 1..];
        let drill_str = match after_colon.find('_') {
            Some(underscore) => &after_colon[..underscore],
            None => after_colon,
        };
        let drill_dia = parse_java_double(&strip_non_numeric(drill_str))?;
        let last_underscore = name[..colon_index].rfind('_')?;
        let outer_str = &name[last_underscore + 1..colon_index];
        let outer_dia = if outer_str.contains('x') {
            let parts: Vec<&str> = java_split(outer_str, 'x');
            let w = parse_java_double(&strip_non_numeric(parts.first()?))?;
            let h = parse_java_double(&strip_non_numeric(parts.get(1)?))?;
            w.min(h)
        } else {
            parse_java_double(&strip_non_numeric(outer_str))?
        };
        if outer_dia > 0.0 {
            let actual_outer_radius = self.smallest_radius();
            if actual_outer_radius > 0.0 {
                return Some(actual_outer_radius * (drill_dia / outer_dia));
            }
        }
        None
    }

    fn smallest_radius(&self) -> f64 {
        let mut min_radius = f64::MAX;
        for shape in self.shapes.iter().flatten() {
            let b = shape.to_shape().bounding_box();
            let radius = f64::from(b.width().min(b.height())) / 2.0;
            if radius < min_radius {
                min_radius = radius;
            }
        }
        if min_radius == f64::MAX {
            0.0
        } else {
            min_radius
        }
    }

    /// The directions in which traces may leave a rectangular or octagonal pad
    /// on `layer`: along its longer side, or in all four directions if the
    /// sides differ by less than `factor`.
    pub fn get_trace_exit_directions(&self, layer: i32, factor: f64) -> Vec<Direction> {
        let mut result = Vec::new();
        let Some(ConvexShape::Tile(tile)) = self.get_shape(layer) else {
            return result;
        };
        if !matches!(tile, TileShape::IntBox(_) | TileShape::IntOctagon(_)) {
            return result;
        }
        let b = tile.bounding_box();
        let (w, h) = (f64::from(b.width()), f64::from(b.height()));
        let all_dirs = w.max(h) < factor * w.min(h);
        if all_dirs || b.width() >= b.height() {
            result.push(Direction::RIGHT);
            result.push(Direction::LEFT);
        }
        if all_dirs || b.width() <= b.height() {
            result.push(Direction::UP);
            result.push(Direction::DOWN);
        }
        result
    }
}

/// Java `s.replaceAll("[^0-9.]", "")`.
fn strip_non_numeric(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_digit() || *c == '.')
        .collect()
}

/// Java `Double.parseDouble` for strings of digits and dots.
fn parse_java_double(s: &str) -> Option<f64> {
    if s.is_empty() || s == "." {
        return None;
    }
    s.parse().ok()
}

/// Java `String.split` with a single character: trailing empty strings are
/// removed.
fn java_split(s: &str, separator: char) -> Vec<&str> {
    let mut parts: Vec<&str> = s.split(separator).collect();
    while parts.len() > 1 && parts.last() == Some(&"") {
        parts.pop();
    }
    if parts.len() == 1 && parts[0].is_empty() {
        parts.clear();
    }
    parts
}

/// The padstacks of a board.
///
/// Port of `core.library.Padstacks`.
#[derive(Clone, Debug)]
pub struct Padstacks {
    layer_count: usize,
    padstacks: Vec<Padstack>,
}

impl Padstacks {
    /// An empty list for a board with `layer_count` layers.
    pub fn new(layer_count: usize) -> Self {
        Padstacks {
            layer_count,
            padstacks: Vec::new(),
        }
    }

    /// The number of layers of the board.
    pub fn board_layer_count(&self) -> usize {
        self.layer_count
    }

    /// The padstack named `name`, compared without regard to case.
    pub fn get_by_name(&self, name: &str) -> Option<&Padstack> {
        self.padstacks
            .iter()
            .find(|p| crate::util::equals_ignore_case(&p.name, name))
    }

    /// The padstack with id `id`.
    pub fn get(&self, id: PadstackId) -> Option<&Padstack> {
        if id.0 == 0 {
            return None;
        }
        self.padstacks.get(id.0 - 1)
    }

    pub(crate) fn get_mut(&mut self, id: PadstackId) -> Option<&mut Padstack> {
        if id.0 == 0 {
            return None;
        }
        self.padstacks.get_mut(id.0 - 1)
    }

    /// The number of padstacks.
    pub fn count(&self) -> usize {
        self.padstacks.len()
    }

    /// All padstacks in id order.
    pub fn iter(&self) -> impl Iterator<Item = &Padstack> {
        self.padstacks.iter()
    }

    /// Appends a padstack and returns its id.
    pub fn add(
        &mut self,
        name: impl Into<String>,
        shapes: Vec<Option<ConvexShape>>,
        auxiliary_shapes: Vec<Option<Vec<ConvexShape>>>,
        attach_allowed: bool,
        placed_absolute: bool,
    ) -> PadstackId {
        let id = PadstackId(self.padstacks.len() + 1);
        self.padstacks.push(Padstack::new(
            name.into(),
            id,
            shapes,
            auxiliary_shapes,
            attach_allowed,
            placed_absolute,
        ));
        id
    }

    /// Appends a padstack named `padstack#<id>` without attach and absolute
    /// placement.
    pub fn add_unnamed(&mut self, shapes: Vec<Option<ConvexShape>>) -> PadstackId {
        let name = format!("padstack#{}", self.padstacks.len() + 1);
        self.add(name, shapes, Vec::new(), false, false)
    }

    /// Appends a padstack with `shape` on the layers `from_layer..=to_layer`.
    pub fn add_layer_range(
        &mut self,
        shape: ConvexShape,
        from_layer: i32,
        to_layer: i32,
    ) -> PadstackId {
        let mut shapes = vec![None; self.layer_count];
        let first = from_layer.max(0);
        let last = to_layer.min(self.layer_count as i32 - 1);
        for i in first..=last {
            shapes[i as usize] = Some(shape.clone());
        }
        self.add_unnamed(shapes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use freerouting_geometry::IntBox;

    fn square(half: i32) -> ConvexShape {
        ConvexShape::Tile(TileShape::IntBox(IntBox::new(-half, -half, half, half)))
    }

    #[test]
    fn layer_range_and_lookup() {
        let mut padstacks = Padstacks::new(4);
        let id = padstacks.add(
            "Via",
            vec![None, Some(square(10)), Some(square(10)), None],
            Vec::new(),
            true,
            false,
        );
        let padstack = padstacks.get(id).unwrap();
        assert_eq!(padstack.from_layer(), 1);
        assert_eq!(padstack.to_layer(), 2);
        assert_eq!(padstacks.get_by_name("via").unwrap().id, id);
        assert!(padstack.get_shape(0).is_none());
        assert!(padstack.get_shape(7).is_none());
    }

    #[test]
    fn drill_radius_from_kicad_name() {
        let mut padstacks = Padstacks::new(2);
        let id = padstacks.add(
            "Round[A]Pad_1600:800_um",
            vec![Some(square(800)), Some(square(800))],
            Vec::new(),
            true,
            false,
        );
        assert_eq!(padstacks.get(id).unwrap().get_drill_radius(), 400.0);
        let id = padstacks.add("plain", vec![Some(square(100))], Vec::new(), true, false);
        assert_eq!(padstacks.get(id).unwrap().get_drill_radius(), 45.0);
    }

    #[test]
    fn trace_exit_directions() {
        let mut padstacks = Padstacks::new(1);
        let id = padstacks.add(
            "wide",
            vec![Some(ConvexShape::Tile(TileShape::IntBox(IntBox::new(
                -100, -10, 100, 10,
            ))))],
            Vec::new(),
            true,
            false,
        );
        let dirs = padstacks.get(id).unwrap().get_trace_exit_directions(0, 1.5);
        assert_eq!(dirs, vec![Direction::RIGHT, Direction::LEFT]);
    }
}
