//! Jobs: Java's load sequence for routing (C03).
//!
//! - [`overrides`]: the five board overrides Java applies at load time.
//! - [`prepare`]: `prepare_job`, the whole load sequence in Java's order.

pub mod overrides;
pub mod prepare;

/// How loud a [`LogLine`] is (Java's `FRLogger` levels).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

/// One line of Java's log output during a job.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogLine {
    pub level: LogLevel,
    pub text: String,
}
