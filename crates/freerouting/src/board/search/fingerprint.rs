//! Tree fingerprints: the shape of a search tree as text and hashes, so a
//! probe can compare it with Java's (decision D5, B02.1).
//!
//! The canonical walk, which `tools/java-reference/probes/board/TreeProbe.java`
//! writes the same way, is a preorder walk (node, first child's subtree,
//! second child's subtree) with one line per node:
//! - inner node: `<depth> I <bounds>`;
//! - leaf: `<depth> L <bounds> <R|I> <id> <shape index>` (`R` for a room,
//!   `I` for an item).
//!
//! The root has depth 0. `<bounds>` is `O` and an octagon's eight fields in
//! the order `left_x bottom_y right_x top_y upper_left_diagonal_x
//! lower_right_diagonal_x lower_left_diagonal_x upper_right_diagonal_x`, or
//! `B llx lly urx ury` for a box. The lines are joined by `\n` with a
//! trailing `\n` and hashed with FNV-1a 64 over the UTF-8 bytes.
//!
//! [`ShapeSearchTree::fingerprint`] hashes the walk, so it changes with the
//! topology. [`ShapeSearchTree::content_digest`] hashes the leaves sorted by
//! (key, shape index), one line `<R|I> <id> <shape index> <bounds>` each, so
//! it ignores the topology: a live tree's shape depends on its history, and
//! only the content can be compared with a rebuild.

use std::fmt::Write as _;

use freerouting_geometry::RegularTileShape;

use super::{ShapeSearchTree, TreeObject};

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0100_0000_01b3;

fn fnv1a64(mut hash: u64, bytes: &[u8]) -> u64 {
    for b in bytes {
        hash ^= u64::from(*b);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

/// The shape of a search tree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TreeFingerprint {
    /// Inner nodes and leaves.
    pub nodes: usize,
    pub leaves: usize,
    /// The largest node depth; the root has depth 0.
    pub max_depth: usize,
    /// FNV-1a 64 of the canonical walk.
    pub shape_hash: u64,
}

/// Appends a bounding shape as the walk writes it.
fn write_bounds(out: &mut String, bounds: &RegularTileShape) {
    match bounds {
        RegularTileShape::IntOctagon(o) => {
            let _ = write!(
                out,
                "O {} {} {} {} {} {} {} {}",
                o.left_x,
                o.bottom_y,
                o.right_x,
                o.top_y,
                o.upper_left_diagonal_x,
                o.lower_right_diagonal_x,
                o.lower_left_diagonal_x,
                o.upper_right_diagonal_x
            );
        }
        RegularTileShape::IntBox(b) => {
            let _ = write!(out, "B {} {} {} {}", b.ll.x, b.ll.y, b.ur.x, b.ur.y);
        }
    }
}

fn write_object(out: &mut String, object: TreeObject, shape_index: usize) {
    let (label, id) = object.walk_label();
    let _ = write!(out, "{label} {id} {shape_index}");
}

impl ShapeSearchTree {
    /// The canonical preorder walk, one line per node (see the module
    /// docs).
    pub fn walk_lines(&self) -> Vec<String> {
        let mut lines = Vec::with_capacity(2 * self.tree.size());
        self.tree.preorder(|depth, bounds, leaf| {
            let mut line = String::new();
            match leaf {
                None => {
                    let _ = write!(line, "{depth} I ");
                    write_bounds(&mut line, bounds);
                }
                Some((object, shape_index)) => {
                    let _ = write!(line, "{depth} L ");
                    write_bounds(&mut line, bounds);
                    line.push(' ');
                    write_object(&mut line, object, shape_index);
                }
            }
            lines.push(line);
        });
        lines
    }

    /// Node and leaf counts, the maximum depth and the hash of the walk.
    pub fn fingerprint(&self) -> TreeFingerprint {
        let mut result = TreeFingerprint {
            nodes: 0,
            leaves: 0,
            max_depth: 0,
            shape_hash: FNV_OFFSET,
        };
        let mut line = String::new();
        self.tree.preorder(|depth, bounds, leaf| {
            line.clear();
            result.nodes += 1;
            result.max_depth = result.max_depth.max(depth);
            match leaf {
                None => {
                    let _ = write!(line, "{depth} I ");
                    write_bounds(&mut line, bounds);
                }
                Some((object, shape_index)) => {
                    result.leaves += 1;
                    let _ = write!(line, "{depth} L ");
                    write_bounds(&mut line, bounds);
                    line.push(' ');
                    write_object(&mut line, object, shape_index);
                }
            }
            line.push('\n');
            result.shape_hash = fnv1a64(result.shape_hash, line.as_bytes());
        });
        result
    }

    /// FNV-1a 64 of the leaves sorted by (key, shape index), one line
    /// `<R|I> <id> <shape index> <bounds>` each.
    pub fn content_digest(&self) -> u64 {
        let mut leaves: Vec<(TreeObject, usize, RegularTileShape)> = self
            .tree
            .to_array()
            .into_iter()
            .map(|leaf| {
                let (object, index) = self.tree.leaf(leaf);
                (object, index, self.tree.leaf_bounds(leaf))
            })
            .collect();
        leaves.sort_unstable_by_key(|(object, index, _)| (*object, *index));
        let mut hash = FNV_OFFSET;
        let mut line = String::new();
        for (object, index, bounds) in &leaves {
            line.clear();
            write_object(&mut line, *object, *index);
            line.push(' ');
            write_bounds(&mut line, bounds);
            line.push('\n');
            hash = fnv1a64(hash, line.as_bytes());
        }
        hash
    }
}
