//! Writing routed copper back into a copy of the `.kicad_pcb` (K08).
//!
//! The writer edits text and never re-serializes the board: the copper the
//! routed board no longer has is cut out by byte span, new copper is inserted
//! before the board's final `)`, and every other byte stays as it was, so
//! footprints, zones, user edits and settings survive. Zones are not touched;
//! their fills go stale after routing, and grading refills them
//! (`kicad-cli pcb drc --refill-zones`).
//!
//! - [`CopperIndex`] (K08.1): every `(segment)`, `(arc)` and `(via)` of the
//!   board with its geometry in nanometres, net, layer or span, width or size
//!   and drill, uuid, locked flag and byte span, plus what a new block needs
//!   (layer names, the net dialect, the uuid token). The reader builds it
//!   ([`crate::KicadRead::copper_index`]).
//! - [`diff`] (K08.1): what to keep, remove and add, by exact geometry in
//!   board units.
//! - [`splice`] and [`format`] (K08.2): the text edit and the new blocks, in
//!   KiCad 10's multi-line layout, in the file's own net dialect and uuid
//!   token.
//! - [`uuid`] (K08.3): deterministic version-8 UUIDs.
//! - [`render`]: all of it; the result is parsed again and must have balanced
//!   parentheses.
//!
//! The engine entry point is `freerouting::io::kicad::write_kicad_board`,
//! which reads the board again, refuses a file that changed since the board
//! was built and refuses to overwrite its input.
//!
//! # Credits
//!
//! The approach (splice text, keep the net dialect, give new vias no tenting
//! token, check paren balance before writing, and the finding that random
//! UUIDs make KiCad's zone fill unstable) comes from KiCadRoutingTools'
//! `py_router/kicad_writer.py` and `docs/api-kicad-writer.md` at `0486e799`
//! (`drandyhaas/KiCadRoutingTools`, MIT; issues #80, #369, #489, #523, #741).
//! No code was taken from it.

pub mod diff;
pub mod format;
pub mod splice;
pub mod uuid;

use std::collections::BTreeSet;

use crate::error::{ErrorKind, KicadError};
use crate::layers::Layers;
use crate::legacy::{Pt, item_uuid};
use crate::nets::{Dialect, NetTable};
use crate::sexpr::{Lexer, NodeKind, NodeRef, TokenKind, Tree};
use crate::tracks::{arc_points, is_locked, layer_position};
use crate::units::parse_board_units;

pub use diff::{
    BPt, BoardFrame, CopperDiff, FinalItem, FinalTrace, FinalVia, GridFrame, NewItem, diff,
};
pub use format::{BlockStyle, format_nm_mm};
pub use uuid::{UuidGen, fnv1a128};

/// What a copper block is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CopperKind {
    Segment,
    Arc {
        mid: Pt,
    },
    /// `kind` is `blind`, `buried`, `micro` or empty (through).
    Via {
        kind: String,
    },
}

/// One `(segment)`, `(arc)` or `(via)` of the file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CopperItem {
    pub kind: CopperKind,
    /// Start of a track, centre of a via, nanometres.
    pub start: Pt,
    /// End of a track; the centre again for a via.
    pub end: Pt,
    /// The track as the reader flattens it: start and end, or the arc's
    /// points; the centre for a via.
    pub points: Vec<Pt>,
    /// KiCad net code; `None` for no net or an unknown one.
    pub net: Option<u32>,
    /// Copper position of a track.
    pub layer: Option<usize>,
    /// Copper positions of a via, top to bottom.
    pub span: Option<(usize, usize)>,
    /// Track width or via diameter, nanometres.
    pub width: Option<i32>,
    pub drill: Option<i32>,
    pub uuid: Option<String>,
    pub locked: bool,
    /// Whether the reader put this item on the board (a net, copper layers,
    /// complete geometry). Other items are always kept.
    pub routable: bool,
    /// The block, from its `(` to just past its `)`.
    pub span_bytes: (usize, usize),
}

