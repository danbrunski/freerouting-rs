//! Q17.1: the width policy of the maze search.
//!
//! Routing with Freerouting's policy written out ([`FreeroutingWidths`],
//! the control's half width in every room) leaves the same board as
//! routing without a policy (the parity path, which reads the control
//! directly): every sampled connection of two fixtures is routed in turn
//! both ways, with ripup, and the sessions are compared byte for byte.
//! The A14 gate covers the parity path itself.

mod autoroute_common;

use std::collections::BTreeSet;

use autoroute_common::{load, route_sets, router_control, sample};
use freerouting::autoroute::connection::{Ripped, RouteLimits, route_connection_with_widths};
use freerouting::autoroute::path::RipupCosts;
use freerouting::autoroute::session::drills::DrillCarry;
use freerouting::autoroute::session::search::SearchSettings;
use freerouting::autoroute::session::sink::NoopSink;
use freerouting::autoroute::width_policy::{FreeroutingWidths, WidthPolicy};
use freerouting::io::specctra::ses_writer::{SesStyle, write_ses};

const BOARDS: [&str; 2] = [
    "fixtures/Issue143-rpi_splitter.dsn",
    "fixtures/Issue508-SMD-routing-issue-demo.dsn",
];

/// Routes the sampled connections of `dsn` in turn; with `explicit`, each
/// through Freerouting's policy. The session and the attempt states.
fn route_all(dsn: &str, explicit: bool) -> (String, Vec<String>) {
    let mut loaded = load(dsn);
    let conns = sample(&loaded.board);
    let settings = SearchSettings::from_router(&loaded.router);
    let mut states = Vec::new();
    for conn in conns {
        let conn = route_sets(&loaded.board, conn.net, conn.item);
        if conn.state.is_some() || conn.start.is_empty() || conn.dest.is_empty() {
            continue;
        }
        let Ok(control) = router_control(&loaded, conn.net, true) else {
            continue;
        };
        let policy = FreeroutingWidths { control: &control };
        let widths = explicit.then_some(&policy as &dyn WidthPolicy);
        let mut ripped = BTreeSet::new();
        let mut costs = RipupCosts::new();
        let (result, _) = route_connection_with_widths(
            &mut loaded.board,
            &conn.start,
            &conn.dest,
            &control,
            RouteLimits::unlimited(settings),
            Ripped {
                items: &mut ripped,
                costs: Some(&mut costs),
            },
            NoopSink,
            &mut DrillCarry::default(),
            widths,
        );
        states.push(format!("{:?} {:?}", result.state, ripped));
    }
    (write_ses(&loaded.board, dsn, SesStyle::Java), states)
}

#[test]
fn the_freerouting_policy_routes_like_no_policy() {
    for dsn in BOARDS {
        let (without, states_without) = route_all(dsn, false);
        let (with, states_with) = route_all(dsn, true);
        assert!(!states_without.is_empty(), "{dsn}: nothing routed");
        assert_eq!(states_without, states_with, "{dsn}");
        assert_eq!(without, with, "{dsn}");
    }
}
