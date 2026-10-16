//! The `freerouting` command line as a library, so tests can call it.
//!
//! - [`args`]: Java's two argument parsers as one (C07.1 to C07.3).
//! - [`cli`]: what the arguments do: help, the DRC mode and the job.
//! - [`log`]: the console printer for the engine's log lines (C07.6).
//!
//! The binary (`src/main.rs`) only calls [`cli::run`]. The CLI draws no
//! conclusions of its own about boards: it reads files, builds the engine's
//! `JobInput`, calls `run_job` or `run_drc` and prints what they return.

pub mod args;
pub mod cli;
pub mod log;
