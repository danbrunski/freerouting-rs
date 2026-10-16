//! The maze queue (A02.6).
//!
//! Port of `MazeListElement` (`MazeListElement.java`) and the queue it lives
//! in, `MazeSearchEngine.mazeExpansionList` (a `TreeSet`), at `a773a70ec`.
//!
//! The queue orders its elements by [`QueueKey`]: `(sorting value,
//! expansion value, door id, section)`, the doubles compared with `<` and
//! `>` as `MazeListElement.compareTo` does (`:80-113`), the ints by value. A
//! full tie is equal, so the `TreeSet` keeps the element already there and
//! drops the newcomer. `-0.0` is stored as `0.0` (the two compare equal
//! either way), and no NaN may enter (`debug_assert!`).
//!
//! A fanout search filters what enters the queue (A04.2, the `add` override
//! of `MazeSearchEngine.java:91-137`): [`FanoutFilter`], applied by
//! [`Session::queue_element`] to every element, the start doors of the
//! initialisation included.

use std::cmp::Ordering;
use std::collections::BTreeMap;

use freerouting_geometry::{FloatLine, FloatPoint};

use super::{Adjustment, ExpandableRef, RoomId, Session};
use crate::autoroute::control::AutorouteControl;

/// The order of the maze queue (`MazeListElement.compareTo`).
#[derive(Clone, Copy, Debug)]
pub struct QueueKey {
    /// The expansion value plus the lower bound of the distance to the
    /// destination (`sortingValue`).
    pub sorting_value: f64,
    /// The weighted distance from the start (`expansionValue`).
    pub expansion_value: f64,
    /// The Java `getId` of the door.
    pub door_id: i32,
    /// The section of the door (`sectionNoOfDoor`).
    pub section: i32,
}

/// Java's `double` comparison with `<` and `>`: neither means a tie.
fn compare_doubles(a: f64, b: f64) -> Ordering {
    if a < b {
        Ordering::Less
    } else if a > b {
        Ordering::Greater
    } else {
        Ordering::Equal
    }
}

/// `-0.0` as `0.0`; no NaN.
fn normalize(value: f64) -> f64 {
    debug_assert!(!value.is_nan(), "a NaN entered the maze queue");
    if value == 0.0 { 0.0 } else { value }
}

impl QueueKey {
    /// A key with `-0.0` normalized to `0.0`.
    pub fn new(sorting_value: f64, expansion_value: f64, door_id: i32, section: i32) -> QueueKey {
        QueueKey {
            sorting_value: normalize(sorting_value),
            expansion_value: normalize(expansion_value),
            door_id,
            section,
        }
    }
}

impl Ord for QueueKey {
    fn cmp(&self, other: &Self) -> Ordering {
        // PARITY: MazeListElement.java:80-113.
        compare_doubles(self.sorting_value, other.sorting_value)
            .then_with(|| compare_doubles(self.expansion_value, other.expansion_value))
            .then_with(|| self.door_id.cmp(&other.door_id))
            .then_with(|| self.section.cmp(&other.section))
    }
}

impl PartialOrd for QueueKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for QueueKey {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for QueueKey {}

/// An element of the maze queue (Java `MazeListElement`).
#[derive(Clone, Debug)]
pub struct MazeListElement {
    /// The door, target door, drill or drill page (`door`).
    pub door: ExpandableRef,
    /// The section of the door, or the layer of a drill
    /// (`sectionNoOfDoor`).
    pub section: usize,
    /// The door and section this element was expanded from
    /// (`backtrackDoor`, `sectionNoOfBacktrackDoor`).
    pub backtrack: Option<(ExpandableRef, usize)>,
    pub expansion_value: f64,
    pub sorting_value: f64,
    /// The room expanded next (`nextRoom`).
    pub next_room: Option<RoomId>,
    /// The part of the door nearest the backtrack door (`shapeEntry`).
    pub shape_entry: FloatLine,
    pub room_ripped: bool,
    pub adjustment: Adjustment,
    pub already_checked: bool,
    pub ripup_cost: i32,
}

/// The maze queue: elements ordered by [`QueueKey`], a tie keeping the
/// element already queued.
#[derive(Clone, Debug, Default)]
pub struct MazeQueue {
    order: BTreeMap<QueueKey, usize>,
    elements: Vec<MazeListElement>,
    filter: Option<FanoutFilter>,
}

/// The queue filter of a fanout search (`MazeSearchEngine.java:91-137`).
///
/// On the start layer, an element whose entry midpoint is farther than the
/// maximum escape length from the pin centre is refused; a drill nearer
/// than the minimum escape length is refused. Lengths are `setting_mm *
/// 1000.0 * resolution(UM)`; a plane fanout's minimum is 0.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FanoutFilter {
    pub pin_center: FloatPoint,
    pub start_layer: i32,
    /// `maxLen * resolution`, in board units.
    pub max_escape: f64,
    /// `minLen * resolution`, in board units.
    pub min_escape: f64,
}

