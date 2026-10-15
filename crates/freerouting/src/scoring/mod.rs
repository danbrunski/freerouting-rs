//! Statistics of a board.
//!
//! Port of the board-model part of `core.scoring`: `BoardStatistics` and
//! `BoardStatisticsBoundsCalculator`.

mod board_statistics;
mod bounds_calculator;

pub use board_statistics::{
    BoardStatistics, BoardStatisticsBends, BoardStatisticsBoard, BoardStatisticsComponents,
    BoardStatisticsDifficulty, BoardStatisticsFanout, BoardStatisticsItems, BoardStatisticsLayers,
    BoardStatisticsNets, BoardStatisticsPads, BoardStatisticsTraces, BoardStatisticsVias, Rect,
};
pub use bounds_calculator::{BoardStatisticsBounds, calculate_bounds};
