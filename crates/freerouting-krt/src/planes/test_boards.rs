//! Synthetic two-layer boards for the unit tests: a 40 x 30 mm board, a GND
//! pour on B.Cu from (10, 5) to (30, 25) mm, through-hole pads, and an
//! optional pre-routed trace of net S on B.Cu (with an S pad at each end)
//! that cuts the pour.

use freerouting::board::RoutingBoard;
use freerouting::drc::load_design;

/// A through-hole pad of one net.
pub struct Pad {
    pub name: &'static str,
    pub x: i32,
    pub y: i32,
    pub net: &'static str,
}

impl Pad {
    /// A GND pad at (`x`, `y`) µm.
    pub fn gnd(name: &'static str, x: i32, y: i32) -> Pad {
        Pad {
            name,
            x,
            y,
            net: "GND",
        }
    }
}

/// The board with `pads` and, if given, the S trace `cut` (x1, y1, x2, y2).
pub fn board(pads: &[Pad], cut: Option<(i32, i32, i32, i32)>) -> RoutingBoard {
    board_with_wires(pads, cut, &[])
}

/// A wire: (net, layer, (x1, y1, x2, y2)).
pub type Wire<'a> = (&'a str, &'a str, (i32, i32, i32, i32));

/// [`board`] plus `wires`, 250 µm wide.
pub fn board_with_wires(
    pads: &[Pad],
    cut: Option<(i32, i32, i32, i32)>,
    wires: &[Wire<'_>],
) -> RoutingBoard {
    let mut all: Vec<(String, i32, i32, String)> = pads
        .iter()
        .map(|p| (p.name.to_string(), p.x, p.y, p.net.to_string()))
        .collect();
    let mut wiring = String::new();
    if let Some((x1, y1, x2, y2)) = cut {
        all.push(("S1".into(), x1, y1, "S".into()));
        all.push(("S2".into(), x2, y2, "S".into()));
        wiring.push_str(&format!(
            "    (wire (path B.Cu 500 {x1} {y1} {x2} {y2}) (net S) (type route))\n"
        ));
    }
    for (net, layer, (x1, y1, x2, y2)) in wires {
        wiring.push_str(&format!(
            "    (wire (path {layer} 250 {x1} {y1} {x2} {y2}) (net {net}) (type route))\n"
        ));
    }
    let places: String = all
        .iter()
        .map(|(n, x, y, _)| format!("      (place {n} {x} {y} front 0)\n"))
        .collect();
    let mut nets: Vec<String> = all.iter().map(|p| p.3.clone()).collect();
    nets.sort();
    nets.dedup();
    let network: String = nets
        .iter()
        .map(|net| {
            let pins: Vec<String> = all
                .iter()
                .filter(|p| &p.3 == net)
                .map(|p| format!("{}-1", p.0))
                .collect();
            format!("    (net {net} (pins {}))\n", pins.join(" "))
        })
        .collect();
    let dsn = format!(
        "(pcb test.dsn
  (parser (string_quote \") (space_in_quoted_tokens on) (host_cad \"KiCad's Pcbnew\") (host_version \"x02 unit test\"))
  (resolution um 10)
  (unit um)
  (structure
    (layer F.Cu (type signal) (property (index 0)))
    (layer B.Cu (type signal) (property (index 1)))
    (boundary (path pcb 0  0 0  40000 0  40000 30000  0 30000  0 0))
    (plane GND (polygon B.Cu 0  10000 5000  30000 5000  30000 25000  10000 25000))
    (via \"Via[0-1]_600:300_um\")
    (rule (width 250) (clearance 200))
  )
  (placement
    (component \"TH\"
{places}    )
  )
  (library
    (image \"TH\" (pin Th 1 0 0))
    (padstack Th (shape (circle F.Cu 1000)) (shape (circle B.Cu 1000)) (attach off))
    (padstack \"Via[0-1]_600:300_um\" (shape (circle F.Cu 600)) (shape (circle B.Cu 600)) (attach off))
  )
  (network
{network}    (class kicad_default \"\" {}
      (circuit (use_via \"Via[0-1]_600:300_um\"))
      (rule (width 250) (clearance 200))
    )
  )
  (wiring
{wiring}  )
)
",
        nets.join(" ")
    );
    load_design(dsn.as_bytes()).unwrap_or_else(|e| panic!("{e}\n{dsn}"))
}
