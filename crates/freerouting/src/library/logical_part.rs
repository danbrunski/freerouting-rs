use crate::util::equals_ignore_case;

/// The id of a logical part: its 1-based position in [`LogicalParts`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LogicalPartId(pub usize);

/// A pin of a logical part, with its gate and swap information.
///
/// Port of `core.library.LogicalPart.PartPin`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PartPin {
    /// The index of the pin in the package of the part.
    pub pin_index: usize,
    pub pin_name: String,
    pub gate_name: String,
    /// Gates with the same non-zero swap code may be swapped.
    pub gate_swap_code: i32,
    pub gate_pin_name: String,
    /// Pins of a gate with the same non-zero swap code may be swapped.
    pub gate_pin_swap_code: i32,
}

/// The gate and pin swap information of a component type.
///
/// Port of `core.library.LogicalPart`.
#[derive(Clone, Debug)]
pub struct LogicalPart {
    pub name: String,
    pub id: LogicalPartId,
    pins: Vec<PartPin>,
}

impl LogicalPart {
    pub fn pin_count(&self) -> usize {
        self.pins.len()
    }

    pub fn get_pin(&self, index: usize) -> Option<&PartPin> {
        self.pins.get(index)
    }
}

/// The logical parts of a board.
///
/// Port of `core.library.LogicalParts`.
#[derive(Clone, Debug, Default)]
pub struct LogicalParts {
    parts: Vec<LogicalPart>,
}

impl LogicalParts {
    /// Adds a part; its pins are sorted by pin index (stable, as Java's
    /// `Arrays.sort`).
    pub fn add(&mut self, name: impl Into<String>, mut pins: Vec<PartPin>) -> LogicalPartId {
        pins.sort_by_key(|pin| pin.pin_index);
        let id = LogicalPartId(self.parts.len() + 1);
        self.parts.push(LogicalPart {
            name: name.into(),
            id,
            pins,
        });
        id
    }

    /// The part named `name` (any case).
    pub fn get_by_name(&self, name: &str) -> Option<&LogicalPart> {
        self.parts
            .iter()
            .find(|part| equals_ignore_case(&part.name, name))
    }

    pub fn get(&self, id: LogicalPartId) -> Option<&LogicalPart> {
        if id.0 == 0 {
            return None;
        }
        self.parts.get(id.0 - 1)
    }

    pub fn count(&self) -> usize {
        self.parts.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pin(index: usize, name: &str) -> PartPin {
        PartPin {
            pin_index: index,
            pin_name: name.into(),
            gate_name: "A".into(),
            gate_swap_code: 0,
            gate_pin_name: name.into(),
            gate_pin_swap_code: 0,
        }
    }

    #[test]
    fn pins_are_sorted_by_index() {
        let mut parts = LogicalParts::default();
        let id = parts.add("NAND", vec![pin(2, "c"), pin(0, "a"), pin(1, "b")]);
        let part = parts.get_by_name("nand").unwrap();
        assert_eq!(part.id, id);
        let names: Vec<_> = (0..part.pin_count())
            .map(|i| part.get_pin(i).unwrap().pin_name.clone())
            .collect();
        assert_eq!(names, ["a", "b", "c"]);
    }
}
