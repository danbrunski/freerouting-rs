/// Creates unique ids for board items.
///
/// Port of the Java interface `datastructures.IdGenerator`.
pub trait IdGenerator {
    /// A new id, different from the ids returned before (until the counter
    /// wraps around).
    fn new_id(&mut self) -> i32;
    /// The largest id generated so far.
    fn max_generated_id(&self) -> i32;
}

/// The id generator of a board: counts up from 1 and wraps around at
/// `i32::MAX / 2`.
///
/// Port of `board.actions.ItemIdGenerator`.
#[derive(Clone, Debug, Default)]
pub struct ItemIdGenerator {
    last_generated_id: i32,
    wrap_around_count: i64,
}

impl ItemIdGenerator {
    const MAX_ID: i32 = i32::MAX / 2;

    pub fn new() -> Self {
        Self::default()
    }

    /// How often the counter wrapped around.
    pub fn wrap_around_count(&self) -> i64 {
        self.wrap_around_count
    }
}

impl IdGenerator for ItemIdGenerator {
    fn new_id(&mut self) -> i32 {
        if self.last_generated_id >= Self::MAX_ID {
            self.wrap_around_count += 1;
            self.last_generated_id = 0;
        }
        self.last_generated_id += 1;
        self.last_generated_id
    }

    fn max_generated_id(&self) -> i32 {
        self.last_generated_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_up_from_one() {
        let mut generator = ItemIdGenerator::new();
        assert_eq!(generator.new_id(), 1);
        assert_eq!(generator.new_id(), 2);
        assert_eq!(generator.max_generated_id(), 2);
    }

    #[test]
    fn wraps_around_at_half_of_int_max() {
        let mut generator = ItemIdGenerator {
            last_generated_id: ItemIdGenerator::MAX_ID,
            wrap_around_count: 0,
        };
        assert_eq!(generator.new_id(), 1);
        assert_eq!(generator.wrap_around_count(), 1);
    }
}
