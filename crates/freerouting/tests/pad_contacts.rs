//! N04: `router.pad_contacts = inside_pad`, a trace end whose round cap
//! overlaps a same-net pin's pad connects to the pin (KiCad's rule;
//! `board::contact_rule`).
//!
//! The fixture is KiCad's ecc83-pp demo exported with its tracks
//! (`fixtures/Issue690-ecc83.dsn`; its wires are byte-identical to the
//! export in the N04 notes). Its tracks end off the pad centre on 16 pins,
//! some just outside the pad, so Java's centre rule leaves 10 connections
//! open on a board KiCad calls fully connected. `inside_pad` leaves none, as
//! KiCad: with N04.4 a trace end whose cap overlaps a same-net trace's copper
//! (a T junction: on Net-(U1A-K) a track ends on an inner corner of another)
//! is a contact too.
//!
//! - N04.1: the incompletes, the routing ledger and the DRC agree under each
//!   rule; the contacts are symmetric; unset and `legacy` keep Java's rule;
//!   a snapshot keeps the rule; an unknown value fails the job.
//! - N04.2: pull-tight keeps every off-centre end on its pad (one end moves
//!   onto the pin's centre), the router and optimizer keep them, trace tails
//!   keep the traces, and the SES writes the ends unchanged and reads back to
//!   the same contacts.
//! - N04.4: the T junction is a contact of both traces, every contact of the
//!   board is symmetric, a walk along a connection stops at the junction as
//!   at a fork, the end cap test has no tolerance (an end just out of reach
//!   is no contact), and pull-tight splits a trace at a junction on its
//!   centre line (Java's end-to-end contact) and leaves it alone at one off
//!   it.

mod autoroute_common;

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;

use autoroute_common::repo_root;
use freerouting::autoroute::events::{CancelToken, NullEvents};
use freerouting::autoroute::pipeline::{PipelineJob, run_pipeline};
use freerouting::board::contact_rule::ContactRule;
use freerouting::board::items::{ItemId, StopConnectionOption};
use freerouting::board::structure::FixedState;
use freerouting::board::{BasicBoard, RoutingBoard};
use freerouting::geometry::{IntPoint, Point, Polyline};
use freerouting::io::read_session;
use freerouting::io::specctra::ses_writer::{SesStyle, write_ses};
use freerouting::job::prepare::{JobError, JobInput, PreparedJob, RulesChoice, prepare_job};
use freerouting::settings::profiles::TestingProfile;
use freerouting::settings::{ResolveContext, Source};

const ECC83: &str = "fixtures/Issue690-ecc83.dsn";
const INSIDE: &[(&str, &str)] = &[("pad_contacts", "inside_pad")];
const LEGACY: &[(&str, &str)] = &[("profile", "legacy")];
/// The DRC's unconnected items (dangling traces) of ecc83 under `inside_pad`.
const DANGLING_INSIDE: usize = 0;

fn prepare(dsn: &str, extra: &[(&str, &str)]) -> Result<PreparedJob, JobError> {
    let path = repo_root().join(dsn);
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{dsn}: {e}"));
    let mut profile = TestingProfile::default();
    profile.set_optimizer_enabled(true);
    profile.set_max_passes(20);
    for (key, value) in extra {
        profile
            .patch
            .set_path(key, value)
            .unwrap_or_else(|e| panic!("{key}: {e:?}"));
    }
    let mut input = JobInput::new(bytes, JobInput::design_name_of(dsn));
    input.rules = RulesChoice::Adjacent;
    input.dsn_dir = path.parent().map(Path::to_path_buf);
    input.extra_sources = vec![(Source::Testing, profile.patch)];
    prepare_job(&input, &ResolveContext { cores: 1 })
}

fn ecc83(extra: &[(&str, &str)]) -> PreparedJob {
    prepare(ECC83, extra).unwrap_or_else(|e| panic!("{ECC83}: {e}"))
}

fn route(prepared: PreparedJob) -> RoutingBoard {
    let job = PipelineJob {
        settings: &prepared.settings,
        board_settings: &prepared.board_settings,
        cancel: Arc::new(CancelToken::new()),
        events: &NullEvents,
        deadline: None,
        registry: None,
    };
    let mut outcome =
        run_pipeline(prepared.board, &job).unwrap_or_else(|f| panic!("{ECC83}: {}", f.error));
    outcome.board.take().expect("no listener took the board")
}

