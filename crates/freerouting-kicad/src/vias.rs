//! Vias (K06.6).
//!
//! `(via [blind|buried|micro] [locked] (at) (size) (drill) (layers a b)
//! (net ...))` in file order, net 0 skipped. The padstack is
//! `SPECCTRA_DB::makeVia` (`specctra_export.cpp` at tag `10.0.0`): one circle
//! of the via diameter on each copper layer from `a` to `b` in physical order,
//! named `Via[top-bottom]_<d>:<drill>_um` with `%.6g`; new names are added to
//! the via library after the net classes' vias (`LookupVia`). A locked via is
//! `(type fix)`, any other `(type route)`.

use freerouting_dsn::ast::{NetId, Point, Via, WireType};

use crate::error::Warning;
use crate::layers::Layers;
use crate::nets::NetTable;
use crate::network::via_name;
use crate::sexpr::NodeRef;
use crate::tracks::{is_locked, layer_position};
use crate::units::{map_pt, parse_board_units};

/// One via of the board.
#[derive(Clone, Debug, PartialEq)]
pub struct BoardVia {
    pub at: Point,
    pub name: String,
    pub diameter: i32,
    pub drill: i32,
    pub top: usize,
    pub bottom: usize,
    pub net: String,
    pub locked: bool,
    /// `blind`, `buried`, `micro` or empty for a through via.
    pub kind: String,
}

impl BoardVia {
    pub fn to_dsn(&self) -> Via {
        Via {
            padstack: self.name.clone(),
            at: self.at,
            net: Some(NetId {
                name: self.net.clone(),
                subnet: None,
            }),
            clearance_class: None,
            via_type: Some(if self.locked {
                WireType::Fix
            } else {
                WireType::Other("route".to_owned())
            }),
        }
    }
}

/// The vias of the board with a net, in file order.
pub fn read_vias(
    board: NodeRef<'_>,
    layers: &Layers,
    nets: &NetTable,
    warnings: &mut Vec<Warning>,
) -> Vec<BoardVia> {
    let mut out = Vec::new();
    for node in board.children("via") {
        let net = nets.net_of(node).unwrap_or(0);
        if net == 0 {
            continue;
        }
        let at = node.child("at").and_then(|a| {
            Some((
                parse_board_units(a.atom(1)?)?,
                parse_board_units(a.atom(2)?)?,
            ))
        });
        let diameter = node
            .child("size")
            .and_then(|s| s.atom(1))
            .and_then(parse_board_units);
        let (Some(at), Some(diameter)) = (at, diameter) else {
            warnings.push(Warning {
                line: node.line(),
                message: "via without position or size; ignored".to_owned(),
            });
            continue;
        };
        let drill = node
            .child("drill")
            .and_then(|s| s.atom(1))
            .and_then(parse_board_units)
            .unwrap_or(0);
        let span = node.child("layers").and_then(|l| {
            let a = layer_position(layers, &l.str_value(1)?)?;
            let b = layer_position(layers, &l.str_value(2)?)?;
            Some((a.min(b), a.max(b)))
        });
        let Some((top, bottom)) = span else {
            warnings.push(Warning {
                line: node.line(),
                message: "via whose layers are not copper layers of the board; ignored".to_owned(),
            });
            continue;
        };
        let kind = ["blind", "buried", "micro"]
            .iter()
            .find(|k| {
                node.items()
                    .skip(1)
                    .any(|i| !i.is_list() && i.text() == **k)
            })
            .map(|k| (*k).to_owned())
            .unwrap_or_default();
        out.push(BoardVia {
            at: map_pt(i64::from(at.0), i64::from(at.1)),
            name: via_name(top, bottom, diameter, drill),
            diameter,
            drill,
            top,
            bottom,
            net: nets.name(net).unwrap_or("").replace('"', "''"),
            locked: is_locked(node),
            kind,
        });
    }
    out
}
