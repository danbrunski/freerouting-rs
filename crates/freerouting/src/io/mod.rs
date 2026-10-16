//! Conversion between the design file formats and the board model.
//!
//! [`read_design`] picks the reader by extension: `.dsn` is the Specctra
//! reader ([`specctra`]), `.kicad_pcb` the direct KiCad reader ([`kicad`], K07.1).

mod coordinate_transform;
pub mod kicad;
pub mod specctra;

pub use coordinate_transform::CoordinateTransform;
pub use kicad::{DesignKind, DesignRead, design_kind, read_design, read_kicad_board};
pub use specctra::{
    BoardReadResult, SesImportSummary, read_board, read_board_from_str, read_session,
};