/// The DRC's incomplete count and the ledger's, which must agree.
fn incompletes(board: &BasicBoard) -> usize {
    let drc: usize = board.incompletes().iter().map(|n| n.count()).sum();
    assert_eq!(
        drc,
        board.ledger_incomplete_count(),
        "DRC and ledger disagree"
    );
    drc
}

/// A trace end that touches a pin off the pin's centre.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct OffCentreEnd {
    pin: ItemId,
    /// The end corner, as `(x, y)` (every end on the fixture is an integer
    /// point).
    at: (i32, i32),
}

fn int_xy(point: &Point) -> (i32, i32) {
    match point {
        Point::Int(p) => (p.x, p.y),
        other => panic!("rational end {other:?}"),
    }
}

/// Every trace end of `board` in contact with a pin off its centre.
fn off_centre_ends(board: &BasicBoard) -> BTreeSet<OffCentreEnd> {
    let mut result = BTreeSet::new();
    for id in board.traces() {
        let trace = board.item(id).and_then(|i| i.as_trace()).expect("trace");
        for (end, contacts) in [
            (trace.first_corner(), board.trace_start_contacts(id)),
            (trace.last_corner(), board.trace_end_contacts(id)),
        ] {
            for contact in contacts {
                let item = board.item(contact).expect("contact");
                let Some(drill) = item.drill().filter(|_| item.is_pin()) else {
                    continue;
                };
                if Point::Int(drill.center) != end {
                    result.insert(OffCentreEnd {
                        pin: contact,
                        at: int_xy(&end),
                    });
                }
            }
        }
    }
    result
}

/// The T junction of ecc83 (N04.4): on Net-(U1A-K) a track ends here, on
/// an inner corner of another track.
const T_JUNCTION: (i32, i32) = (1371600, -1155700);

/// The traces of `board` with an end corner at `at`.
fn traces_ending_at(board: &BasicBoard, at: (i32, i32)) -> usize {
    board
        .traces()
        .into_iter()
        .filter(|id| {
            let t = board.item(*id).and_then(|i| i.as_trace()).expect("trace");
            int_xy(&t.first_corner()) == at || int_xy(&t.last_corner()) == at
        })
        .count()
}

/// True if `end` is kept ([`end_kept`]) or a trace of `board` touches
/// `end.pin` at the pin's centre instead.
fn end_held(board: &BasicBoard, end: &OffCentreEnd) -> bool {
    let centre = board
        .item(end.pin)
        .and_then(|i| i.drill())
        .expect("pin")
        .center;
    end_kept(board, end)
        || end_kept(
            board,
            &OffCentreEnd {
                pin: end.pin,
                at: (centre.x, centre.y),
            },
        )
}

/// True if a trace of `board` ends at `end.at` and touches `end.pin` there.
fn end_kept(board: &BasicBoard, end: &OffCentreEnd) -> bool {
    board.traces().into_iter().any(|id| {
        let trace = board.item(id).and_then(|i| i.as_trace()).expect("trace");
        [
            (trace.first_corner(), board.trace_start_contacts(id)),
            (trace.last_corner(), board.trace_end_contacts(id)),
        ]
        .into_iter()
        .any(|(corner, contacts)| int_xy(&corner) == end.at && contacts.contains(&end.pin))
    })
}

// ---------------------------------------------------------------------
// N04.1: the contact rule
// ---------------------------------------------------------------------

#[test]
fn inside_pad_connects_ecc83s_off_centre_track_ends() {
    let legacy = ecc83(LEGACY);
    assert_eq!(legacy.board.contact_rule(), ContactRule::Center);
    assert_eq!(incompletes(&legacy.board), 10, "Java's count");
    assert!(off_centre_ends(&legacy.board).is_empty());
    // N04.2: the DRC's unconnected-items check (dangling traces) agrees.
    let dangling = |board: &BasicBoard| board.design_rules_check().unconnected_items.len();
    assert_eq!(dangling(&legacy.board), 15);

    let inside = ecc83(INSIDE);
    assert_eq!(inside.board.contact_rule(), ContactRule::InsidePad);
    // Sixteen off-centre track ends now touch their pins: the valve socket
    // U1's and seven more, among them ends 15-89 µm outside their pad whose
    // round end overlaps it (Net-(P1-PM), Net-(P4-P1), Net-(U1A-K)).
    let ends = off_centre_ends(&inside.board);
    assert_eq!(ends.len(), 16, "{ends:?}");
    // KiCad's 0: the last one, on Net-(U1A-K), is the T junction of N04.4
    // (a track ends on an inner corner of another track).
    assert_eq!(incompletes(&inside.board), 0);
    assert_eq!(dangling(&inside.board), DANGLING_INSIDE);
}

