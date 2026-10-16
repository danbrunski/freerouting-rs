//! `freerouting` command-line entry point.
//!
//! This file holds no behaviour: the arguments, the flow and the log live in
//! the library (`args`, `cli`, `log`), and the flow itself in the engine
//! (`freerouting::job::run::run_job`, `freerouting::drc::run_drc`). A gate in
//! `tests/settings_parity.rs` scans this file for anything else.

fn main() -> std::process::ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    freerouting_cli::cli::run(&argv)
}
