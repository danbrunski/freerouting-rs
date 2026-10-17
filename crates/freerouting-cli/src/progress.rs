//! `--progress json`: the run as JSON lines on standard error (C08.3).
//!
//! With the flag every stderr line is one JSON object with an `event`:
//!
//! - `start`, first: `{"event":"start","schema_version":1,"mode":"route"}`;
//! - `progress`: `phase` (`load`, `fanout`, `route`, `optimize` or
//!   `write`), `pass`, `routed`, `failed`, `ripped`, `incomplete`,
//!   `violations`, `score`, `pass_end` and `elapsed_ms`. `pass_end` is true
//!   for the event that closes a routing pass (it carries the figures of the
//!   pass log line). A figure that is not known at
//!   that point (the router's counters during the load) is `null`;
//! - `log`: a log line, `{"event":"log","level":...,"text":...}`;
//! - `done`, last: `exit_code`, `state` (`completed`, `timed_out`,
//!   `cancelled`, `terminated` or `usage_error`) and `elapsed_ms`.
//!
//! **Cadence.** A `progress` event is sent when the phase changes, at the
//! end of each routing pass (the figures of the pass log line), at the
//! start of each fanout pass and every [`CONNECTION_INTERVAL`] connections
//! within a pass. All of that is counted from the router's events, never
//! timed, so apart from `elapsed_ms` the stream is the same on every run.
//!
//! [`JsonSink`] is the listener: the job runner hands it the router's
//! counters ([`JobEvent::Progress`]) and its own `load` and `write` marks.
//! The optimizer sends no events until O06; the job runner sends one
//! `optimize` event at its end when the optimizer ran.

use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use freerouting::autoroute::events::{Phase, Progress};
use freerouting::job::run::{JobEvent, JobObserver};
use freerouting::job::{LogLevel, LogLine};

use crate::exit::Code;
use crate::log::ConsoleLevel;

/// The version of the event schema.
pub const SCHEMA_VERSION: u32 = 1;

/// How many connections of a pass go by between two `progress` events.
pub const CONNECTION_INTERVAL: u32 = 50;

/// The figures of one `progress` event; `None` is `null`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Figures {
    pass: Option<i64>,
    routed: Option<u64>,
    failed: Option<u64>,
    ripped: Option<u64>,
    incomplete: Option<u64>,
    violations: Option<u64>,
    score: Option<f64>,
}

#[derive(Default)]
struct State {
    /// The phase of the last event.
    phase: Option<&'static str>,
    /// The pass of the last router event.
    pass: i32,
    /// Router events since the last `progress` event of this pass.
    since_emit: u32,
    /// The counters of the latest router event of the pass.
    routed: u64,
    failed: u64,
    ripped: u64,
}

/// The writer of the JSON lines and the state that decides when to write
/// one.
pub struct JsonSink {
    out: Mutex<Box<dyn Write + Send>>,
    state: Mutex<State>,
    start: Instant,
}

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn number<T: std::fmt::Display>(value: Option<T>) -> String {
    value.map_or_else(|| "null".to_string(), |v| v.to_string())
}

fn level_name(level: LogLevel) -> &'static str {
    match level {
        LogLevel::Debug => "debug",
        LogLevel::Info => "info",
        LogLevel::Warn => "warn",
        LogLevel::Error => "error",
    }
}

fn phase_name(phase: Phase) -> &'static str {
    match phase {
        Phase::Fanout => "fanout",
        Phase::Autoroute => "route",
        Phase::Optimize => "optimize",
    }
}

impl JsonSink {
    /// A sink that writes to `out`.
    pub fn new(out: Box<dyn Write + Send>) -> JsonSink {
        JsonSink {
            out: Mutex::new(out),
            state: Mutex::new(State::default()),
            start: Instant::now(),
        }
    }

    /// A sink that writes to standard error.
    pub fn stderr() -> JsonSink {
        JsonSink::new(Box::new(std::io::stderr()))
    }

    fn elapsed_ms(&self) -> u128 {
        self.start.elapsed().as_millis()
    }

    fn line(&self, text: &str) {
        let mut out = self.out.lock().unwrap_or_else(|e| e.into_inner());
        // A closed stderr is not an error for the run.
        let _ = writeln!(out, "{text}");
        let _ = out.flush();
    }

