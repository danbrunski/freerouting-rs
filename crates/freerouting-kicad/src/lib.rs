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
//! Board structure (layers, nets, footprints, pads) and copper arrive with
//! K05 and K06.
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
//! GPL-3.0-only licence).

pub mod error;
pub mod header;
pub mod legacy;
pub mod nets;
pub mod sexpr;
pub mod units;

pub use error::{ErrorKind, KicadError, Warning};