/// A copper layer: its canonical name (`In1.Cu`) and the name the DSN and the
/// board use (the user name when there is one).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexLayer {
    pub std_name: String,
    pub dsn_name: String,
}

/// A via padstack of the DSN tree with its KiCad sizes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ViaDef {
    pub name: String,
    pub diameter: i32,
    pub drill: i32,
    pub top: usize,
    pub bottom: usize,
}

/// The copper of a board file and what new copper blocks need.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CopperIndex {
    pub items: Vec<CopperItem>,
    pub layers: Vec<IndexLayer>,
    /// Net name (unescaped) by code.
    pub net_names: Vec<Option<String>>,
    /// The net name as the DSN tree and the board have it (`"` as `''`).
    pub dsn_net_names: Vec<Option<String>>,
    /// The raw (escaped) name token by code, for the name dialect.
    pub net_raw: Vec<Option<String>>,
    pub style: BlockStyle,
    /// Every `uuid` and `tstamp` value of the file.
    pub uuids: BTreeSet<String>,
    /// The via padstacks of the DSN tree, in its order.
    pub via_defs: Vec<ViaDef>,
}

impl CopperIndex {
    /// The KiCad net code of a board net name.
    pub fn net_code(&self, dsn_name: &str) -> Option<u32> {
        self.dsn_net_names
            .iter()
            .position(|n| n.as_deref() == Some(dsn_name))
            .map(|c| c as u32)
    }

    /// The copper position of a board layer name.
    pub fn layer_of(&self, dsn_name: &str) -> Option<usize> {
        self.layers
            .iter()
            .position(|l| l.dsn_name == dsn_name || l.std_name == dsn_name)
    }

    /// The via padstack of this name.
    pub fn via_def(&self, name: &str) -> Option<&ViaDef> {
        self.via_defs.iter().find(|v| v.name == name)
    }
}

/// The byte offset of `node` (a list) in `src` and the end of its block.
fn block_span(src: &str, node: NodeRef<'_>) -> Option<(usize, usize)> {
    let head = node.item(0)?;
    if head.kind() != NodeKind::Atom {
        return None;
    }
    let text = head.text();
    let base = src.as_ptr() as usize;
    let at = (text.as_ptr() as usize).checked_sub(base)?;
    if at >= src.len() {
        return None;
    }
    let bytes = src.as_bytes();
    let mut open = at;
    while open > 0 && bytes[open - 1] <= b' ' {
        open -= 1;
    }
    let open = open.checked_sub(1)?;
    if bytes[open] != b'(' {
        return None;
    }
    let mut depth = 0u32;
    for token in Lexer::new(&src[open..]) {
        let t = token.ok()?;
        match t.kind {
            TokenKind::Open => depth += 1,
            TokenKind::Close => {
                depth -= 1;
                if depth == 0 {
                    return Some((open, open + t.end as usize));
                }
            }
            _ => {}
        }
    }
    None
}

fn point(node: NodeRef<'_>, name: &str) -> Option<Pt> {
    let c = node.child(name)?;
    Some((
        parse_board_units(c.atom(1)?)?,
        parse_board_units(c.atom(2)?)?,
    ))
}

fn number(node: NodeRef<'_>, name: &str) -> Option<i32> {
    node.child(name)
        .and_then(|w| w.atom(1))
        .and_then(parse_board_units)
}

