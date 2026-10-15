//! Board structure: layers, units, components and fixed states.
//!
//! Port of `board.model.structure`. `BoardOutline` is an item and lives in
//! [`crate::board::items`].

mod component;
mod layer;

pub use component::{Component, ComponentId, Components};
pub use layer::{Layer, LayerStructure};

/// How firmly an item is fixed on the board.
///
/// Port of `board.model.structure.FixedState`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum FixedState {
    /// The item may be moved or deleted.
    #[default]
    Unfixed,
    /// The item may not be shoved.
    ShoveFixed,
    /// Fixed by the user.
    UserFixed,
    /// Fixed by the system.
    SystemFixed,
}

impl From<freerouting_dsn::ast::FixedState> for FixedState {
    fn from(state: freerouting_dsn::ast::FixedState) -> Self {
        use freerouting_dsn::ast::FixedState as F;
        match state {
            F::Unfixed => FixedState::Unfixed,
            F::ShoveFixed => FixedState::ShoveFixed,
            F::UserFixed => FixedState::UserFixed,
            F::SystemFixed => FixedState::SystemFixed,
        }
    }
}

/// The trace angle restriction of a board.
///
/// Port of `board.model.structure.AngleRestriction`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum AngleRestriction {
    None,
    #[default]
    FortyfiveDegree,
    NinetyDegree,
}

/// A length unit.
///
/// Port of `board.model.structure.Unit`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Unit {
    Mil,
    Inch,
    Mm,
    Um,
}

impl Unit {
    /// The length of the unit in micrometers.
    pub fn micrometers(self) -> f64 {
        match self {
            Unit::Mil => 25.4,
            Unit::Inch => 25_400.0,
            Unit::Mm => 1000.0,
            Unit::Um => 1.0,
        }
    }

    /// Converts `value` from `from_unit` to `to_unit`.
    pub fn scale(value: f64, from_unit: Unit, to_unit: Unit) -> f64 {
        value * from_unit.micrometers() / to_unit.micrometers()
    }

    /// The unit with name `name` (any case).
    pub fn from_name(name: &str) -> Option<Unit> {
        match name.to_ascii_lowercase().as_str() {
            "mil" => Some(Unit::Mil),
            "inch" => Some(Unit::Inch),
            "mm" => Some(Unit::Mm),
            "um" => Some(Unit::Um),
            _ => None,
        }
    }

    /// The lower case name.
    pub fn name(self) -> &'static str {
        match self {
            Unit::Mil => "mil",
            Unit::Inch => "inch",
            Unit::Mm => "mm",
            Unit::Um => "um",
        }
    }
}

impl From<freerouting_dsn::ast::Unit> for Unit {
    fn from(unit: freerouting_dsn::ast::Unit) -> Self {
        use freerouting_dsn::ast::Unit as U;
        match unit {
            U::Mil => Unit::Mil,
            U::Inch => Unit::Inch,
            U::Mm => Unit::Mm,
            U::Um => Unit::Um,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_scale() {
        assert_eq!(Unit::scale(1.0, Unit::Inch, Unit::Mil), 1000.0);
        assert_eq!(Unit::scale(2540.0, Unit::Um, Unit::Mm), 2.54);
        assert_eq!(Unit::from_name("MM"), Some(Unit::Mm));
        assert!(FixedState::UserFixed > FixedState::ShoveFixed);
    }
}
