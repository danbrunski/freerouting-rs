//! The routed copper `tidy` reads: a session file or a `.kicad_pcb` (Q15.1).
//!
//! Both end as a Specctra [`Session`] imported into the board by
//! [`import_at_writer_scale`], so the copper is scored by one code path whichever file
//! it came from. The routed copper replaces the board's own: a session
//! repeats the board's traces and vias but the system-fixed ones
//! ([`import_routed_session`]), and a `.kicad_pcb` holds all of them, so
//! the import first removes those (for a `.kicad_pcb`, every trace and via).
//! Importing on top of them counted the input's wiring twice.
//!
//! A `.kicad_pcb` is read by the K-series reader
//! ([`freerouting::io::kicad`]): its segments, arcs (as polylines) and vias
//! become the wires and vias of the session. The reader's frame is KiCad's own
//! DSN frame (micrometres, y negated, no offset), so **the `.kicad_pcb` must be
//! the board the DSN was exported from** (KiCad's exporter or this tool's
//! reader), or the copper lands beside the pins. Net names are matched as the
//! importer matches them. A via whose padstack the DSN does not have (KiCad
//! names them `Via[0-1]_800:400_um`; a DSN written by another tool may not)
//! takes the board's first via type; the count is in [`Imported::via_substitutions`].
//! A wire the reader joined from several segments is cut again where another
//! wire touches one of its corners, so a branch meets a trace end, as the
//! importer and the router's connectivity need. Only copper is read: zones
//! and keepouts of the file are ignored.

use std::collections::BTreeMap;
use std::path::Path;

use freerouting::board::BasicBoard;
use freerouting::board::structure::Unit;
use freerouting::dsn::Session;
use freerouting::dsn::ast::{LayerRef, Point, Routes, SessionNet, Shape, Via, Wire, WiringItem};
use freerouting::io::kicad::{DesignKind, ReadOptions, design_kind, read_kicad_input};
use freerouting::io::specctra::SesImportSummary;

use crate::ses_diff::{import_at_writer_scale, import_routed_session, parse_session};
use crate::{load_board, read_text};

/// What importing the routed copper did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Imported {
    pub summary: SesImportSummary,
    /// Vias whose padstack the board lacked and that took the first via type.
    pub via_substitutions: usize,
}

/// Board units per session unit: what the writer divides by and the
/// writer-scale reader ([`import_at_writer_scale`]) multiplies by.
fn session_denominator(board: &BasicBoard) -> f64 {
    let c = &board.communication;
    c.coordinate_transform.dsn_to_board(1.0) / f64::from(c.resolution)
}

/// The first via type's padstack name, if the board has one.
fn first_via_padstack(board: &BasicBoard) -> Option<String> {
    let (_, info) = board.rules.via_infos.iter().next()?;
    board
        .library
        .padstacks
        .get(info.padstack)
        .map(|p| p.name.clone())
}

type VertexKey = (String, u64, u64);

fn vertex_key(layer: &str, p: &Point) -> VertexKey {
    (layer.to_string(), p.x.to_bits(), p.y.to_bits())
}

fn layer_text(layer: &LayerRef) -> String {
    match layer {
        LayerRef::Pcb => "pcb".into(),
        LayerRef::Signal => "signal".into(),
        LayerRef::Named(name) => name.clone(),
    }
}

/// How often each vertex (layer and position) appears in the paths of the
/// board's wiring, ends and corners alike.
fn vertex_counts(wiring: &[WiringItem]) -> BTreeMap<VertexKey, usize> {
    let mut counts = BTreeMap::new();
    for item in wiring {
        let WiringItem::Wire(wire) = item else {
            continue;
        };
        for shape in &wire.shapes {
            if let Shape::Path { layer, points, .. } = shape {
                for p in points {
                    *counts.entry(vertex_key(&layer_text(layer), p)).or_default() += 1;
                }
            }
        }
    }
    counts
}

/// Cuts a path at every interior vertex that another path also touches. The
/// reader joins consecutive segments into one wire; a branch that starts at
/// one of its corners would not touch a trace end, so it would not connect.
fn split_path(
    layer: &LayerRef,
    points: &[Point],
    counts: &BTreeMap<VertexKey, usize>,
) -> Vec<Vec<Point>> {
    let name = layer_text(layer);
    let mut parts = Vec::new();
    let mut current = Vec::new();
    for (i, p) in points.iter().enumerate() {
        current.push(*p);
        let interior = i > 0 && i + 1 < points.len();
        if interior && counts.get(&vertex_key(&name, p)).copied().unwrap_or(0) > 1 {
            parts.push(std::mem::replace(&mut current, vec![*p]));
        }
    }
    parts.push(current);
    parts
}

