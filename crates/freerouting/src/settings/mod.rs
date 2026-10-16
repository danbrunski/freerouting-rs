//! The router settings model: one field table, Java's names and Java's
//! merge.
//!
//! Port of Java's `settings` package (`RouterSettings` and the settings
//! classes it holds, `SettingsMerger`, `LegacyRouterSettingsBridge`, the
//! sources in `settings.sources`) and `util.ReflectionUtil`. It does not
//! apply settings to a board; that is C03.
//!
//! # The model
//!
//! - [`fields`] lists every field once, with its JSON path, Java field name,
//!   Gson alternates, flat alias, Java type, default and doc line, and
//!   generates [`SettingsPatch`] (Java's nullable `RouterSettings`, what
//!   one source says), [`RouterSettings`] (the resolved getter view) and
//!   [`FIELDS`]. The CLI, environment variables, JSON files, the comparison
//!   dump, the help text and the GUI settings form all walk [`FIELDS`].
//! - Java's types are kept: `Integer` is `i32`, `Long` `i64`, `Double` `f64`,
//!   `Float` `f32` (the score weights are `float` arithmetic), `String[]` a
//!   `Vec<String>`.
//! - [`resolve`] merges `(Source, SettingsPatch)` pairs with Java's
//!   precedence, quirks included: sources are sorted by priority with a
//!   stable sort, `Some` overwrites and `None` never does, arrays are
//!   replaced whole, and `layers` merges element by element only when the
//!   target is at least as long as the source. A rules file's
//!   `autoroute_settings` are applied on top of the merged settings by C04,
//!   so they override the CLI; that quirk is reproduced, not fixed.
//!
//! | Source | Priority |
//! |---|---|
//! | Defaults | 0 |
//! | Config file | 10 |
//! | DSN | 20 |
//! | Rules file (first merge only through `-dr`) | 40 |
//! | Environment | 55 |
//! | CLI | 60 |
//! | Second merge ([`Source::Api`]) | 70 |
//! | Testing | 80 |
//!
//! # Work bounds and deadlines (D2)
//!
//! [`RouterSettings::work_bounds`] carries the pass and item limits and the
//! fanout's per-pin budget, which is a work budget in nominal milliseconds
//! (one nominal ms is 64,000 search-tree node visits, J01); it is stored and
//! never converted to time. [`RouterSettings::deadlines`] carries the
//! wall-clock limits. Stage code takes `&WorkBounds` only; only the job
//! runner sees [`Deadlines`], and a deadline can only stop work.
//!
//! # Deliberate differences from Java
//!
//! - Machine-dependent defaults take an explicit `cores`
//!   ([`ResolveContext`]), never a thread count read from the machine.
//! - Environment variables are processed in sorted name order, so a
//!   collision between two variables has a platform-independent winner
//!   (Java iterates `System.getenv()` in hash order).
//! - The settings JSON parser is strict JSON plus comments; Gson's other
//!   lenient forms are errors. `null` entries of `ignore_net_classes` and
//!   `layers` are dropped or read as an empty layer.
//! - Hexadecimal floating point literals are not accepted as numbers.

pub mod autoroute_scope;
pub mod board;
pub mod cli_args;
pub mod config_path;
pub mod dump;
pub mod env;
pub mod fields;
pub mod json_file;
pub mod layers;
pub mod merge;
pub mod path;
pub mod profiles;
pub mod text;
pub mod timespan;
pub mod types;
pub mod validate;

use std::time::Duration;

pub use cli_args::{CliParse, parse_cli_arguments};
pub use dump::dump;
pub use fields::{FIELDS, RouterSettings, SECTIONS, SettingsPatch, default_patch, field_by_path};
pub use merge::{ResolveContext, Resolved, Source, defaults, resolve};
pub use path::SetError;
pub use types::{
    BoardUpdateStrategy, FieldDesc, FieldKind, FieldValue, Flat, ItemSelectionStrategy,
    LayerSettings, OptimizerScoringVersion, RouterScoringVersion,
};

use crate::scoring::{OptimizerScoreSettings, RouterScoreSettings};