#[test]
fn inside_pad_contacts_are_symmetric() {
    let inside = ecc83(INSIDE);
    let board = &inside.board;
    let ends = off_centre_ends(board);
    assert!(!ends.is_empty());
    for end in &ends {
        let traces: Vec<ItemId> = board
            .normal_contacts(end.pin)
            .into_iter()
            .filter(|c| board.item(*c).is_some_and(|i| i.is_trace()))
            .collect();
        let pin = board.item(end.pin).expect("pin");
        let centre = Point::Int(pin.drill().expect("pin").center);
        // The contact point is the end in the pad, or Java's centre when the
        // trace's other end is at the centre of the same pin.
        let found = traces.iter().any(|t| {
            let trace = board.item(*t).expect("trace");
            let point = board.normal_contact_point(trace, pin);
            point.is_some()
                && point == board.normal_contact_point(pin, trace)
                && point.is_some_and(|p| int_xy(&p) == end.at || p == centre)
        });
        assert!(found, "{end:?}: the pin does not see the trace end");
    }
}

#[test]
fn unset_and_legacy_keep_javas_rule() {
    // Profile `enhanced` does not set the key until N04.3 accepts it.
    assert_eq!(ecc83(&[]).board.contact_rule(), ContactRule::Center);
    assert_eq!(ecc83(LEGACY).board.contact_rule(), ContactRule::Center);
    let explicit = ecc83(&[("pad_contacts", "center")]);
    assert_eq!(explicit.board.contact_rule(), ContactRule::Center);
    assert_eq!(incompletes(&explicit.board), 10);
    // An explicit key beats the profile.
    let legacy_inside = ecc83(&[("profile", "legacy"), ("pad_contacts", "inside_pad")]);
    assert_eq!(legacy_inside.board.contact_rule(), ContactRule::InsidePad);
}

#[test]
fn an_unknown_pad_contacts_value_fails_the_job() {
    match prepare(ECC83, &[("pad_contacts", "overlap")]) {
        Err(JobError::Settings(text)) => {
            assert!(text.contains("router.pad_contacts"), "{text}");
            assert!(text.contains("'overlap'"), "{text}");
        }
        Err(other) => panic!("wrong error: {other}"),
        Ok(_) => panic!("an unknown value was accepted"),
    }
}

#[test]
fn a_board_copy_keeps_the_rule_and_its_contacts() {
    let inside = ecc83(INSIDE);
    let copy = inside.board.deep_copy();
    assert_eq!(copy.contact_rule(), ContactRule::InsidePad);
    assert_eq!(incompletes(&copy), 0);
    assert_eq!(off_centre_ends(&copy), off_centre_ends(&inside.board));
}

#[test]
fn routing_ecc83_under_inside_pad_keeps_its_tracks_connected() {
    let legacy = route(ecc83(LEGACY));
    assert_eq!(incompletes(&legacy), 6, "Java's result (route.log)");
    let prepared = ecc83(INSIDE);
    let ends = off_centre_ends(&prepared.board);
    let routed = route(prepared);
    assert_eq!(incompletes(&routed), 0);
    // The router and the optimizer keep every off-centre end and its
    // contact (the DSN's tracks load user-fixed).
    for end in &ends {
        assert!(end_kept(&routed, end), "{end:?} moved or lost its contact");
    }
}

// ---------------------------------------------------------------------
// N04.2: everything that walks connectivity agrees
// ---------------------------------------------------------------------