/// The session of the copper in the `.kicad_pcb` `path`, in the units of
/// `board`, and how many vias took the board's first via type.
pub fn kicad_session(board: &BasicBoard, path: &Path) -> Result<(Session, usize), String> {
    let input = read_kicad_input(path, ReadOptions::default())
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let denominator = session_denominator(board);
    let c = &board.communication;
    // Reader coordinates are micrometres; the session's are board units divided
    // by the denominator, as the writer writes them.
    let convert = |v: f64| {
        c.coordinate_transform
            .dsn_to_board(Unit::scale(v, Unit::Um, c.unit))
            / denominator
    };
    let point = |p: &Point| Point::new(convert(p.x), convert(p.y));
    let fallback = first_via_padstack(board);
    let wiring: &[WiringItem] = input.pcb.wiring.as_ref().map_or(&[], |w| &w.items);
    let counts = vertex_counts(wiring);
    let mut substitutions = 0;
    let mut nets: Vec<SessionNet> = Vec::new();
    for item in wiring {
        let (net_name, converted) = match item {
            WiringItem::Wire(wire) => {
                let Some(name) = wire.net.as_ref().map(|n| n.name.clone()) else {
                    continue;
                };
                let mut converted = Vec::new();
                for shape in &wire.shapes {
                    let Shape::Path {
                        layer,
                        width,
                        points,
                    } = shape
                    else {
                        continue;
                    };
                    for part in split_path(layer, points, &counts) {
                        converted.push(WiringItem::Wire(Wire {
                            shapes: vec![Shape::Path {
                                layer: layer.clone(),
                                width: convert(*width),
                                points: part.iter().map(point).collect(),
                            }],
                            windows: Vec::new(),
                            net: None,
                            clearance_class: None,
                            wire_type: None,
                            attr: None,
                        }));
                    }
                }
                (name, converted)
            }
            WiringItem::Via(via) => {
                let Some(name) = via.net.as_ref().map(|n| n.name.clone()) else {
                    continue;
                };
                let mut padstack = via.padstack.clone();
                if board.library.padstacks.get_by_name(&padstack).is_none()
                    && let Some(first) = &fallback
                {
                    padstack = first.clone();
                    substitutions += 1;
                }
                (
                    name,
                    vec![WiringItem::Via(Via {
                        padstack,
                        at: point(&via.at),
                        net: None,
                        clearance_class: None,
                        via_type: None,
                    })],
                )
            }
        };
        match nets.iter_mut().find(|n| n.name == net_name) {
            Some(net) => net.items.extend(converted),
            None => nets.push(SessionNet {
                name: net_name,
                items: converted,
            }),
        }
    }
    let session = Session {
        name: "kicad_pcb".into(),
        routes: Some(Routes {
            network_out: nets,
            ..Routes::default()
        }),
        ..Session::default()
    };
    Ok((session, substitutions))
}

/// Imports the routed copper in `path` (`.ses` or `.kicad_pcb`) into
/// `board`, the input it was routed from, in place of the board's wiring
/// that the file holds too.
pub fn import_routed(board: &mut BasicBoard, path: &Path) -> Result<Imported, String> {
    match design_kind(path) {
        DesignKind::KicadPcb => {
            let (session, via_substitutions) = kicad_session(board, path)?;
            board.delete_all_traces_and_vias();
            Ok(Imported {
                summary: import_at_writer_scale(board, &session),
                via_substitutions,
            })
        }
        DesignKind::Dsn => {
            let session =
                parse_session(&read_text(path)?).map_err(|e| format!("{}: {e}", path.display()))?;
            Ok(Imported {
                summary: import_routed_session(board, &session),
                via_substitutions: 0,
            })
        }
    }
}

/// `dsn` loaded with the copper of `routed` imported.
pub fn routed_board(
    dsn: &Path,
    routed: &Path,
) -> Result<(freerouting::board::RoutingBoard, Imported), String> {
    let mut board = load_board(dsn)?;
    let imported = import_routed(&mut board, routed)?;
    Ok((board, imported))
}
