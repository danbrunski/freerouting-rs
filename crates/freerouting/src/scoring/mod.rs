//! Statistics of a board.
//!
//! Port of `core.scoring`: `BoardStatistics` (with the V2 scores) and
//! `BoardStatisticsBoundsCalculator`.

mod board_statistics;
mod bounds_calculator;

pub use board_statistics::{
    BoardStatistics, BoardStatisticsBends, BoardStatisticsBoard,
    BoardStatisticsClearanceViolations, BoardStatisticsComponents, BoardStatisticsConnections,
    BoardStatisticsDifficulty, BoardStatisticsFanout, BoardStatisticsItems, BoardStatisticsLayers,
    BoardStatisticsNets, BoardStatisticsPads, BoardStatisticsTraces, BoardStatisticsVias,
    OptimizerScoreSettings, Rect, RouterScoreSettings, is_pin_escaped,
};
pub use bounds_calculator::{BoardStatisticsBounds, calculate_bounds};