#[test]
fn pull_tight_keeps_off_centre_ends_inside_the_pad() {
    let mut inside = ecc83(INSIDE);
    let ends = off_centre_ends(&inside.board);
    let accuracy = inside.settings.trace_pull_tight_accuracy;
    // The DSN's tracks load user-fixed; pull-tight moves only unfixed ones.
    for id in inside.board.traces() {
        inside.board.unfix(id);
    }
    for id in inside.board.traces() {
        if inside.board.item(id).is_some() {
            inside.board.pull_tight_own_net(id, accuracy);
        }
    }
    // One end moves: on Net-(P4-P1) pull-tight straightens the track that
    // ends 15 µm outside pin 315's pad onto the pin's centre (Java's split
    // at a drill centre). It stays connected, now at the centre.
    let moved: Vec<&OffCentreEnd> = ends
        .iter()
        .filter(|end| !end_kept(&inside.board, end))
        .collect();
    assert_eq!(moved.len(), 1, "{moved:?}");
    for end in &ends {
        assert!(end_held(&inside.board, end), "{end:?} lost its pin");
    }
    // N04.4: the track whose inner corner the T junction is was split there
    // before it was pulled tight, so three tracks now end at the junction
    // (Java's fork) and the net stays complete.
    assert_eq!(traces_ending_at(&inside.board, T_JUNCTION), 3);
    assert_eq!(incompletes(&inside.board), 0);
    // Bump every straight axis-parallel track at an off-centre end 0.2 mm
    // sideways in its middle, so pull-tight has work to do on it.
    let mut kinked = Vec::new();
    for end in &ends {
        let board = &mut inside.board;
        let Some((id, other)) = board.traces().into_iter().find_map(|id| {
            let t = board.item(id)?.as_trace()?;
            let (first, last) = (int_xy(&t.first_corner()), int_xy(&t.last_corner()));
            (t.corner_count() == 2 && (first == end.at || last == end.at))
                .then(|| (id, if first == end.at { last } else { first }))
        }) else {
            continue;
        };
        let item = board.item(id).expect("trace").clone();
        let trace = item.as_trace().expect("trace");
        let (dx, dy) = (other.0 - end.at.0, other.1 - end.at.1);
        // A 45-degree bump 0.2 mm high in the middle of an axis-parallel
        // track (the board routes at 45 degrees).
        let length = dx.abs().max(dy.abs());
        if (dx != 0 && dy != 0) || length < 20000 {
            continue;
        }
        let (ux, uy) = (dx.signum(), dy.signum());
        let (px, py) = (-uy, ux);
        let (a, h) = (length / 4, 2000);
        let at = |along: i32, side: i32| {
            (
                end.at.0 + ux * along + px * side,
                end.at.1 + uy * along + py * side,
            )
        };
        let points = [
            end.at,
            at(a, 0),
            at(a + h, h),
            at(length - a - h, h),
            at(length - a, 0),
            other,
        ]
        .map(|(x, y)| Point::Int(IntPoint::new(x, y)));
        assert!(board.remove_item(id));
        let new_id = board
            .insert_trace_without_cleaning(
                Polyline::from_points(&points),
                trace.layer,
                trace.half_width,
                item.net_numbers(),
                item.clearance_class(),
                FixedState::Unfixed,
            )
            .expect("the kinked track is inserted");
        kinked.push((new_id, end.clone()));
    }
    // The fourth is a piece of the track split at the T junction.
    assert_eq!(kinked.len(), 4, "{} kinked", kinked.len());
    let mut changed = 0;
    for (id, end) in &kinked {
        assert!(
            end_kept(&inside.board, end),
            "{end:?}: kinked track lost the pin"
        );
        if inside.board.pull_tight_own_net(*id, accuracy) {
            changed += 1;
        }
    }
    assert_eq!(
        changed,
        kinked.len(),
        "pull-tight changed {changed} of {}",
        kinked.len()
    );
    for end in &ends {
        assert!(end_held(&inside.board, end), "{end:?} lost its pin");
    }
    for (_, end) in &kinked {
        assert!(end_kept(&inside.board, end), "{end:?} moved by pull-tight");
    }
    assert_eq!(traces_ending_at(&inside.board, T_JUNCTION), 3);
    assert_eq!(incompletes(&inside.board), 0);
}

#[test]
fn off_centre_tracks_are_not_trace_tails() {
    // The DSN's tracks load user-fixed, which no tail removal deletes.
    let unfixed = |extra| {
        let mut prepared = ecc83(extra);
        for id in prepared.board.traces() {
            prepared.board.unfix(id);
        }
        prepared.board
    };
    let mut inside = unfixed(INSIDE);
    // Every net is complete (N04.4's T junction included), so no off-centre
    // end is on a real tail.
    assert_eq!(incompletes(&inside), 0);
    let ends = off_centre_ends(&inside);
    assert_eq!(ends.len(), 16);
    inside.remove_trace_tails(-1, StopConnectionOption::None);
    for end in &ends {
        assert!(end_kept(&inside, end), "{end:?} removed as a tail");
    }
    // The track ending on the other track's corner is no tail either.
    assert_eq!(traces_ending_at(&inside, T_JUNCTION), 1);
    assert_eq!(incompletes(&inside), 0);
    // Under Java's rule the same tracks are tails, and go.
    let mut legacy = unfixed(LEGACY);
    let ends_at = |board: &BasicBoard| {
        ends.iter()
            .filter(|end| {
                board.traces().into_iter().any(|id| {
                    let t = board.item(id).and_then(|i| i.as_trace()).expect("trace");
                    int_xy(&t.first_corner()) == end.at || int_xy(&t.last_corner()) == end.at
                })
            })
            .count()
    };
    assert_eq!(ends_at(&legacy), ends.len());
    assert!(legacy.remove_trace_tails(-1, StopConnectionOption::None));
    assert!(
        ends_at(&legacy) < ends.len(),
        "Java's rule removed none of them"
    );
}

