//! Direct reader for KiCad `.kicad_pcb` files.
//!
//! KiCad's DSN export loses information Freerouting needs: rule-area kinds,
//! track arcs and some precision (shapes, paths, wires, vias and pins are
//! printed with C `%.6g`, so absolute coordinates above 100 mm lose
//! everything below 1 µm). This crate reads the board file itself. Its output
//! is a [`freerouting_dsn::ast::Pcb`] in KiCad's DSN frame (`um`, Y negated,
//! no offset) that `freerouting::io::specctra::build_board` consumes
//! unchanged. The layers are:
//!
//! - [`sexpr`] (K04): a layout-independent lexer and an arena tree. Atoms and
//!   strings are byte spans into the source; numbers stay text until used.
//! - [`header`] (K04): `(kicad_pcb (version N) (generator ...))` and the
//!   version gate. Files before KiCad 6 (`20201115`) are refused.
//! - [`legacy`] (K04): the conversions KiCad applies when it loads an older
//!   file (old arcs, old overbars, `tstamp` as `uuid`), done on the tree.
//! - [`nets`] (K04): net dialect detection (names or numeric table) and the
//!   net table.
//!
//! - [`read`] (K05): `read( text, project )` builds the board structure of the
//!   DSN tree the way KiCad's own exporter (`specctra_export.cpp`, tag
//!   `10.0.0`; 10.0.7 for the reference exports) does: [`layers`], [`network`]
//!   and [`classes`] (net classes from the [`project`] file), [`outline`] (the
//!   board outline), [`footprints`] and [`pads`] (images, padstacks, pins),
//!   [`image_outlines`] (courtyards and graphics) and [`arc`] (KiCad's
//!   arc-to-polyline arithmetic).
//! - K07 adds [`dru`]: the board's `.kicad_dru` rules, read and reported, and
//!   applied (the class-pair clearances and class track widths) with
//!   `ReadOptions::apply_design_rules`.
//! - K06 adds the copper: [`zones`] reads `(zone ...)`; [`rule_areas`] maps rule
//!   areas to keepouts by their flags; copper zones become planes
//!   ([`zones::make_plane`]); [`tracks`] merges segments and arcs into wires;
//!   [`vias`] reads vias and their padstacks. [`report`] says what the reader
//!   decided ([`ReadReport`], in [`KicadRead::report`]).
//! - K08 adds [`write`]: the copper index of the file
//!   ([`KicadRead::copper_index`]), the geometry diff against a routed board,
//!   and the text splice that writes the routed copper into a copy of the
//!   file with deterministic UUIDs.
//!
//! Rule areas are the main departure from KiCad's export, which makes a full
//! `keepout` of any area that bans neither tracks nor vias. The reader maps by
//! flags: tracks and vias give `keepout`, vias only `via_keepout`, tracks only
//! `wire_keepout` (built as a keepout, as Java does, with a warning), and an
//! area that bans footprints also gets a `place_keepout`; an area that bans only
//! pads, copper pour or nothing emits no keepout and warns. Track arcs follow
//! the curve instead of KiCad's chord, copper zones owned by footprints stay
//! planes (`ReadOptions::drop_footprint_zones` restores KiCad's behaviour), and
//! the vias of net classes come in project order. `docs/DIVERGENCES.md` lists
//! each with its test.
//!
//! Parity with KiCad is tested against `fixtures/kicad/*/*.kicad10.dsn`, the
//! DSN KiCad 10.0.7 writes for each board (`tests/structure.rs`). Deviations:
//! every shape keeps KiCad's nanometre precision instead of `%.6g`; a custom
//! pad is the convex hull of its parts, like KiCad's export, but named with
//! its own hash; the start vertex Clipper gives a courtyard polygon is
//! reproduced by a rule fitted to the exports ([`outline::rotate_to_clipper_start`]);
//! and an empty or repeated reference designator is renamed (`REF**`,
//! `R1_1`) like KiCad 10.0.7 instead of refused (KiCad 10.0.0's behaviour is
//! [`ReadOptions::strict_references`]).
//!
//! # Determinism
//!
//! Children keep file order and every accessor iterates in file order; no
//! `HashMap` reaches a result.
//!
//! # Credits
//!
//! The pitfall list this reader was designed against (the `(version)` gates,
//! the quoted-string trap of issue #113, the net-dialect traps of issues
//! #1028, #163 and #369) comes from KiCadRoutingTools
//! (`drandyhaas/KiCadRoutingTools`, MIT); no code was taken from it. The
//! conversions in [`legacy`] and [`units`] are ported from KiCad itself
//! (`pcbnew/pcb_io/kicad_sexpr/pcb_io_kicad_sexpr_parser.cpp`,
//! `common/string_utils.cpp`, `common/eda_shape.cpp`,
//! `libs/kimath/src/trigo.cpp` and `libs/kimath/include/geometry/eda_angle.h`
//! at tag `10.0.0`; GPL-2.0-or-later, compatible with this crate's
//! GPL-3.0-only licence). The K05 modules port, from the same tag,
//! `pcbnew/specctra_import_export/specctra_export.cpp` and `specctra.h`,
//! `pcbnew/pad.cpp`, `footprint.cpp`, `convert_shape_list_to_polygon.cpp`,
//! `common/project/net_settings.cpp` and `eda_pattern_match.cpp`,
//! `libs/kimath/src/convert_basic_shapes_to_polygon.cpp`,
//! `geometry/shape_arc.cpp`, `geometry/arc_chord_params.cpp`,
//! `geometry/convex_hull.cpp` and `math/vector2d.h`. No KiCadRoutingTools
//! code was taken in K05 either (F04 does not apply).

pub mod arc;
pub mod classes;
pub mod dru;
pub mod error;
pub mod fmt6g;
pub mod footprints;
pub mod header;
pub mod image_outlines;
pub mod layers;
pub mod legacy;
pub mod nets;
pub mod network;
pub mod outline;
pub mod pads;
pub mod project;
pub mod read;
pub mod report;
pub mod rule_areas;
pub mod sexpr;
pub mod tracks;
pub mod units;
pub mod vias;
pub mod write;
pub mod zones;

pub use error::{ErrorKind, KicadError, Warning};
pub use read::{
    DieLength, Extras, KicadRead, ReadOptions, read, read_named, read_with, read_with_rules,
};
pub use report::ReadReport;
