//! Stopping a run from outside: stdin lines and interrupts (C08.5).
//!
//! **Java's two levels.** `StoppableThread` has `requestStopAutoRouter`
//! (the router ends, the optimizer still runs) and `requestStop` (everything
//! ends). A08's [`CancelToken`] has both: [`CancelToken::stop_router`] and
//! [`CancelToken::stop_all`].
//!
//! **Stdin.** A reader thread acts on whole lines, ignoring case and
//! surrounding whitespace:
//!
//! - `stop`: stop everything. The step in progress ends, the best board is
//!   written and the process exits 0;
//! - `stop-router`: stop the router and let the optimizer run.
//!
//! Anything else is ignored. **End of input is not a stop**: the plugin
//! passes `stdin=DEVNULL`, which is an immediate EOF.
//!
//! **Interrupts.** [`Interrupts`] is the policy for Ctrl-C and SIGTERM: the
//! first is `stop`, the second aborts the run (the job ends `Cancelled`,
//! nothing is written, exit 130). A stop that arrives while the board is
//! still loading also ends the run as `Cancelled`, with nothing written
//! (`run_job`). Installing the signal handlers needs the `ctrlc` crate, a
//! `NEW DEP` the operator has not approved, so nothing calls
//! [`Interrupts::interrupt`] from a signal yet; the test hook
//! ([`TEST_INTERRUPT_VAR`]) injects interrupts instead, and a signal still
//! kills the process as before (C07's temporary-file write leaves no partial
//! output).

use std::io::BufRead;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::thread::JoinHandle;

use freerouting::autoroute::events::{CancelToken, Phase};
use freerouting::job::run::{JobContext, JobEvent};

/// The lines the reader understands, for `--capabilities`.
pub const COMMANDS: &[&str] = &["stop", "stop-router"];

/// The environment variable that injects interrupts for tests: `load:N` sends
/// N interrupts when the load begins, `route:N` when the first routing event
/// arrives.
pub const TEST_INTERRUPT_VAR: &str = "FREEROUTING_TEST_INTERRUPT";

/// A command on standard input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    /// `stop`: [`CancelToken::stop_all`].
    Stop,
    /// `stop-router`: [`CancelToken::stop_router`].
    StopRouter,
}

impl Command {
    /// The command a line names, if any.
    pub fn parse(line: &str) -> Option<Command> {
        match line.trim().to_ascii_lowercase().as_str() {
            "stop" => Some(Command::Stop),
            "stop-router" => Some(Command::StopRouter),
            _ => None,
        }
    }

    /// Applies the command to the token.
    pub fn apply(self, token: &CancelToken) {
        match self {
            Command::Stop => token.stop_all(),
            Command::StopRouter => token.stop_router(),
        }
    }
}

/// Reads `input` line by line until it ends, applying each command to
/// `token` and telling `on_command` about it. A read error ends the loop like
/// EOF does.
pub fn read_commands<R: BufRead>(
    input: R,
    token: &CancelToken,
    on_command: impl Fn(Command),
) -> usize {
    let mut applied = 0;
    for line in input.lines() {
        let Ok(line) = line else { break };
        if let Some(command) = Command::parse(&line) {
            command.apply(token);
            on_command(command);
            applied += 1;
        }
    }
    applied
}

/// Starts the reader on standard input. The thread ends at EOF; if standard
/// input stays open it is left behind when the process exits.
pub fn spawn_stdin_reader(
    token: Arc<CancelToken>,
    on_command: impl Fn(Command) + Send + 'static,
) -> JoinHandle<()> {
    std::thread::spawn(move || {
        let stdin = std::io::stdin();
        read_commands(stdin.lock(), &token, on_command);
    })
}

/// What an interrupt did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Interrupt {
    /// The first one: a full stop.
    Stop,
    /// A later one: the run is cancelled and writes nothing.
    Abort,
}

/// The interrupt policy of a run: the first interrupt is `stop`, the second
/// aborts.
pub struct Interrupts {
    context: JobContext,
    count: AtomicU32,
}

impl Interrupts {
    /// The policy for the run that `context` belongs to.
    pub fn new(context: JobContext) -> Interrupts {
        Interrupts {
            context,
            count: AtomicU32::new(0),
        }
    }

    /// One Ctrl-C or SIGTERM.
    pub fn interrupt(&self) -> Interrupt {
        if self.count.fetch_add(1, Ordering::AcqRel) == 0 {
            self.context.cancel.stop_all();
            Interrupt::Stop
        } else {
            self.context.cancel_by_user();
            Interrupt::Abort
        }
    }