/// Builds the copper index of a parsed board. `via_defs` are the via
/// padstacks the reader put in the DSN tree.
pub fn build_index(
    tree: &Tree<'_>,
    board: NodeRef<'_>,
    layers: &Layers,
    nets: &NetTable,
    via_defs: Vec<ViaDef>,
) -> CopperIndex {
    let src = tree.source();
    let mut index = CopperIndex {
        layers: layers
            .copper
            .iter()
            .map(|l| IndexLayer {
                std_name: l.std_name.clone(),
                dsn_name: l.name.clone(),
            })
            .collect(),
        net_names: nets.by_code.clone(),
        dsn_net_names: nets
            .by_code
            .iter()
            .map(|n| n.as_ref().map(|s| s.replace('"', "''")))
            .collect(),
        net_raw: vec![None; nets.by_code.len()],
        via_defs,
        ..CopperIndex::default()
    };
    for (raw, &code) in &nets.codes_by_name {
        if let Some(slot) = index.net_raw.get_mut(code as usize)
            && slot.is_none()
        {
            *slot = Some(raw.clone());
        }
    }
    // every uuid of the file, and the token the file's copper uses
    let mut token: Option<(String, bool)> = None;
    let mut first_any: Option<(String, bool)> = None;
    for idx in 0..tree.len() as u32 {
        let n = tree.node(idx);
        if !n.is_list() || !matches!(n.head(), Some("uuid" | "tstamp")) {
            continue;
        }
        if let Some(v) = n.str_value(1) {
            index.uuids.insert(v.to_ascii_lowercase());
        }
        if first_any.is_none() {
            first_any = n.item(1).map(|v| {
                (
                    n.head().unwrap_or("uuid").to_owned(),
                    v.kind() == NodeKind::Str,
                )
            });
        }
    }
    for node in board.items().filter(|c| c.is_list()) {
        let head = node.head();
        if !matches!(head, Some("segment" | "arc" | "via")) {
            continue;
        }
        if token.is_none() {
            for t in ["uuid", "tstamp"] {
                if let Some(u) = node.child(t).and_then(|u| u.item(1)) {
                    token = Some((t.to_owned(), u.kind() == NodeKind::Str));
                    break;
                }
            }
        }
        let Some(span_bytes) = block_span(src, node) else {
            continue;
        };
        let net = nets.net_of(node).filter(|c| *c > 0);
        let uuid = item_uuid(node).map(|u| u.into_owned());
        let locked = is_locked(node);
        let item = if head == Some("via") {
            let at = point(node, "at");
            let span = node.child("layers").and_then(|l| {
                let a = layer_position(layers, &l.str_value(1)?)?;
                let b = layer_position(layers, &l.str_value(2)?)?;
                Some((a.min(b), a.max(b)))
            });
            let width = number(node, "size");
            let kind = ["blind", "buried", "micro"]
                .iter()
                .find(|k| {
                    node.items()
                        .skip(1)
                        .any(|i| !i.is_list() && i.text() == **k)
                })
                .map(|k| (*k).to_owned())
                .unwrap_or_default();
            let at_pt = at.unwrap_or((0, 0));
            CopperItem {
                kind: CopperKind::Via { kind },
                start: at_pt,
                end: at_pt,
                points: vec![at_pt],
                net,
                layer: None,
                span,
                width,
                drill: Some(number(node, "drill").unwrap_or(0)),
                uuid,
                locked,
                routable: net.is_some() && at.is_some() && width.is_some() && span.is_some(),
                span_bytes,
            }
        } else {
            let (start, end) = (point(node, "start"), point(node, "end"));
            let layer = node
                .child("layer")
                .and_then(|l| l.str_value(1))
                .and_then(|n| layer_position(layers, &n));
            let mid = if head == Some("arc") {
                point(node, "mid")
            } else {
                None
            };
            let (s, e) = (start.unwrap_or((0, 0)), end.unwrap_or((0, 0)));
            let (kind, points) = match mid {
                Some(m) => (CopperKind::Arc { mid: m }, arc_points(s, m, e)),
                None => (CopperKind::Segment, vec![s, e]),
            };
            CopperItem {
                kind,
                start: s,
                end: e,
                points,
                net,
                layer,
                span: None,
                width: Some(number(node, "width").unwrap_or(0)),
                drill: None,
                uuid,
                locked,
                routable: net.is_some() && start.is_some() && end.is_some() && layer.is_some(),
                span_bytes,
            }
        };
        index.items.push(item);
    }
    let (t, quoted) = token.or(first_any).unwrap_or(("uuid".to_owned(), true));
    index.style = BlockStyle {
        dialect: nets.dialect,
        uuid_token: t,
        uuid_quoted: quoted,
    };
    index
}

