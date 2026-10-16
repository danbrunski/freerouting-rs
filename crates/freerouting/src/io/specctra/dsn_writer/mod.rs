//! Writing a board as a Specctra design file.
//!
//! Port of `io.specctra.DsnWriter` and the `write*` methods of `io.specctra.parser.*`
//! (at `a773a70ec`; the Java files are identical to `cda540f`). Content and layout are
//! separate: [`pcb_from_board`] decides what goes into the file and builds a [`Pcb`] syntax
//! tree, the inverse of the reader in `parser/`; [`write_dsn`] and [`write_dsn_to`] write it with
//! the plain layout of [`Pcb::to_dsn`].
//!
//! ```text
//! (pcb name (parser ...) (resolution u n) (unit u)
//!   (structure layers, boundary (the bounding box, then each outline shape), via, rule,
//!              snap_angle, control, plane*, keepout*)
//!   (placement (component <image> (place ...)...)...)
//!   (library images, padstacks)  (part_library ...)
//!   (network nets, vias, via rules, classes)  (wiring wires, vias, conduction areas))
//! ```
//!
//! The scopes are built in one pass over the item list each, bucketed by kind, net or
//! component. Java scans the list once per net and once per component.
//!
//! # What is not ported
//!
//! * `compatMode` (the GUI's compatible export) is not ported: nothing headless uses it.
//! * `autoroute_settings` is never written, as in Java's `DsnWriter`, which passes `null`
//!   settings.
//! * The layout is the plain one, not `IndentFileWriter`'s. The comparison with Java's text is
//!   on parsed trees (`tests/dsn_round_trip.rs`).
//!
//! # Differences from Java
//!
//! Java's writer is not the inverse of its reader in several places. The Rust writer writes
//! what the reader needs to rebuild the board, so that `build_board(parse(write_dsn(board)))`
//! equals `board` (`tests/dsn_writer.rs`, `tests/dsn_round_trip.rs`). Each difference is listed
//! in `docs/DIVERGENCES.md` ("Writing a design file") and pinned by a test:
//!
//! * **Clearance classes.** Java writes `(clearance v (type name))` for each named class. Its
//!   reader skips a type list of one name that has no `_`, so the matrix is lost. Rust writes
//!   the pairs `(clearance v (type a b))` that rebuild the matrix (see `rules.rs`).
//! * **Net classes.** A class with `(rule (clearance v))` makes the reader create its clearance
//!   class and make it the default of every item kind of the class. Such classes are written
//!   that way, with a `use_via` per class that has a via rule of its own, and the via types
//!   and via rules the reader makes itself are left out. `rules.rs` checks on a copy that the
//!   reader ends with the board's matrix and rules, and falls back to Java's form if not.
//! * **Order.** Items are written oldest first; Java writes the newest first, which reverses
//!   the item list on every write and read.
//! * **Bounding box.** The `pcb` rectangle is the board's box less the 1000 the reader adds.
//! * **Layer rules** repeat the width of the layer, not (as Java does) the width of layer 0. A
//!   `use_layer` is left out when it lists the signal layers.
//! * **Keepout kinds.** Java writes a `place_keepout` of the structure as `keepout`; Rust writes
//!   `place_keepout`. Package keepouts of the place kind, which Java leaves out, are written.
//! * **Clearance classes of areas and the outline.** Java writes none for planes and the
//!   outline, and none for a keepout of class `default`. Rust adds the class whenever the
//!   reader's default would be another one.
//! * **Fixed state of conduction areas** is written (`(type protect)`), and `shove_fixed` is
//!   spelled as the reader knows it (Java writes `shoveFixed`).
//! * **Plane nets.** Power layers list their plane nets in `use_net`, and a fixed area of a
//!   plane net on a signal layer is a `plane` scope: only these make the reader mark the net.
//! * **Names of package keepouts** are written when the file named them, because the keepouts
//!   of a placed component refer to them.
//!
//! A placed component that owns no item is not written, as in Java.

mod library;
mod network;
mod placement;
mod rules;
mod structure;
mod wiring;

use std::io::{self, Write};

use freerouting_dsn::Pcb;
use freerouting_dsn::ast::{
    LayerRef, ParserInfo, Resolution, Shape as DsnShape, WireType, WriteResolution,
};
use freerouting_geometry::Area;

use super::ses_writer::{board_shape_to_dsn, dsn_unit};
use crate::board::BasicBoard;
use crate::board::items::Item;
use crate::board::structure::FixedState;
use crate::io::CoordinateTransform;
use crate::rules::{BoardRules, ItemClass};

/// What the scope builders share: the board and its coordinate transform.
pub(crate) struct Cx<'a> {
    pub board: &'a BasicBoard,
    pub transform: CoordinateTransform,
    /// The items in creation order, oldest first. `BasicBoard::items` lists the newest first
    /// (Java's `startReadObject`); the writer reverses that, so that the reader, which
    /// inserts in file order, rebuilds the item list in the order the board had.
    pub items: Vec<&'a Item>,
}

impl<'a> Cx<'a> {
    fn new(board: &'a BasicBoard) -> Self {
        let mut items: Vec<&Item> = board.items().collect();
        items.reverse();
        Cx {
            board,
            transform: board.communication.coordinate_transform,
            items,
        }
    }

    /// The name of clearance class `index` (`null` for class 0).
    pub fn class_name(&self, index: i32) -> String {
        self.board
            .rules
            .clearance_matrix
            .get_name(index)
            .unwrap_or("null")
            .to_string()
    }

