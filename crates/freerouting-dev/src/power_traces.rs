//! `freerouting-dev power-traces board.dsn [session.ses]`: the traces on
//! power layers that break Q01.7's rule (a net without a plane there) or
//! Q13's (a plane's net that leaves its plane), after importing a session
//! when one is given. Prints one line per trace and a count line; exits 1
//! when it finds any. Q13.3 counts `outside_own_plane` with it.

use std::process::ExitCode;

use freerouting::autoroute::power_layer_traces::{PowerLayerTrace, power_layer_breaches};
use freerouting::board::BasicBoard;

use crate::ses_diff::parse_session;
use crate::{load_board, read_text};

const USAGE: &str = "usage: freerouting-dev power-traces board.dsn [session.ses]";

/// The breach lines of `board`, `<kind> <net> on <layer>: (x1, y1) -> (x2, y2)` in
/// board units, in item order.
pub fn breach_lines(board: &BasicBoard) -> Vec<(PowerLayerTrace, String)> {
    power_layer_breaches(board)
        .into_iter()
        .filter_map(|(id, kind)| {
            let item = board.item(id)?;
            let trace = item.as_trace()?;
            let net = item.net_numbers().first().copied().unwrap_or(0);
            let net = board.rules.nets.get(net).map_or("?", |n| n.name.as_str());
            let layer = &board.layer_structure.layers[trace.layer as usize].name;
            let corners: Vec<String> = trace
                .polyline
                .corner_approx_arr()
                .iter()
                .map(|p| format!("({:.0}, {:.0})", p.x, p.y))
                .collect();
            let label = match kind {
                PowerLayerTrace::Foreign => "foreign",
                PowerLayerTrace::OutsideOwnPlane => "outside_own_plane",
            };
            Some((
                kind,
                format!("{label} {net} on {layer}: {}", corners.join(" -> ")),
            ))
        })
        .collect()
}

/// Runs the subcommand.
pub fn run(args: &[String]) -> Result<ExitCode, String> {
    let (dsn, ses) = match args {
        [dsn] => (dsn, None),
        [dsn, ses] => (dsn, Some(ses)),
        _ => return Err(USAGE.to_string()),
    };
    let mut board = load_board(std::path::Path::new(dsn))?;
    if let Some(ses) = ses {
        let session = parse_session(&read_text(std::path::Path::new(ses))?)?;
        freerouting::io::specctra::import_session(&mut board, &session);
    }
    let lines = breach_lines(&board);
    let count = |k: PowerLayerTrace| lines.iter().filter(|(kind, _)| *kind == k).count();
    for (_, line) in &lines {
        println!("{line}");
    }
    println!(
        "power-layer traces: foreign {}, outside_own_plane {}",
        count(PowerLayerTrace::Foreign),
        count(PowerLayerTrace::OutsideOwnPlane)
    );
    Ok(if lines.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    })
}