#[test]
fn the_ses_writes_off_centre_ends_unchanged() {
    let inside = ecc83(INSIDE);
    let ends = off_centre_ends(&inside.board);
    // Java's endpoint snap (C05) would move these ends: they lie within the
    // pad's inradius of its centre.
    let snappable = ends
        .iter()
        .filter(|end| {
            let drill = inside.board.item(end.pin).and_then(|i| i.drill()).unwrap();
            let centre = drill.center.to_float();
            let at =
                freerouting::geometry::FloatPoint::new(f64::from(end.at.0), f64::from(end.at.1));
            drill
                .shapes
                .iter()
                .flatten()
                .any(|pad| at.distance(centre) <= pad.border_distance(centre))
        })
        .count();
    assert!(snappable > 0);
    let ses = write_ses(&inside.board, "Issue690-ecc83.dsn", SesStyle::Java);
    for end in &ends {
        let text = format!("{} {}", end.at.0, end.at.1);
        assert!(ses.contains(&text), "{end:?} not written as is");
    }
    // Read back onto the bare board: the same ends touch the same pins.
    let mut back = ecc83(INSIDE).board;
    back.delete_all_traces_and_vias();
    read_session(&mut back, &ses).expect("session reads");
    assert_eq!(off_centre_ends(&back), ends);
    assert_eq!(incompletes(&back), 0);
}

// ---------------------------------------------------------------------
// N04.4: a trace end on a same-net trace (a T junction)
// ---------------------------------------------------------------------

/// The two traces of ecc83's T junction: the one ending at [`T_JUNCTION`]
/// and the one with an inner corner there.
fn t_junction_traces(board: &BasicBoard) -> (ItemId, ItemId) {
    let at = |id: ItemId| {
        let t = board.item(id).and_then(|i| i.as_trace()).expect("trace");
        let corners: Vec<(i32, i32)> = t.polyline.corners().iter().map(int_xy).collect();
        let n = corners.len();
        (
            corners[0] == T_JUNCTION || corners[n - 1] == T_JUNCTION,
            corners[1..n - 1].contains(&T_JUNCTION),
        )
    };
    let ending: Vec<ItemId> = board.traces().into_iter().filter(|id| at(*id).0).collect();
    let cornered: Vec<ItemId> = board.traces().into_iter().filter(|id| at(*id).1).collect();
    assert_eq!(
        (ending.len(), cornered.len()),
        (1, 1),
        "{ending:?} {cornered:?}"
    );
    (ending[0], cornered[0])
}

#[test]
fn a_track_ending_on_a_track_corner_connects_both_ways() {
    let inside = ecc83(INSIDE);
    let board = &inside.board;
    let (ending, cornered) = t_junction_traces(board);
    let t = board
        .item(ending)
        .and_then(|i| i.as_trace())
        .expect("trace");
    let end_contacts = if int_xy(&t.first_corner()) == T_JUNCTION {
        board.trace_start_contacts(ending)
    } else {
        board.trace_end_contacts(ending)
    };
    assert!(end_contacts.contains(&cornered), "{end_contacts:?}");
    // The other trace sees the junction as a contact, not at its ends.
    assert!(board.normal_contacts(cornered).contains(&ending));
    assert!(!board.trace_start_contacts(cornered).contains(&ending));
    assert!(!board.trace_end_contacts(cornered).contains(&ending));
    let (a, b) = (board.item(ending).unwrap(), board.item(cornered).unwrap());
    let point = board.normal_contact_point(a, b).expect("a contact point");
    assert_eq!(int_xy(&point), T_JUNCTION);
    assert_eq!(board.normal_contact_point(b, a), Some(point));
    // A walk along either connection stops at the junction, as at Java's
    // fork of three traces.
    let stop = StopConnectionOption::None;
    assert!(!board.connection_items(ending, stop).contains(&cornered));
    assert!(!board.connection_items(cornered, stop).contains(&ending));

    // Java's rule: no contact either way.
    let legacy = ecc83(LEGACY);
    let board = &legacy.board;
    assert_eq!(t_junction_traces(board), (ending, cornered));
    assert!(!board.normal_contacts(ending).contains(&cornered));
    assert!(!board.normal_contacts(cornered).contains(&ending));
    let (a, b) = (board.item(ending).unwrap(), board.item(cornered).unwrap());
    assert_eq!(board.normal_contact_point(a, b), None);
}