    /// The name of layer `index`.
    pub fn layer_name(&self, index: i32) -> String {
        usize::try_from(index)
            .ok()
            .and_then(|i| self.board.layer_structure.layers.get(i))
            .map(|l| l.name.clone())
            .unwrap_or_default()
    }

    /// The clearance class the reader gives an item of `item_class` without an explicit class,
    /// in the default net class.
    pub fn default_class(&self, item_class: ItemClass) -> i32 {
        self.board
            .rules
            .default_net_class_id()
            .map_or(BoardRules::clearance_class_none(), |id| {
                self.board
                    .rules
                    .net_classes
                    .get(id)
                    .default_item_clearance_classes
                    .get(item_class)
            })
    }

    /// An area as the border and the holes in DSN shapes on `layer`.
    pub fn area_shapes(
        &self,
        area: &Area,
        layer: LayerRef,
        relative: bool,
    ) -> (Option<DsnShape>, Vec<DsnShape>) {
        let (border, holes) = match area {
            Area::Shape(shape) => (shape.clone(), Vec::new()),
            other => (other.get_border(), other.get_holes()),
        };
        let border = board_shape_to_dsn(&border, layer.clone(), &self.transform, relative);
        let holes = holes
            .iter()
            .filter_map(|hole| board_shape_to_dsn(hole, layer.clone(), &self.transform, relative))
            .collect();
        (border, holes)
    }
}

/// Java's `Wiring.writeFixedState`: the type of a fixed item. `SystemFixed` is `fix`, a user
/// fix is `protect`, and a shove fix is `shove_fixed` (Java writes `shoveFixed`).
pub(crate) fn fixed_state_type(state: FixedState) -> Option<WireType> {
    match state {
        FixedState::Unfixed => None,
        FixedState::ShoveFixed => Some(WireType::ShoveFixed),
        FixedState::SystemFixed => Some(WireType::Fix),
        FixedState::UserFixed => Some(WireType::Protect),
    }
}

/// True if a conduction area is written as a `plane` of the structure scope. That is every area
/// on a power layer. Java writes the areas on signal layers in the wiring scope, which loses
/// that the net is a plane net when the file was read: the reader sets the flag for the nets of
/// `plane` scopes. So a fixed area of a plane net on a signal layer is written as a plane too.
pub(crate) fn in_plane_scope(cx: &Cx, item: &Item) -> bool {
    let Some(area) = item.as_area() else {
        return false;
    };
    let Some(layer) = usize::try_from(area.layer())
        .ok()
        .and_then(|l| cx.board.layer_structure.layers.get(l))
    else {
        return false;
    };
    if !layer.is_signal {
        return true;
    }
    item.net_count() == 1
        && item.fixed_state() == FixedState::SystemFixed
        && cx
            .board
            .rules
            .nets
            .get(item.net_number(0))
            .is_some_and(|net| net.contains_plane())
}

/// The design file of `board` as a syntax tree: Java's `DsnWriter.writePcbScope` without the
/// layout. `design_name` is written into the `(pcb ...)` header.
pub fn pcb_from_board(board: &BasicBoard, design_name: &str) -> Pcb {
    let cx = Cx::new(board);
    let plan = rules::plan(&cx);
    let communication = &board.communication;
    let unit = dsn_unit(communication.unit);
    Pcb {
        name: design_name.to_string(),
        parser: Some(parser_info(board)),
        resolution: Some(Resolution {
            unit,
            value: communication.resolution,
        }),
        unit: Some(unit),
        structure: Some(structure::structure(
            &cx,
            plan.global,
            &plan.layers,
            plan.via.as_ref(),
        )),
        placement: Some(placement::placement(&cx)),
        library: Some(library::library(&cx)),
        part_library: library::part_library(&cx),
        network: Some(network::network(&cx, &plan.owned, plan.via.as_ref())),
        wiring: Some(wiring::wiring(&cx)),
    }
}

/// `Parser.writeScope` in its full form.
fn parser_info(board: &BasicBoard) -> ParserInfo {
    let info = board.communication.specctra_parser_info.as_ref();
    ParserInfo {
        string_quote: Some(info.map_or("\"", |i| i.string_quote.as_str()).to_string()),
        space_in_quoted_tokens: Some(true),
        host_cad: info.and_then(|i| i.host_cad.clone()),
        host_version: info.and_then(|i| i.host_version.clone()),
        constants: info.map(|i| i.constants.clone()).unwrap_or_default(),
        write_resolution: info.and_then(|i| i.write_resolution.as_ref()).map(|w| {
            // Java writes the first character of the name only.
            WriteResolution {
                name: w.char_name.chars().take(1).collect(),
                value: w.positive_int,
            }
        }),
        generated_by_freerouting: true,
    }
}

/// The design file of `board` as text in the plain layout of [`Pcb::to_dsn`].
pub fn write_dsn(board: &BasicBoard, design_name: &str) -> String {
    pcb_from_board(board, design_name).to_dsn()
}

/// Writes the design file of `board` to `out`. The output is the same as [`write_dsn`]'s;
/// `out` is flushed and not closed. Wrap a file in a `BufWriter`.
pub fn write_dsn_to<W: Write>(board: &BasicBoard, design_name: &str, mut out: W) -> io::Result<()> {
    out.write_all(write_dsn(board, design_name).as_bytes())?;
    out.flush()
}
