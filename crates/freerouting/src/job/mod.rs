//! Jobs: Java's load sequence for routing (C03).
//!
//! - [`overrides`]: the five board overrides Java applies at load time.
//! - [`prepare`]: `prepare_job`, the whole load sequence in Java's order.
//!
//! The job flow of the CLI (C07), which the GUI's front-end mode reuses:
//!
//! - [`run`]: `run_job`, prepare, route (a stub until C07.8) and write the
//!   outputs; the job states and Java's exit code.
//! - [`outputs`]: the output formats and the atomic, never-empty writer.
//! - [`deadline`]: wall-clock limits that only stop work, and the mapping to
//!   a terminal state.
//! - [`log`]: the log lines tools parse and the score format.

pub mod deadline;
pub mod log;
pub mod outputs;
pub mod overrides;
pub mod prepare;
pub mod run;

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