/// Every contact of the board is seen from both sides, at the same point,
/// so the incompletes and the ledger cannot go one-sided.
#[test]
fn every_contact_is_symmetric_under_inside_pad() {
    let inside = ecc83(INSIDE);
    let board = &inside.board;
    let ids: Vec<ItemId> = board.items().map(|i| i.id()).collect();
    let mut pairs = 0;
    for &id in &ids {
        for other in board.normal_contacts(id) {
            assert!(
                board.normal_contacts(other).contains(&id),
                "{id:?} sees {other:?}, not back"
            );
            let (a, b) = (board.item(id).unwrap(), board.item(other).unwrap());
            assert_eq!(
                board.normal_contact_point(a, b),
                board.normal_contact_point(b, a),
                "{id:?} {other:?}"
            );
            pairs += 1;
        }
    }
    assert!(pairs > 0);
}

/// Inserts an unfixed vertical trace of `like`'s net, layer and width on
/// ecc83, from `(x, y)` up 2 mm.
fn insert_vertical(board: &mut BasicBoard, like: ItemId, x: i32, y: i32) -> ItemId {
    let item = board.item(like).expect("trace").clone();
    let trace = item.as_trace().expect("trace");
    let points = [(x, y), (x, y + 20000)].map(|(x, y)| Point::Int(IntPoint::new(x, y)));
    board
        .insert_trace_without_cleaning(
            Polyline::from_points(&points),
            trace.layer,
            trace.half_width,
            item.net_numbers(),
            item.clearance_class(),
            FixedState::Unfixed,
        )
        .expect("the trace is inserted")
}

/// The end cap test is the board's exact shape intersection: an end whose
/// cap just reaches the other track's copper connects, one unit further it
/// does not. Pull-tight leaves a track alone that a trace ends on off its
/// centre line, and splits it at a junction on the centre line.
#[test]
fn the_t_junction_end_cap_test_has_no_tolerance() {
    let mut inside = ecc83(INSIDE);
    let (_, cornered) = t_junction_traces(&inside.board);
    let t = inside
        .board
        .item(cornered)
        .and_then(|i| i.as_trace())
        .unwrap();
    // The corner's horizontal segment, to (1435000, -1155700).
    assert_eq!(int_xy(&t.polyline.corner(2)), (1435000, T_JUNCTION.1));
    let reach = 2 * t.half_width;
    let x = 1400000;
    let touching = insert_vertical(&mut inside.board, cornered, x, T_JUNCTION.1 + reach);
    assert!(inside.board.normal_contacts(cornered).contains(&touching));
    assert!(
        inside
            .board
            .trace_start_contacts(touching)
            .contains(&cornered)
    );
    let apart = insert_vertical(
        &mut inside.board,
        cornered,
        x - 20000,
        T_JUNCTION.1 + reach + 1,
    );
    assert!(!inside.board.normal_contacts(cornered).contains(&apart));
    assert!(inside.board.trace_start_contacts(apart).is_empty());

    // Off the centre line no split joins the ends: pull-tight keeps it.
    inside.board.unfix(cornered);
    let accuracy = inside.settings.trace_pull_tight_accuracy;
    let corners = |board: &BasicBoard| {
        let t = board
            .item(cornered)
            .and_then(|i| i.as_trace())
            .expect("kept");
        t.polyline.corners().iter().map(int_xy).collect::<Vec<_>>()
    };
    let before = corners(&inside.board);
    assert!(!inside.board.pull_tight_own_net(cornered, accuracy));
    assert_eq!(corners(&inside.board), before);
    // With only the junction on its corner left, it is split there.
    assert!(inside.board.remove_item(touching));
    assert!(inside.board.pull_tight_own_net(cornered, accuracy));
    assert!(inside.board.item(cornered).is_none());
    assert_eq!(traces_ending_at(&inside.board, T_JUNCTION), 3);
}