impl FanoutFilter {
    /// The filter of `control`, `None` unless it is a fanout with a start
    /// pin centre. `max_escape_mm` and `min_escape_mm` are the settings'
    /// `fanout.max_escape_length_mm` and `fanout.min_escape_length_mm`
    /// (defaults 4.5 and 2.5, `DefaultSettings.java:186-187`; Java's 3 mm
    /// and 0.5 mm apply only to absent fanout settings, which the port
    /// never has), `resolution_um` the board's units per micrometre.
    pub fn new(
        control: &AutorouteControl,
        max_escape_mm: f64,
        min_escape_mm: f64,
        resolution_um: f64,
    ) -> Option<FanoutFilter> {
        if !control.is_fanout {
            return None;
        }
        let pin_center = control.fanout_start_pin_center.as_ref()?.to_float();
        Some(FanoutFilter::with_lengths(
            pin_center,
            control.fanout_start_pin_layer,
            max_escape_mm,
            min_escape_mm,
            control.is_plane_fanout,
            resolution_um,
        ))
    }

    /// The filter around `pin_center` on `start_layer` with the escape
    /// lengths in millimetres; a plane fanout's minimum is 0
    /// (`PLANE_FANOUT_MIN_ESCAPE_LENGTH_UM`).
    pub fn with_lengths(
        pin_center: FloatPoint,
        start_layer: i32,
        max_escape_mm: f64,
        min_escape_mm: f64,
        plane_fanout: bool,
        resolution_um: f64,
    ) -> FanoutFilter {
        // PARITY: MazeSearchEngine.java:102-130, `(mm * 1000.0) * resolution`.
        let max_len = max_escape_mm * 1000.0;
        let min_len = if plane_fanout {
            0.0
        } else {
            min_escape_mm * 1000.0
        };
        FanoutFilter {
            pin_center,
            start_layer,
            max_escape: max_len * resolution_um,
            min_escape: min_len * resolution_um,
        }
    }

    /// True if the element may enter the queue: `next_room_layer` is the
    /// layer of its next room, `drill_location` the location of its drill.
    pub fn admits(
        &self,
        shape_entry: &FloatLine,
        next_room_layer: Option<i32>,
        drill_location: Option<FloatPoint>,
    ) -> bool {
        if next_room_layer == Some(self.start_layer) {
            let entry = shape_entry.a.middle_point(shape_entry.b);
            if entry.distance(self.pin_center) > self.max_escape {
                return false;
            }
        }
        if let Some(location) = drill_location
            && location.distance(self.pin_center) < self.min_escape
        {
            return false;
        }
        true
    }
}

/// What became of an element offered to the queue.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Push {
    Added,
    /// The fanout filter refused it.
    Refused,
    /// An equal key is queued; the newcomer is dropped.
    Tie,
}

impl MazeQueue {
    pub fn new() -> MazeQueue {
        MazeQueue::default()
    }

    /// Sets the fanout filter (`None` for none).
    pub fn set_fanout_filter(&mut self, filter: Option<FanoutFilter>) {
        self.filter = filter;
    }

    /// The fanout filter.
    pub fn fanout_filter(&self) -> Option<&FanoutFilter> {
        self.filter.as_ref()
    }