    /// The `start` event.
    pub fn start(&self, mode: &str) {
        self.line(&format!(
            "{{\"event\":\"start\",\"schema_version\":{SCHEMA_VERSION},\"mode\":{}}}",
            escape(mode)
        ));
    }

    /// `log` events for `lines`.
    pub fn log(&self, lines: &[LogLine]) {
        for line in lines {
            self.line(&format!(
                "{{\"event\":\"log\",\"level\":{},\"text\":{}}}",
                escape(level_name(line.level)),
                escape(&line.text)
            ));
        }
    }

    /// The `done` event; `state` is the job's state name or
    /// [`Code::state_name`].
    pub fn done(&self, code: Code, state: &str) {
        self.line(&format!(
            "{{\"event\":\"done\",\"exit_code\":{},\"state\":{},\"elapsed_ms\":{}}}",
            code.value(),
            escape(state),
            self.elapsed_ms()
        ));
    }

    fn progress_line(&self, phase: &str, figures: Figures, pass_end: bool) {
        self.line(&format!(
            "{{\"event\":\"progress\",\"phase\":{},\"pass\":{},\"routed\":{},\"failed\":{},\
             \"ripped\":{},\"incomplete\":{},\"violations\":{},\"score\":{},\"pass_end\":{},\"elapsed_ms\":{}}}",
            escape(phase),
            number(figures.pass),
            number(figures.routed),
            number(figures.failed),
            number(figures.ripped),
            number(figures.incomplete),
            number(figures.violations),
            figures
                .score
                .filter(|s| s.is_finite())
                .map_or_else(|| "null".to_string(), |s| format!("{s:.2}")),
            pass_end,
            self.elapsed_ms()
        ));
    }

    fn emit(&self, state: &mut State, phase: &'static str, figures: Figures, pass_end: bool) {
        state.phase = Some(phase);
        state.since_emit = 0;
        self.progress_line(phase, figures, pass_end);
    }

    /// The load has begun.
    fn load(&self) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        self.emit(&mut state, "load", Figures::default(), false);
    }

    /// One router event.
    fn router(&self, p: &Progress) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let phase = phase_name(p.phase);
        let count = |n: usize| u64::try_from(n).unwrap_or(u64::MAX);
        let figures = |state: &State, p: &Progress| Figures {
            pass: Some(i64::from(p.pass)),
            routed: Some(state.routed),
            failed: Some(state.failed),
            ripped: Some(state.ripped),
            incomplete: Some(count(p.incomplete)),
            violations: Some(count(p.violations)),
            score: Some(f64::from(p.score)),
        };
        if p.pass_end {
            // The counters are those of the pass's last event; the rest is
            // the pass line's.
            let f = figures(&state, p);
            self.emit(&mut state, phase, f, true);
            state.pass = p.pass;
            state.routed = 0;
            state.failed = 0;
            state.ripped = 0;
            return;
        }
        let new_phase = state.phase != Some(phase);
        let new_fanout_pass = p.phase == Phase::Fanout && state.pass != p.pass;
        if new_phase || state.pass != p.pass {
            state.routed = 0;
            state.failed = 0;
            state.ripped = 0;
        }
        state.pass = p.pass;
        state.routed = count(p.routed);
        state.failed = count(p.failed);
        state.ripped = count(p.ripped);
        state.since_emit += 1;
        if new_phase || new_fanout_pass || state.since_emit >= CONNECTION_INTERVAL {
            let f = figures(&state, p);
            self.emit(&mut state, phase, f, false);
        }
    }

    /// The outputs are about to be written.
    fn write(&self, summary: Option<&freerouting::autoroute::events::RunSummary>) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let count = |n: usize| u64::try_from(n).unwrap_or(u64::MAX);
        let figures = summary.map_or_else(Figures::default, |s| Figures {
            pass: Some(i64::from(s.passes)),
            incomplete: Some(count(s.incomplete)),
            violations: Some(count(s.violations)),
            score: Some(f64::from(s.score)),
            ..Figures::default()
        });
        self.emit(&mut state, "write", figures, false);
    }

    /// The listener the job runner takes ([`JobContext::observer`]).
    ///
    /// [`JobContext::observer`]: freerouting::job::run::JobContext::observer
    pub fn observer(self: &Arc<Self>) -> JobObserver {
        let sink = Arc::clone(self);
        Arc::new(move |event| match event {
            JobEvent::Load => sink.load(),
            JobEvent::Progress(p) => sink.router(p),
            JobEvent::Write(summary) => sink.write(summary),
            JobEvent::Log(lines) => sink.log(lines),
        })
    }
}

