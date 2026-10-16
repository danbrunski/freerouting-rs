//! Net dialect detection and the net table.
//!
//! KiCad 9 and earlier write a table at the top of the board, `(net 1
//! "GND")`, and refer to nets by number: `(segment ... (net 1))`, pads as
//! `(net 1 "GND")`. KiCad 10 stopped writing net codes (`20251028`, "Stop
//! writing netcodes; they're an internal implementation detail") and refers
//! to nets by name only: `(net "GND")`, no table.
//!
//! The dialect comes from the content, not the version stamp (KiCadRoutingTools
//! `board_uses_name_nets`, its issues #1028 and #163): the KiCad 10 demo
//! `CM5_MINIMA_3.kicad_pcb` is `(version 20250513)`, newer than KiCadRoutingTools'
//! `KICAD_10_MIN_VERSION` of `20250000`, and still has a numeric table.
//!
//! Detection order:
//!
//! 1. any `(net "name")` reference (two items, the second a string) means
//!    [`Dialect::Names`];
//! 2. else a top-level `(net N "name")` table means [`Dialect::Ids`];
//! 3. else the version stamp decides: [`NAME_NETS_FROM_VERSION`] and later
//!    means names. (KiCadRoutingTools uses `20250000`; the KiCad 10.0.0
//!    source says `20251028`. The choice only matters for a board with no
//!    nets at all.)
//!
//! Lookups use the raw escaped token text, never the unescaped string
//! (KiCadRoutingTools #369 A12: unescaping first put a zone on the wrong net).
//! KiCad's writer escapes names canonically, so the same net always has the
//! same raw text.
//!
//! In the name dialect, codes are assigned in order of first appearance in the
//! file, starting at 1; code 0 is KiCad's unconnected net with the empty
//! name. This is what KiCad's parser does while loading (`FindNet` then `new
//! NETINFO_ITEM`, `parseNETINFO_ITEM` and the pad/zone/track readers in
//! `pcb_io_kicad_sexpr_parser.cpp`); K05.2 checks it against KiCad's export.
//! No `HashMap` is involved.

use std::collections::BTreeMap;

use crate::error::Warning;
use crate::sexpr::{NodeKind, NodeRef, Tree};

/// First file version that writes net references by name only
/// (`SEXPR_BOARD_FILE_VERSION 20251028` in `pcb_io_kicad_sexpr.h`).
pub const NAME_NETS_FROM_VERSION: u32 = 20251028;

/// How a board refers to its nets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dialect {
    /// A numeric table and numeric references (KiCad 6 to 9).
    Ids,
    /// Names only (KiCad 10).
    Names,
}

/// The nets of a board.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NetTable {
    pub dialect: Dialect,
    /// Net name (unescaped) by code. `None` marks a gap in a numeric table.
    pub by_code: Vec<Option<String>>,
    /// Code by raw (escaped) token text of the name, sorted.
    pub codes_by_name: BTreeMap<String, u32>,
    /// Problems found while building the table.
    pub warnings: Vec<Warning>,
}

/// Decides the dialect of `tree` (a parsed board) with the rules above.
pub fn detect_dialect(tree: &Tree<'_>, version: u32) -> Dialect {
    let root = tree.root();
    let board = root.child("kicad_pcb");
    let has_table = board.is_some_and(|b| b.children("net").any(is_table_entry));
    // Cheap exit: a numeric table board refers to nets by number, but only a
    // scan can prove there is no name reference.
    let has_name_ref = (0..tree.len() as u32).any(|i| {
        let n = tree.node(i);
        n.is_list() && n.head() == Some("net") && is_name_ref(n)
    });
    if has_name_ref {
        Dialect::Names
    } else if has_table {
        Dialect::Ids
    } else if version >= NAME_NETS_FROM_VERSION {
        Dialect::Names
    } else {
        Dialect::Ids
    }
}

/// `(net "name")`: two items, the second a quoted string.
fn is_name_ref(net: NodeRef<'_>) -> bool {
    net.len() == 2 && net.item(1).is_some_and(|i| i.kind() == NodeKind::Str)
}

