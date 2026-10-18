//! N04: `router.pad_contacts = inside_pad`, a trace end whose round cap
//! overlaps a same-net pin's pad connects to the pin (KiCad's rule;
//! `board::contact_rule`).
//!
//! The fixture is KiCad's ecc83-pp demo exported with its tracks
//! (`fixtures/Issue690-ecc83.dsn`; its wires are byte-identical to the
//! export in the N04 notes). Its tracks end off the pad centre on 16 pins,
//! some just outside the pad, so Java's centre rule leaves 10 connections
//! open on a board KiCad calls fully connected; `inside_pad` leaves 1, a T
//! junction (N04.4).
//!
//! - N04.1: the incompletes, the routing ledger and the DRC agree under each
//!   rule; the contacts are symmetric; unset and `legacy` keep Java's rule;
//!   a snapshot keeps the rule; an unknown value fails the job.
//! - N04.2: pull-tight keeps every off-centre end on its pad (one end moves
//!   onto the pin's centre), the router and optimizer keep them, trace tails
//!   keep the traces, and the SES writes the ends unchanged and reads back to
//!   the same contacts.

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
const DANGLING_INSIDE: usize = 1;

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
    // The one left is not a pad contact: on Net-(U1A-K) a track ends on an
    // inner corner of another track (a T junction, N04.4). KiCad connects it.
    assert_eq!(incompletes(&inside.board), 1);
    assert_eq!(dangling(&inside.board), DANGLING_INSIDE);
    let open: BTreeSet<String> = inside
        .board
        .incompletes()
        .iter()
        .filter(|n| n.count() > 0)
        .map(|n| {
            inside
                .board
                .rules
                .nets
                .get(n.net_number)
                .expect("net")
                .name
                .clone()
        })
        .collect();
    assert_eq!(open, BTreeSet::from(["Net-(U1A-K)".to_string()]));
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
    assert_eq!(incompletes(&copy), 1);
    assert_eq!(off_centre_ends(&copy), off_centre_ends(&inside.board));
}

#[test]
fn routing_ecc83_under_inside_pad_keeps_its_tracks_connected() {
    let legacy = route(ecc83(LEGACY));
    assert_eq!(incompletes(&legacy), 6, "Java's result (route.log)");
    let prepared = ecc83(INSIDE);
    let ends = off_centre_ends(&prepared.board);
    let routed = route(prepared);
    assert_eq!(incompletes(&routed), 1);
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
    assert_eq!(kinked.len(), 3, "{} kinked", kinked.len());
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
    assert_eq!(incompletes(&inside.board), 1);
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
    // The ends on the three nets left open are on connections whose other
    // end is open (a real tail); the rest are on complete nets.
    let open: BTreeSet<i32> = inside
        .incompletes()
        .iter()
        .filter(|n| n.count() > 0)
        .map(|n| n.net_number)
        .collect();
    let ends: BTreeSet<OffCentreEnd> = off_centre_ends(&inside)
        .into_iter()
        .filter(|end| {
            !inside
                .item(end.pin)
                .expect("pin")
                .net_numbers()
                .iter()
                .any(|n| open.contains(n))
        })
        .collect();
    assert_eq!(ends.len(), 13);
    inside.remove_trace_tails(-1, StopConnectionOption::None);
    for end in &ends {
        assert!(end_kept(&inside, end), "{end:?} removed as a tail");
    }
    assert_eq!(incompletes(&inside), 1);
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
    assert_eq!(incompletes(&back), 1);
}