/// Where the CLI's log lines go: the console, or JSON events.
#[derive(Clone)]
pub struct Printer {
    level: ConsoleLevel,
    json: Option<Arc<JsonSink>>,
}

impl Printer {
    /// A printer for `level`; with a sink every line becomes a `log` event.
    pub fn new(level: ConsoleLevel, json: Option<Arc<JsonSink>>) -> Printer {
        Printer { level, json }
    }

    /// The JSON sink, if `--progress json` is on.
    pub fn json(&self) -> Option<&Arc<JsonSink>> {
        self.json.as_ref()
    }

    /// Prints the lines the console level lets through.
    pub fn print(&self, lines: &[LogLine]) {
        match &self.json {
            Some(sink) => {
                let kept: Vec<LogLine> = lines
                    .iter()
                    .filter(|l| self.level.allows(l.level))
                    .cloned()
                    .collect();
                sink.log(&kept);
            }
            None => crate::log::print(lines, self.level),
        }
    }

    /// The console level.
    pub fn level(&self) -> ConsoleLevel {
        self.level
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Default)]
    struct Buffer(Arc<Mutex<Vec<u8>>>);

    impl Write for Buffer {
        fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(data);
            Ok(data.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn sink() -> (Arc<JsonSink>, Buffer) {
        let buffer = Buffer::default();
        (Arc::new(JsonSink::new(Box::new(buffer.clone()))), buffer)
    }

    fn lines(buffer: &Buffer) -> Vec<String> {
        String::from_utf8(buffer.0.lock().unwrap().clone())
            .unwrap()
            .lines()
            .map(str::to_string)
            .collect()
    }

    fn event(pass: i32, phase: Phase, pass_end: bool) -> Progress {
        Progress {
            phase,
            pass,
            pass_end,
            incomplete: 4,
            ..Progress::default()
        }
    }

    #[test]
    fn strings_are_escaped() {
        assert_eq!(escape("a\"b\\c\nd\u{1}"), "\"a\\\"b\\\\c\\nd\\u0001\"");
    }

    #[test]
    fn log_lines_become_events() {
        let (sink, buffer) = sink();
        sink.log(&[LogLine {
            level: LogLevel::Warn,
            text: "say \"hi\"".to_string(),
        }]);
        assert_eq!(
            lines(&buffer),
            [r#"{"event":"log","level":"warn","text":"say \"hi\""}"#]
        );
    }

    /// Events go out on the phase change, every 50 connections and at the
    /// end of a pass, and the pass-end event carries the pass line's figures.
    #[test]
    fn the_cadence_is_counted() {
        let (sink, buffer) = sink();
        let observer = sink.observer();
        observer(JobEvent::Load);
        for _ in 0..120 {
            observer(JobEvent::Progress(&event(1, Phase::Autoroute, false)));
        }
        observer(JobEvent::Progress(&event(1, Phase::Autoroute, true)));
        observer(JobEvent::Progress(&event(2, Phase::Autoroute, false)));
        observer(JobEvent::Write(None));
        let got = lines(&buffer);
        let phases: Vec<&str> = got
            .iter()
            .map(|l| {
                let at = l.find("\"phase\":\"").unwrap() + 9;
                &l[at..at + l[at..].find('"').unwrap()]
            })
            .collect();
        // load, first route event, the 50th and 100th connection, the pass
        // end, (pass 2 starts: 1 event, no emit), write.
        assert_eq!(
            phases,
            ["load", "route", "route", "route", "route", "write"]
        );
        assert!(got[4].contains("\"pass\":1") && got[4].contains("\"incomplete\":4"));
        assert!(got[0].contains("\"routed\":null") && got[0].contains("\"score\":null"));
    }
}
