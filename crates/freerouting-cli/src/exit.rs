//! The exit codes of the command line (C08.1).
//!
//! | Code | Meaning |
//! |---|---|
//! | 0 | An output was written, even with incomplete connections (the result manifest has the details). |
//! | 1 | Failure with no output: a load error, an engine error, a write failure. |
//! | 2 | A usage error found before any work. |
//! | 3 | `router.neck_down.strict` (Q02.2): an output was written, but some wide net has copper under its class width (the result manifest's `widths` has the details). |
//! | 130 | Aborted before an output could be written: a second Ctrl-C, or a stop during the load. |
//!
//! Java uses 1 for usage errors; codes 2, 3 and 130 are listed in DIVERGENCES.
//! Only `cli.rs` builds a [`Code`], and only the binary turns it into a
//! process exit status.

use std::process::ExitCode;

use freerouting::job::run::{JobOutcome, JobState};

/// How the process ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Code(u8);

impl Code {
    /// 0: an output was written.
    pub const fn success() -> Code {
        Code(0)
    }

    /// 1: a failure with no output.
    pub const fn failure() -> Code {
        Code(1)
    }

    /// 2: a usage error, found before any work.
    pub const fn usage() -> Code {
        Code(2)
    }

    /// 3: strict sizes (Q02.2) and copper shipped under its class width.
    pub const fn strict_sizes() -> Code {
        Code(3)
    }

    /// 130: aborted before an output could be written.
    pub const fn aborted() -> Code {
        Code(130)
    }

    /// The code of a finished job: a `Cancelled` job (a second interrupt, or
    /// a stop during the load) is 130, Java's `computeCliExitCode` gives 0
    /// or 1 otherwise.
    pub fn of_outcome(outcome: &JobOutcome) -> Code {
        if outcome.state == JobState::Cancelled {
            Code::aborted()
        } else if outcome.exit_code() == 0 {
            Code::success()
        } else {
            Code::failure()
        }
    }

    /// The numeric value.
    pub const fn value(self) -> u8 {
        self.0
    }

    /// The name `--progress json` gives the end of a run that had no job
    /// state of its own.
    pub const fn state_name(self) -> &'static str {
        match self.0 {
            0 => "completed",
            2 => "usage_error",
            130 => "cancelled",
            _ => "terminated",
        }
    }
}

impl From<Code> for ExitCode {
    fn from(code: Code) -> ExitCode {
        ExitCode::from(code.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_four_codes() {
        assert_eq!(Code::success().value(), 0);
        assert_eq!(Code::failure().value(), 1);
        assert_eq!(Code::usage().value(), 2);
        assert_eq!(Code::strict_sizes().value(), 3);
        assert_eq!(Code::aborted().value(), 130);
    }

    fn outcome(state: JobState, written: &[u64]) -> JobOutcome {
        JobOutcome {
            state,
            written: written
                .iter()
                .map(|s| (std::path::PathBuf::from("x"), *s))
                .collect(),
            log: Vec::new(),
            report: Default::default(),
        }
    }

    #[test]
    fn a_job_maps_to_its_code() {
        assert_eq!(
            Code::of_outcome(&outcome(JobState::Completed, &[5])),
            Code::success()
        );
        assert_eq!(
            Code::of_outcome(&outcome(JobState::TimedOut, &[5])),
            Code::success()
        );
        assert_eq!(
            Code::of_outcome(&outcome(JobState::Completed, &[])),
            Code::failure()
        );
        assert_eq!(
            Code::of_outcome(&outcome(JobState::Terminated, &[])),
            Code::failure()
        );
        assert_eq!(
            Code::of_outcome(&outcome(JobState::Cancelled, &[])),
            Code::aborted()
        );
    }
}