/// What [`render`] did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WriteSummary {
    /// Copper blocks of the file left as they were.
    pub kept: usize,
    /// Copper blocks cut out.
    pub removed: usize,
    pub added_segments: usize,
    pub added_vias: usize,
}

/// The text of the board with `diff` applied. `file_hash` is the FNV-1a 128
/// of the input file's bytes (it seeds the new UUIDs). The result is parsed
/// again and must have balanced parentheses.
pub fn render(
    src: &str,
    index: &CopperIndex,
    diff: &CopperDiff,
    file_hash: u128,
) -> Result<(String, WriteSummary), KicadError> {
    render_with(
        src,
        index,
        diff,
        UuidGen::new(file_hash, index.uuids.iter().map(String::as_str)),
    )
}

/// [`render`] with a given UUID generator (tests force collisions with it).
pub fn render_with(
    src: &str,
    index: &CopperIndex,
    diff: &CopperDiff,
    mut uuids: UuidGen,
) -> Result<(String, WriteSummary), KicadError> {
    let fail = |m: String| KicadError::new(ErrorKind::Semantic, m);
    let (at, newline) = splice::insertion_point(src)
        .ok_or_else(|| fail("the board does not end with ')'".into()))?;
    let style = &index.style;
    let layer = |p: usize| -> Result<&str, KicadError> {
        index
            .layers
            .get(p)
            .map(|l| l.std_name.as_str())
            .ok_or_else(|| fail(format!("no copper layer at position {p}")))
    };
    let net = |code: u32| -> Result<String, KicadError> {
        let raw = match style.dialect {
            Dialect::Ids => String::new(),
            Dialect::Names => index
                .net_raw
                .get(code as usize)
                .cloned()
                .flatten()
                .ok_or_else(|| fail(format!("net code {code} has no name in the file")))?,
        };
        Ok(style.net(code, &raw))
    };
    let mut text = String::new();
    if newline {
        text.push('\n');
    }
    let mut summary = WriteSummary {
        kept: diff.keep.len(),
        removed: diff.remove.len(),
        ..WriteSummary::default()
    };
    for (i, item) in diff.add.iter().enumerate() {
        match item {
            NewItem::Segment {
                start,
                end,
                width,
                layer: l,
                net: n,
            } => {
                let coords = [
                    i64::from(start.0),
                    i64::from(start.1),
                    i64::from(end.0),
                    i64::from(end.1),
                ];
                let u = uuids.next(&uuid::UuidKey {
                    kind: "segment",
                    net: *n,
                    layer: *l as u32,
                    coords: &coords,
                    index: i as u64,
                });
                text.push_str(&format::segment_block(
                    style,
                    *start,
                    *end,
                    *width,
                    layer(*l)?,
                    &net(*n)?,
                    &u,
                ));
                summary.added_segments += 1;
            }
            NewItem::Via {
                at: c,
                diameter,
                drill,
                top,
                bottom,
                net: n,
                kind,
            } => {
                let coords = [i64::from(c.0), i64::from(c.1)];
                let u = uuids.next(&uuid::UuidKey {
                    kind: "via",
                    net: *n,
                    layer: *top as u32,
                    coords: &coords,
                    index: i as u64,
                });
                text.push_str(&format::via_block(
                    style,
                    kind,
                    *c,
                    *diameter,
                    *drill,
                    (layer(*top)?, layer(*bottom)?),
                    &net(*n)?,
                    &u,
                ));
                summary.added_vias += 1;
            }
        }
    }
    let removed: Vec<(usize, usize)> = diff
        .remove
        .iter()
        .filter_map(|&i| index.items.get(i))
        .map(|it| splice::removal_range(src, it.span_bytes.0, it.span_bytes.1))
        .collect();
    let out = splice::splice(src, &removed, at, &text);
    if !splice::is_balanced(&out) {
        return Err(fail(
            "refusing to write a board whose parentheses do not balance".into(),
        ));
    }
    Tree::parse(&out).map_err(|e| fail(format!("the written board does not parse: {e}")))?;
    Ok((out, summary))
}