    /// How many interrupts have arrived.
    pub fn count(&self) -> u32 {
        self.count.load(Ordering::Acquire)
    }
}

/// When the test hook fires.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HookPhase {
    Load,
    Route,
}

/// The test hook of [`TEST_INTERRUPT_VAR`]: sends interrupts through the same
/// path a signal would, at a fixed point of the run.
pub struct TestHook {
    phase: HookPhase,
    count: u32,
    fired: std::sync::atomic::AtomicBool,
}

impl TestHook {
    /// Reads `load:N` or `route:N`; anything else is no hook.
    pub fn parse(spec: &str) -> Option<TestHook> {
        let (phase, count) = spec.split_once(':')?;
        let phase = match phase {
            "load" => HookPhase::Load,
            "route" => HookPhase::Route,
            _ => return None,
        };
        Some(TestHook {
            phase,
            count: count.parse().ok()?,
            fired: std::sync::atomic::AtomicBool::new(false),
        })
    }

    /// Sends the interrupts if `event` is the point the hook waits for.
    pub fn observe(&self, event: &JobEvent<'_>, interrupts: &Interrupts) {
        let hit = match (self.phase, event) {
            (HookPhase::Load, JobEvent::Load) => true,
            (HookPhase::Route, JobEvent::Progress(p)) => p.phase == Phase::Autoroute,
            _ => false,
        };
        if hit && !self.fired.swap(true, Ordering::AcqRel) {
            for _ in 0..self.count {
                interrupts.interrupt();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;
    use std::sync::Mutex;

    use freerouting::settings::ResolveContext;

    use super::*;

    #[test]
    fn lines_are_matched_whole_ignoring_case_and_space() {
        assert_eq!(Command::parse("stop"), Some(Command::Stop));
        assert_eq!(Command::parse("  STOP \r"), Some(Command::Stop));
        assert_eq!(Command::parse("Stop-Router"), Some(Command::StopRouter));
        assert_eq!(Command::parse("stopp"), None);
        assert_eq!(Command::parse("please stop"), None);
        assert_eq!(Command::parse(""), None);
    }

    #[test]
    fn eof_is_not_a_stop() {
        let token = CancelToken::new();
        assert_eq!(read_commands(Cursor::new(""), &token, |_| {}), 0);
        assert!(!token.is_router_stop_requested());
        assert!(!token.is_stop_requested());
    }

    #[test]
    fn stop_router_stops_the_router_only() {
        let token = CancelToken::new();
        let seen = Mutex::new(Vec::new());
        let input = Cursor::new("noise\nstop-router\n");
        read_commands(input, &token, |c| seen.lock().unwrap().push(c));
        assert!(token.is_router_stop_requested());
        assert!(!token.is_stop_requested(), "the optimizer still runs");
        assert_eq!(*seen.lock().unwrap(), [Command::StopRouter]);
    }

    #[test]
    fn stop_stops_everything() {
        let token = CancelToken::new();
        read_commands(Cursor::new("STOP\nafter\n"), &token, |_| {});
        assert!(token.is_stop_requested());
    }

    #[test]
    fn the_second_interrupt_aborts() {
        let context = JobContext::new(ResolveContext { cores: 4 });
        let interrupts = Interrupts::new(context.clone());
        assert_eq!(interrupts.interrupt(), Interrupt::Stop);
        assert!(context.cancel.is_stop_requested());
        assert!(!context.cancelled_by_user.load(Ordering::Acquire));
        assert_eq!(interrupts.interrupt(), Interrupt::Abort);
        assert!(context.cancelled_by_user.load(Ordering::Acquire));
        assert_eq!(interrupts.count(), 2);
    }

    #[test]
    fn the_hook_fires_once() {
        assert!(TestHook::parse("later:1").is_none());
        assert!(TestHook::parse("route").is_none());
        let hook = TestHook::parse("load:2").unwrap();
        let context = JobContext::new(ResolveContext { cores: 4 });
        let interrupts = Interrupts::new(context);
        hook.observe(&JobEvent::Write(None), &interrupts);
        assert_eq!(interrupts.count(), 0);
        hook.observe(&JobEvent::Load, &interrupts);
        hook.observe(&JobEvent::Load, &interrupts);
        assert_eq!(interrupts.count(), 2);
    }
}
