//! The sink requirements of the maze search (A04.8): one core, whatever
//! the sink.
//!
//! - `noop_and_counter_sinks_agree`: on every sampled connection of
//!   `tools/bench/sets/connection-parity.txt`, ripup off and on, the search
//!   with `NoopSink` and with `WorkCounters` reaches the same destination
//!   after the same steps, and the counters agree with the search's own
//!   counts; the `maze-log` written through `Tee<ProbeSink, WorkCounters>`
//!   equals the one written by walking the queue between steps.
//! - `stepped_search_matches_find_connection`: on `Issue026-J2_reference`,
//!   `Issue508-DAC2020_bm08` and `scoring-mixed-layer`, a search stepped
//!   one `step()` at a time with `ProgressEvents`, reading the GUI
//!   accessors between steps as `ExpandTestState` does, gives the same
//!   pops, destination and backtrack chain as `find_connection` with
//!   `NoopSink`.
//! - `hook_cost_timing` (`#[ignore]`, release): ten runs of every sampled
//!   connection of `connection-parity.txt` with `NoopSink`; compare its
//!   time with the same test built with `--cfg freerouting_no_search_hooks`.

mod autoroute_common;
mod autoroute_maze;

use std::collections::BTreeSet;
use std::sync::mpsc;
use std::time::Instant;

use autoroute_common::{Conn, Loaded, board_name, load, repo_root, router_control, sample};
use autoroute_maze::{Mode, maze_lines};
use freerouting::autoroute::session::search::{MazeSearch, SearchSettings};
use freerouting::autoroute::session::sink::{
    NoExtraCost, NoopSink, ProgressEvents, SearchSink, Step, WorkCounters,
};
use freerouting::autoroute::session::{ExpandableRef, Session};

/// The boards of `tools/bench/sets/connection-parity.txt`.
fn connection_parity_boards() -> Vec<String> {
    let path = repo_root().join("tools/bench/sets/connection-parity.txt");
    let text = std::fs::read_to_string(&path).expect("connection-parity.txt");
    text.lines()
        .map(|l| l.split('#').next().unwrap_or("").trim())
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect()
}

/// What a finished search reached.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Outcome {
    outcome: Option<Step>,
    steps: u64,
    pops: u64,
    destination: Option<(ExpandableRef, usize)>,
    /// The backtrack chain from the destination: Java ids and sections.
    chain: Vec<(i32, usize)>,
}

fn backtrack_chain(
    session: &Session<'_>,
    destination: Option<(ExpandableRef, usize)>,
) -> Vec<(i32, usize)> {
    let mut chain = Vec::new();
    let mut current = destination;
    while let Some((door, section)) = current {
        chain.push((session.expandable_java_id(door).unwrap_or(0), section));
        if chain.len() > 100_000 {
            break;
        }
        current = session
            .section_state(door, section)
            .and_then(|state| state.backtrack);
    }
    chain
}

fn outcome_of<S: SearchSink>(search: &MazeSearch<'_, '_, S, NoExtraCost>) -> Outcome {
    Outcome {
        outcome: search.outcome(),
        steps: search.steps(),
        pops: search.pops(),
        destination: search.destination(),
        chain: backtrack_chain(search.session(), search.destination()),
    }
}

