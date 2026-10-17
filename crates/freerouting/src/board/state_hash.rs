//! `state_hash`: a hash of the board state with Java's equality (B01.5).
//!
//! Java's `BoardSnapshotManager.getHash` is the MD5 of `serialize(true)`:
//! `getTraces()`, `getVias()` and the item list `UndoableObjects` with its
//! whole undo structure. No field of `UndoableObjects` or its nodes is
//! transient, so the hash covers every version node, the deleted-objects
//! stack, `stackLevel` and `redoPossible`; an item contributes every
//! non-transient field, including `onTheBoard`.
//!
//! [`BasicBoard::state_hash`] is FNV-1a 128 (hand-written) over a canonical
//! byte encoding of the same content: the undo structure's stack level and
//! `redo_possible`, every version node reachable from the key map (key
//! order, so the live items come in item-list order; then the undo and redo
//! links, a node seen before by its handle), each with its level and its
//! item, and the deleted-objects stack, level by level, in list order. An
//! item contributes its id, kind, nets, clearance class, component id, fixed
//! state, `on_the_board` and geometry (trace: layer, half width, lines; via:
//! padstack, center, attach and escape flags; pin: pin index, `changed_to`
//! and center; areas and outlines: their full data, as the FNV-1a 64 of their
//! `Debug` text, which holds every field and no cache). Java's
//! `getTraces()` and `getVias()` prefix is left out: the item list already
//! determines it, so it can't change an equality.
//!
//! What must match Java is equality, not bytes: two board states hash equal
//! in Rust exactly when they hash equal in Java. One field is left out: the
//! DRC cache `Item.smallestClearance`, which Java's `clearanceViolations()`
//! writes as a side effect, so computing statistics changes Java's hash. The
//! port has no such cache (`docs/DIVERGENCES.md`; B00 open question 2).
//!
//! The hash is computed on demand. Callers use it once per pass or history
//! entry, which a full scan affords.

use std::fmt::Write as _;

use crate::board::basic_board::BasicBoard;
use crate::board::items::{AreaKind, Item, ItemKind};
use crate::board::structure::FixedState;
use crate::datastructures::StateRecord;

const FNV128_OFFSET: u128 = 0x6c62_272e_07bb_0142_62b8_2175_6295_c58d;
const FNV128_PRIME: u128 = 0x0000_0000_0100_0000_0000_0000_0000_013b;

/// FNV-1a 128 over the bytes it is given.
struct Fnv128(u128);

impl Fnv128 {
    fn bytes(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 ^= u128::from(b);
            self.0 = self.0.wrapping_mul(FNV128_PRIME);
        }
    }

    fn u8(&mut self, value: u8) {
        self.bytes(&[value]);
    }

    fn i32(&mut self, value: i32) {
        self.bytes(&value.to_le_bytes());
    }

    fn u64(&mut self, value: u64) {
        self.bytes(&value.to_le_bytes());
    }

    fn bool(&mut self, value: bool) {
        self.u8(u8::from(value));
    }

    /// Adds the FNV-1a 64 of `value`'s `Debug` text (a 64-bit digest keeps
    /// the long text of areas and outlines cheap).
    fn debug_digest(&mut self, value: &impl std::fmt::Debug) {
        let mut digest = Fnv64(FNV64_OFFSET);
        let _ = write!(digest, "{value:?}");
        self.u64(digest.0);
    }
}

const FNV64_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV64_PRIME: u64 = 0x0000_0100_0000_01b3;

/// FNV-1a 64 over the text written to it.
struct Fnv64(u64);

impl std::fmt::Write for Fnv64 {
    fn write_str(&mut self, s: &str) -> std::fmt::Result {
        for &b in s.as_bytes() {
            self.0 ^= u64::from(b);
            self.0 = self.0.wrapping_mul(FNV64_PRIME);
        }
        Ok(())
    }
}

/// Record tags, so that no two encodings run together.
mod tag {
    pub const ITEM: u8 = 1;
    pub const HEADER: u8 = 2;
    pub const KEY: u8 = 3;
    pub const NODE: u8 = 4;
    pub const SEEN: u8 = 5;
    pub const NO_NODE: u8 = 6;
    pub const DELETED_LEVEL: u8 = 7;
}

fn fixed_state_code(state: FixedState) -> u8 {
    match state {
        FixedState::Unfixed => 0,
        FixedState::ShoveFixed => 1,
        FixedState::UserFixed => 2,
        FixedState::SystemFixed => 3,
    }
}

/// The canonical encoding of one item (every field Java serializes except
/// `smallestClearance`).
fn encode_item(h: &mut Fnv128, item: &Item) {
    h.u8(tag::ITEM);
    h.i32(item.id().0);
    h.u64(item.net_numbers.len() as u64);
    for net in &item.net_numbers {
        h.i32(*net);
    }
    h.i32(item.clearance_class);
    h.i32(item.component_id);
    h.u8(fixed_state_code(item.fixed_state));
    h.bool(item.on_the_board);
    match &item.kind {
        ItemKind::Trace(trace) => {
            h.u8(10);
            h.i32(trace.layer);
            h.i32(trace.half_width);
            let lines = trace.polyline.lines();
            h.u64(lines.len() as u64);
            for line in lines {
                let (a, b) = (line.a(), line.b());
                h.i32(a.x);
                h.i32(a.y);
                h.i32(b.x);
                h.i32(b.y);
            }
        }
        ItemKind::Via(via) => {
            h.u8(11);
            h.u64(via.drill.padstack.0 as u64);
            h.i32(via.drill.center.x);
            h.i32(via.drill.center.y);
            h.bool(via.attach_allowed);
            h.bool(via.is_escape_via);
            h.i32(via.escape_via_smd_layer);
        }
        ItemKind::Pin(pin) => {
            h.u8(12);
            h.u64(pin.pin_index as u64);
            h.i32(pin.changed_to.0);
            h.i32(pin.drill.center.x);
            h.i32(pin.drill.center.y);
        }
        ItemKind::Area(area) => {
            h.u8(match area.kind {
                AreaKind::Obstacle => 13,
                AreaKind::ViaObstacle => 14,
                AreaKind::ComponentObstacle => 15,
                AreaKind::Conduction { .. } => 16,
                AreaKind::TraceObstacle => 19,
            });
            h.debug_digest(area);
        }
        ItemKind::ComponentOutline(outline) => {
            h.u8(17);
            h.debug_digest(outline);
        }
        ItemKind::BoardOutline(outline) => {
            h.u8(18);
            h.debug_digest(outline);
        }
    }
}

impl BasicBoard {
    /// A hash of the board state that is equal for two states exactly when
    /// Java's `getHash` is (see [`crate::board::state_hash`]).
    pub fn state_hash(&self) -> u128 {
        let mut h = Fnv128(FNV128_OFFSET);
        self.items.walk_state(|record| match record {
            StateRecord::Header {
                stack_level,
                redo_possible,
            } => {
                h.u8(tag::HEADER);
                h.u64(stack_level as u64);
                h.bool(redo_possible);
            }
            StateRecord::Key(key) => {
                h.u8(tag::KEY);
                h.i32(key.0);
            }
            StateRecord::Node { level, object } => {
                h.u8(tag::NODE);
                h.u64(level as u64);
                encode_item(&mut h, object);
            }
            StateRecord::Seen(handle) => {
                h.u8(tag::SEEN);
                h.u64(handle as u64);
            }
            StateRecord::NoNode => h.u8(tag::NO_NODE),
            StateRecord::DeletedLevel(len) => {
                h.u8(tag::DELETED_LEVEL);
                h.u64(len as u64);
            }
        });
        h.0
    }
}
