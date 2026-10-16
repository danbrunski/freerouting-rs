//! The synthetic stress board of `--bench-synthetic 4x4 board.dsn`: the
//! board's scene tiled `nx` by `ny` times, with a gap of 10 % of the board
//! size between tiles and `ItemId`s offset per tile. It is test data for
//! the canvas; the engine is not involved and the session keeps the original
//! board.
//!
//! A 4x4 tiling of Issue190 has 16 times its 10,885 board items (174,160)
//! and about 5.6 million triangles, which is the FT2 board.

use freerouting::board::items::ItemId;

use crate::scene::{Bounds, Geom, Item, Label, P, Scene};
use crate::session::Loaded;
use crate::view::Tessellation;

/// Gap between tiles, as a fraction of the board size.
const GAP: f64 = 0.1;

fn shifted(geom: &Geom, dx: f64, dy: f64) -> Geom {
    let mv = |p: &P| P::new(p.x + dx, p.y + dy);
    match geom {
        Geom::Polygon { outer, holes } => Geom::Polygon {
            outer: outer.iter().map(mv).collect(),
            holes: holes.iter().map(|h| h.iter().map(mv).collect()).collect(),
        },
        Geom::Path { points, width } => Geom::Path {
            points: points.iter().map(mv).collect(),
            width: *width,
        },
        Geom::Circle { center, radius } => Geom::Circle {
            center: mv(center),
            radius: *radius,
        },
    }
}

/// `scene` tiled `nx` by `ny`, row by row. Tile `t` has the items of the
/// original with ids offset by `t` times the original's largest id plus one,
/// so ids stay unique and ascend with the tile.
///
/// # Panics
/// If the offset ids would overflow an `i32`.
pub fn tile(scene: &Scene, nx: u32, ny: u32) -> Scene {
    let (w, h) = (scene.bounds.width(), scene.bounds.height());
    let (step_x, step_y) = (w * (1.0 + GAP), h * (1.0 + GAP));
    let stride = i64::from(scene.items.iter().map(|i| i.id.0).max().unwrap_or(0)) + 1;
    let tiles = i64::from(nx) * i64::from(ny);
    assert!(stride * tiles <= i64::from(i32::MAX), "ids overflow");

    let mut out = Scene {
        name: format!("{} {nx}x{ny}", scene.name),
        layers: scene.layers.clone(),
        nets: scene.nets.clone(),
        components: scene.components * tiles as usize,
        ..Scene::default()
    };
    out.items.reserve(scene.items.len() * tiles as usize);
    for t in 0..tiles {
        let (dx, dy) = (
            (t % i64::from(nx)) as f64 * step_x,
            (t / i64::from(nx)) as f64 * step_y,
        );
        let offset = (t * stride) as i32;
        out.items.extend(scene.items.iter().map(|item| Item {
            id: ItemId(item.id.0 + offset),
            geom: shifted(&item.geom, dx, dy),
            ..item.clone()
        }));
        out.labels.extend(scene.labels.iter().map(|l| Label {
            center: P::new(l.center.x + dx, l.center.y + dy),
            ..l.clone()
        }));
    }
    out.bounds = Bounds {
        min: scene.bounds.min,
        max: P::new(
            scene.bounds.min.x + step_x * f64::from(nx - 1) + w,
            scene.bounds.min.y + step_y * f64::from(ny - 1) + h,
        ),
    };
    out.build_index();
    out
}

/// Replaces the scene and tessellation of `loaded` with the tiled ones.
pub fn tile_loaded(loaded: &mut Loaded, nx: u32, ny: u32) {
    loaded.scene = tile(&loaded.scene, nx, ny);
    loaded.tess = Tessellation::new(&loaded.scene);
    let built = loaded.tess.build_times();
    loaded.timings.tessellation_ms = built.tessellation_ms;
    loaded.timings.chunking_ms = built.chunking_ms;
}

/// FNV-1a over the vertex, index and segment buffers of every chunk in chunk
/// order, for comparing two builds.
pub fn checksum(tess: &Tessellation) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    let mut eat = |bytes: [u8; 4]| {
        for b in bytes {
            hash = (hash ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3);
        }
    };
    tess.with(|set| {
        for chunk in set.chunks() {
            for v in &chunk.mesh.vertices {
                v.iter().for_each(|f| eat(f.to_bits().to_le_bytes()));
            }
            chunk.mesh.indices.iter().for_each(|i| eat(i.to_le_bytes()));
            for l in &chunk.mesh.lines {
                l.iter().for_each(|f| eat(f.to_bits().to_le_bytes()));
            }
        }
    });
    hash
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn scene(name: &str) -> Scene {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/").to_string() + name;
        let bytes = std::fs::read(path).unwrap();
        let result = freerouting::io::specctra::read_board(&bytes);
        Scene::from_board("t", result.board().expect("board"))
    }

    #[test]
    fn tiling_is_deterministic() {
        let base = scene("Issue190-processor.Z80.dsn");
        let ids: BTreeSet<i32> = base.items.iter().map(|i| i.id.0).collect();
        let a = tile(&base, 4, 4);
        let b = tile(&base, 4, 4);
        eprintln!(
            "Issue190: {} scene items from {} engine items; 4x4: {} scene items, {} engine items",
            base.items.len(),
            ids.len(),
            a.items.len(),
            16 * ids.len()
        );
        // 10,885 items on the board (fixtures/reference/load), 12,473 shapes
        // once pads and vias are drawn per layer; 16 tiles of them.
        assert_eq!((ids.len(), 16 * ids.len()), (10_885, 174_160));
        assert_eq!(a.items.len(), 16 * base.items.len());
        assert_eq!(a.items, b.items);
        let tiled_ids: BTreeSet<i32> = a.items.iter().map(|i| i.id.0).collect();
        assert_eq!(tiled_ids.len(), 16 * ids.len());
        // The gap is 10 % of the board size: the whole is 4.3 boards wide.
        let w = base.bounds.width();
        assert!((a.bounds.width() - (4.0 * w + 3.0 * 0.1 * w)).abs() < 1e-9);
        let (ta, tb) = (Tessellation::new(&a), Tessellation::new(&b));
        assert_eq!(ta.triangle_count(), tb.triangle_count());
        assert_eq!(
            ta.triangle_count(),
            16 * Tessellation::new(&base).triangle_count()
        );
        assert_eq!(checksum(&ta), checksum(&tb));
    }
}