/// Runs `body` on a fresh session of every routable sampled connection of
/// `dsn`, ripup off and on.
fn for_each_search(dsn: &str, mut body: impl FnMut(&str, &mut Session<'_>, &Conn, SearchSettings)) {
    let mut loaded = load(dsn);
    let conns = sample(&loaded.board);
    let settings = SearchSettings::from_router(&loaded.router);
    for ripup in [false, true] {
        for conn in &conns {
            if conn.state.is_some() {
                continue;
            }
            let Ok(control) = router_control(&loaded, conn.net, ripup) else {
                continue;
            };
            let label = format!("{}:{}:{}", board_name(dsn), ripup, conn.net);
            let Loaded { board, .. } = &mut loaded;
            let mut session = Session::new(board, &control, None, None);
            body(&label, &mut session, conn, settings);
            session.clear();
        }
    }
}

fn run_plain(session: &mut Session<'_>, conn: &Conn, settings: SearchSettings) -> Outcome {
    let mut search = MazeSearch::plain(session, settings);
    if search.init(&conn.start, &conn.dest).expect("init") {
        search.find_connection().expect("search");
    }
    outcome_of(&search)
}

#[test]
fn noop_and_counter_sinks_agree() {
    let mut failures = Vec::new();
    let mut searches = 0;
    for dsn in connection_parity_boards() {
        let mut noop = Vec::new();
        let mut counted = Vec::new();
        for_each_search(&dsn, |label, session, conn, settings| {
            noop.push((label.to_string(), run_plain(session, conn, settings)));
        });
        for_each_search(&dsn, |label, session, conn, settings| {
            let mut search =
                MazeSearch::new(session, WorkCounters::default(), NoExtraCost, settings);
            if search.init(&conn.start, &conn.dest).expect("init") {
                search.find_connection().expect("search");
            }
            let outcome = outcome_of(&search);
            let counters = search.sink().clone();
            if counters.pops != outcome.pops {
                failures.push(format!(
                    "{label}: WorkCounters counted {} pops, the search {}",
                    counters.pops, outcome.pops
                ));
            }
            counted.push((label.to_string(), outcome));
        });
        searches += noop.len();
        for ((label, a), (_, b)) in noop.iter().zip(&counted) {
            if a != b {
                failures.push(format!("{label}: NoopSink {a:?}\n  WorkCounters {b:?}"));
            }
        }
        let plain = maze_lines(&dsn, Mode::Plain);
        let hooked = maze_lines(&dsn, Mode::Hooked);
        if plain != hooked {
            let i = plain
                .iter()
                .zip(&hooked)
                .position(|(a, b)| a != b)
                .unwrap_or(plain.len().min(hooked.len()));
            failures.push(format!(
                "{}: maze-log through Tee<ProbeSink, WorkCounters> differs at line {}",
                board_name(&dsn),
                i + 1
            ));
        }
    }
    assert!(searches > 0, "no search ran");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn stepped_search_matches_find_connection() {
    let boards = [
        "fixtures/Issue026-J2_reference.dsn",
        "fixtures/Issue508-DAC2020_bm08.dsn",
        "fixtures/scoring-mixed-layer.dsn",
    ];
    let mut failures = Vec::new();
    let mut found = 0;
    for dsn in boards {
        let mut whole = Vec::new();
        for_each_search(dsn, |label, session, conn, settings| {
            whole.push((label.to_string(), run_plain(session, conn, settings)));
        });
        let mut stepped = Vec::new();
        let mut events = 0usize;
        for_each_search(dsn, |label, session, conn, settings| {
            let (sender, receiver) = mpsc::channel();
            let sink = ProgressEvents::new(25, sender);
            let mut search = MazeSearch::new(session, sink, NoExtraCost, settings);
            if search.init(&conn.start, &conn.dest).expect("init") {
                // ExpandTestState: one step, then read what the GUI draws.
                let mut seen_rooms = BTreeSet::new();
                while search.step().expect("step") == Step::Continue {
                    let _ = search.queue_len();
                    if let Some(current) = search.current()
                        && let Some(room) = current.next_room
                    {
                        seen_rooms.insert(room);
                    }
                    let _ = search.session().complete_rooms().len();
                }
            }
            let outcome = outcome_of(&search);
            drop(search);
            let received: Vec<_> = receiver.try_iter().collect();
            if received.len() as u64 != outcome.pops / 25 {
                failures.push(format!(
                    "{label}: {} progress events for {} pops",
                    received.len(),
                    outcome.pops
                ));
            }
            events += received.len();
            stepped.push((label.to_string(), outcome));
        });
        for ((label, a), (_, b)) in whole.iter().zip(&stepped) {
            if a.destination.is_some() {
                found += 1;
            }
            if a != b {
                failures.push(format!("{label}: find_connection {a:?}\n  stepped {b:?}"));
            }
        }
        assert_eq!(whole.len(), stepped.len());
        let _ = events;
    }
    assert!(found > 0, "no search reached a destination");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The cost of the hooks (A04.8): run in release, once as is and once
/// with `RUSTFLAGS='--cfg freerouting_no_search_hooks'`, and compare the
/// printed times.
#[test]
#[ignore = "timing; run in release on a quiet machine"]
fn hook_cost_timing() {
    let boards = connection_parity_boards();
    let mut total = std::time::Duration::ZERO;
    let mut pops = 0u64;
    for _ in 0..10 {
        for dsn in &boards {
            for_each_search(dsn, |_, session, conn, settings| {
                let begin = Instant::now();
                let mut search = MazeSearch::new(session, NoopSink, NoExtraCost, settings);
                if search.init(&conn.start, &conn.dest).expect("init") {
                    search.find_connection().expect("search");
                }
                total += begin.elapsed();
                pops += search.pops();
            });
        }
    }
    let hooks = if cfg!(freerouting_no_search_hooks) {
        "compiled out"
    } else {
        "on"
    };
    println!("hook_cost_timing: hooks {hooks}, {pops} pops, search time {total:?}");
}