    /// Adds `element` under `key` unless an equal key is queued (Java
    /// `TreeSet.add`). Returns true if it was added.
    pub fn add(&mut self, key: QueueKey, element: MazeListElement) -> bool {
        if self.order.contains_key(&key) {
            // PARITY: a full tie keeps the first (TreeSet.add returns false).
            return false;
        }
        self.order.insert(key, self.elements.len());
        self.elements.push(element);
        true
    }

    /// Removes and returns the first element (`TreeSet.pollFirst`).
    pub fn pop_first(&mut self) -> Option<(QueueKey, MazeListElement)> {
        let (key, index) = self.order.pop_first()?;
        Some((key, self.elements[index].clone()))
    }

    /// The first key, if any.
    pub fn first_key(&self) -> Option<&QueueKey> {
        self.order.keys().next()
    }

    /// The queued elements in key order.
    pub fn iter(&self) -> impl Iterator<Item = (&QueueKey, &MazeListElement)> + '_ {
        self.order.iter().map(|(k, i)| (k, &self.elements[*i]))
    }

    pub fn len(&self) -> usize {
        self.order.len()
    }

    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }

    pub fn clear(&mut self) {
        self.order.clear();
        self.elements.clear();
    }
}

impl Session<'_> {
    /// Offers `element` to the maze queue under `key`: Java's
    /// `mazeExpansionList.add`, with the fanout filter of the queue.
    pub fn queue_element(&mut self, key: QueueKey, element: MazeListElement) -> Push {
        if let Some(filter) = self.queue.fanout_filter() {
            let layer = element.next_room.map(|r| self.room(r).layer());
            let location = match element.door {
                ExpandableRef::Drill(drill) => Some(self.drill(drill).location.to_float()),
                _ => None,
            };
            if !filter.admits(&element.shape_entry, layer, location) {
                return Push::Refused;
            }
        }
        if self.queue.add(key, element) {
            Push::Added
        } else {
            Push::Tie
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::autoroute::session::{DoorId, TargetDoorId};
    use freerouting_geometry::FloatPoint;

    fn element(door: u32) -> MazeListElement {
        let p = FloatPoint::new(0.0, 0.0);
        MazeListElement {
            door: ExpandableRef::Door(DoorId(door)),
            section: 0,
            backtrack: None,
            expansion_value: 0.0,
            sorting_value: 0.0,
            next_room: None,
            shape_entry: FloatLine::new(p, p),
            room_ripped: false,
            adjustment: Adjustment::None,
            already_checked: false,
            ripup_cost: 0,
        }
    }

    fn entry(x: f64, y: f64) -> FloatLine {
        FloatLine::new(FloatPoint::new(x, y), FloatPoint::new(x, y))
    }

    /// A04.2: the fanout filter at the exact boundary distances (resolution
    /// 10 units per micrometre: 4.5 mm is 45000, 2.5 mm 25000). An entry at
    /// exactly the maximum passes (`>` refuses), a drill at exactly the
    /// minimum passes (`<` refuses); off the start layer the maximum does
    /// not apply.
    #[test]
    fn fanout_filter_boundaries() {
        let centre = FloatPoint::new(1000.0, -2000.0);
        let filter = FanoutFilter::with_lengths(centre, 0, 4.5, 2.5, false, 10.0);
        assert_eq!(filter.max_escape, 45000.0);
        assert_eq!(filter.min_escape, 25000.0);
        let at_max = entry(1000.0 + 45000.0, -2000.0);
        assert!(filter.admits(&at_max, Some(0), None));
        let beyond_max = entry(1000.0 + 45000.0 + 1e-6, -2000.0);
        assert!(!filter.admits(&beyond_max, Some(0), None));
        assert!(filter.admits(&beyond_max, Some(1), None));
        assert!(filter.admits(&beyond_max, None, None));
        // A diagonal entry: the midpoint of the entry line counts.
        let line = FloatLine::new(
            FloatPoint::new(1000.0, -2000.0),
            FloatPoint::new(1000.0, 88000.0),
        );
        assert!(filter.admits(&line, Some(0), None));
        let near = entry(1000.0, -2000.0);
        let at_min = FloatPoint::new(1000.0, -2000.0 + 25000.0);
        assert!(filter.admits(&near, Some(0), Some(at_min)));
        let inside_min = FloatPoint::new(1000.0, -2000.0 + 25000.0 - 1e-6);
        assert!(!filter.admits(&near, Some(1), Some(inside_min)));
        // A plane fanout allows a drill at the pin centre.
        let plane = FanoutFilter::with_lengths(centre, 0, 4.5, 2.5, true, 10.0);
        assert_eq!(plane.min_escape, 0.0);
        assert!(plane.admits(&near, Some(0), Some(centre)));
    }

    /// Port of `MazeListElementTest.compareToReturnsZeroForSameInstance`.
    #[test]
    fn compare_to_returns_zero_for_same_instance() {
        let key = QueueKey::new(1.0, 0.0, 1, 0);
        assert_eq!(key.cmp(&key), Ordering::Equal);
    }

    /// Port of `MazeListElementTest.compareToSortsBySortingValue`.
    #[test]
    fn compare_to_sorts_by_sorting_value() {
        let mut queue = MazeQueue::new();
        assert!(queue.add(QueueKey::new(2.0, 0.0, 2, 0), element(2)));
        assert!(queue.add(QueueKey::new(1.0, 0.0, 1, 0), element(1)));
        let (key, first) = queue.pop_first().unwrap();
        assert_eq!(key.sorting_value, 1.0);
        assert_eq!(first.door, ExpandableRef::Door(DoorId(1)));
    }

    #[test]
    fn ties_break_by_expansion_value_then_door_id_then_section() {
        let by_expansion = [
            QueueKey::new(1.0, 0.5, 9, 9),
            QueueKey::new(1.0, 0.25, 1, 0),
        ];
        assert_eq!(by_expansion[1].cmp(&by_expansion[0]), Ordering::Less);
        let by_door = [
            QueueKey::new(1.0, 0.5, 3, 9),
            QueueKey::new(1.0, 0.5, -4, 9),
        ];
        assert_eq!(by_door[1].cmp(&by_door[0]), Ordering::Less);
        let by_section = [QueueKey::new(1.0, 0.5, 3, 2), QueueKey::new(1.0, 0.5, 3, 1)];
        assert_eq!(by_section[1].cmp(&by_section[0]), Ordering::Less);
        let mut queue = MazeQueue::new();
        for (i, key) in by_expansion
            .iter()
            .chain(&by_door)
            .chain(&by_section)
            .enumerate()
        {
            queue.add(*key, element(i as u32));
        }
        let order: Vec<(f64, i32, i32)> = queue
            .iter()
            .map(|(k, _)| (k.expansion_value, k.door_id, k.section))
            .collect();
        assert_eq!(
            order,
            [
                (0.25, 1, 0),
                (0.5, -4, 9),
                (0.5, 3, 1),
                (0.5, 3, 2),
                (0.5, 3, 9),
                (0.5, 9, 9)
            ]
        );
    }

    #[test]
    fn a_full_tie_keeps_the_first_element() {
        let mut queue = MazeQueue::new();
        let key = QueueKey::new(1.0, 0.5, 3, 0);
        assert!(queue.add(key, element(1)));
        let mut second = element(2);
        second.door = ExpandableRef::TargetDoor(TargetDoorId(5));
        assert!(!queue.add(key, second), "the newcomer is dropped");
        // -0.0 and 0.0 tie as well.
        assert!(queue.add(QueueKey::new(0.0, 0.0, 1, 0), element(3)));
        assert!(!queue.add(QueueKey::new(-0.0, -0.0, 1, 0), element(4)));
        assert_eq!(queue.len(), 2);
        let first = queue.iter().next().unwrap();
        assert_eq!(first.0.sorting_value.to_bits(), 0.0f64.to_bits());
        let (_, element) = queue.iter().nth(1).unwrap();
        assert_eq!(element.door, ExpandableRef::Door(DoorId(1)));
    }
}