/// `(net N "name")`: three items, a number and a name.
fn is_table_entry(net: NodeRef<'_>) -> bool {
    net.len() == 3
        && net.atom(1).is_some_and(|a| a.parse::<u32>().is_ok())
        && net.item(2).is_some_and(|i| i.kind() != NodeKind::List)
}

impl NetTable {
    /// Detects the dialect of the board in `tree` and builds its table.
    pub fn build(tree: &Tree<'_>, version: u32) -> NetTable {
        match detect_dialect(tree, version) {
            Dialect::Ids => Self::build_ids(tree),
            Dialect::Names => Self::build_names(tree),
        }
    }

    /// The numeric dialect: the top-level `(net N "name")` entries.
    pub fn build_ids(tree: &Tree<'_>) -> NetTable {
        let mut t = NetTable {
            dialect: Dialect::Ids,
            by_code: Vec::new(),
            codes_by_name: BTreeMap::new(),
            warnings: Vec::new(),
        };
        let Some(board) = tree.root().child("kicad_pcb") else {
            return t;
        };
        for net in board.children("net").filter(|n| is_table_entry(*n)) {
            let code: u32 = net.atom(1).and_then(|a| a.parse().ok()).unwrap_or(0);
            let raw = net.raw(2).unwrap_or("");
            let name = net.str_value(2).map(|c| c.into_owned()).unwrap_or_default();
            if t.by_code.get(code as usize).is_some_and(|e| e.is_some()) {
                t.warnings.push(Warning {
                    line: net.line(),
                    message: format!(
                        "net code {code} is defined twice; the first definition is kept"
                    ),
                });
                continue;
            }
            if t.by_code.len() <= code as usize {
                t.by_code.resize(code as usize + 1, None);
            }
            t.by_code[code as usize] = Some(name);
            if t.codes_by_name.contains_key(raw) {
                t.warnings.push(Warning {
                    line: net.line(),
                    message: format!(
                        "net name \"{raw}\" is used by two net codes; the first is kept"
                    ),
                });
            } else {
                t.codes_by_name.insert(raw.to_owned(), code);
            }
        }
        t
    }

    /// The name dialect: every `(net "name")` reference, in file order.
    pub fn build_names(tree: &Tree<'_>) -> NetTable {
        let mut t = NetTable {
            dialect: Dialect::Names,
            by_code: vec![Some(String::new())],
            codes_by_name: BTreeMap::new(),
            warnings: Vec::new(),
        };
        t.codes_by_name.insert(String::new(), 0);
        for i in 0..tree.len() as u32 {
            let n = tree.node(i);
            if !(n.is_list() && n.head() == Some("net") && is_name_ref(n)) {
                continue;
            }
            let raw = n.raw(1).unwrap_or("");
            if !t.codes_by_name.contains_key(raw) {
                let code = t.by_code.len() as u32;
                t.by_code.push(Some(
                    n.str_value(1).map(|c| c.into_owned()).unwrap_or_default(),
                ));
                t.codes_by_name.insert(raw.to_owned(), code);
            }
        }
        t
    }

    /// Number of defined nets, gaps not counted.
    pub fn len(&self) -> usize {
        self.by_code.iter().filter(|n| n.is_some()).count()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Name (unescaped) of net `code`.
    pub fn name(&self, code: u32) -> Option<&str> {
        self.by_code.get(code as usize)?.as_deref()
    }

    /// Code of the net whose name has this raw (escaped) token text.
    pub fn code_of_raw(&self, raw: &str) -> Option<u32> {
        self.codes_by_name.get(raw).copied()
    }

    /// The net of a board item (segment, via, pad, zone): the code its `(net
    /// ...)` child refers to, `None` when the item has no net or the net is
    /// unknown.
    pub fn net_of(&self, item: NodeRef<'_>) -> Option<u32> {
        let net = item.child("net")?;
        match self.dialect {
            Dialect::Ids => {
                let code: u32 = net.atom(1)?.parse().ok()?;
                self.name(code).map(|_| code)
            }
            Dialect::Names => self.code_of_raw(net.raw(1)?),
        }
    }
}