/// The pass and item limits of one stage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StageBounds {
    /// Maximum passes; 0 means unlimited for the autorouter, none for the
    /// optimizer.
    pub max_passes: i32,
    pub max_items: i32,
}

/// The fanout's limits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FanoutBounds {
    pub max_passes: i32,
    pub max_items: i32,
    /// The per-pin budget in nominal milliseconds (64,000 search-tree node
    /// visits each). A work budget, never a duration.
    pub max_milliseconds_per_pin: i64,
}

/// The work limits stage code takes (D2): counts only, no wall-clock time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorkBounds {
    pub autorouter: StageBounds,
    pub fanout: FanoutBounds,
    pub optimizer: StageBounds,
}

/// The wall-clock limits. Only the job runner reads these, and a deadline
/// can only stop work, never change how it is done.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Deadlines {
    /// The job limit, `None` when the text is blank or unparsable (no
    /// limit). Capped at 24 h, as `RoutingJobSchedulerActionThread` does.
    pub job: Option<Duration>,
    pub fanout: Option<Duration>,
    pub optimizer: Option<Duration>,
}

/// `RoutingJobSchedulerActionThread.MAX_TIMEOUT`, in seconds.
const MAX_JOB_TIMEOUT_SECONDS: i64 = 24 * 60 * 60;

fn duration_of(text: Option<&str>) -> Option<Duration> {
    let seconds = timespan::parse_timespan_seconds(text?)?;
    Some(Duration::from_secs(seconds.max(0) as u64))
}

impl RouterSettings {
    /// The pass and item limits for the stages.
    pub fn work_bounds(&self) -> WorkBounds {
        WorkBounds {
            autorouter: StageBounds {
                max_passes: self.autorouter.max_passes,
                max_items: self.autorouter.max_items,
            },
            fanout: FanoutBounds {
                max_passes: self.fanout.max_passes,
                max_items: self.fanout.max_items,
                max_milliseconds_per_pin: self.fanout.max_milliseconds_per_pin,
            },
            optimizer: StageBounds {
                max_passes: self.optimizer.max_passes,
                max_items: self.optimizer.max_items,
            },
        }
    }

    /// The wall-clock limits.
    pub fn deadlines(&self) -> Deadlines {
        Deadlines {
            job: duration_of(self.job_timeout.as_deref())
                .map(|d| d.min(Duration::from_secs(MAX_JOB_TIMEOUT_SECONDS as u64))),
            fanout: duration_of(self.fanout.timeout.as_deref()),
            optimizer: duration_of(self.optimizer.timeout.as_deref()),
        }
    }

    /// Whether the autorouter runs (`getRunRouter`).
    pub fn run_router(&self) -> bool {
        self.autorouter.enabled
    }

    /// Whether the optimizer runs (`getRunOptimizer`).
    pub fn run_optimizer(&self) -> bool {
        self.optimizer.enabled
    }
}

impl RouterScoringSettings {
    /// The V2 router score weights, as `scoring` uses them.
    pub fn score_weights(&self) -> RouterScoreSettings {
        RouterScoreSettings {
            unrouted_free_fraction: self.unrouted_free_fraction,
            unrouted_first_half_weight: self.unrouted_first_half_weight,
            unrouted_second_half_weight: self.unrouted_second_half_weight,
            clearance_violation_count_weight: self.clearance_violation_count_weight,
            clearance_violation_depth_weight: self.clearance_violation_depth_weight,
            clearance_violation_depth_scale: self.clearance_violation_depth_scale,
        }
    }
}

impl OptimizerScoringSettings {
    /// The V2 optimizer score weights, as `scoring` uses them.
    pub fn score_weights(&self) -> OptimizerScoreSettings {
        OptimizerScoreSettings {
            excess_wire_length_weight: self.excess_wire_length_weight,
            excess_via_weight: self.excess_via_weight,
            excess_bend_weight: self.excess_bend_weight,
            length_floor: self.length_floor,
            difficulty_scale_floor: self.difficulty_scale_floor,
        }
    }
}

pub use fields::{OptimizerScoringSettings, RouterScoringSettings};
