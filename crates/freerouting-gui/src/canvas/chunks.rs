//! The board's geometry in chunks: the CPU side of the canvas.
//!
//! The scene's items are grouped by (slot, kind), then by the cell of a
//! uniform grid over the board that holds their centre, and each group is cut
//! into chunks of at most [`MAX_CHUNK_ITEMS`] items. A chunk keeps its items
//! in ascending `ItemId` order, its triangles and hairline segments, and the
//! bounding box of its items, so the painter can skip chunks that are off
//! screen and a change can rebuild just the chunks it touches.
//!
//! The order inside a chunk is a function of its item set alone. A chunk
//! built by [`ChunkSet::patch`] is therefore bit-identical to the one a fresh
//! [`ChunkSet::build`] makes from the same items (`patch_equals_fresh_build`).
//! A chunk covers a range of ids, `lo_id ..` the next chunk's `lo_id`, of its
//! (slot, kind, cell) group, and keeps the range when items come and go, so
//! an item removed and added again lands in the chunk it came from.
//!
//! Patching only touches the chunks that own a removed id or that receive an
//! added item; they are listed as dirty and the GPU re-uploads just those.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicU64, Ordering};

use freerouting::board::items::ItemId;

use crate::scene::{Bounds, Item, Kind, P, Scene, Slot};
use crate::view::tessellate_item;

/// Items per chunk when a group is cut.
pub const MAX_CHUNK_ITEMS: usize = 4096;
/// A chunk that grows past this many items is split in two.
pub const SPLIT_ITEMS: usize = 8192;
/// The grid's cells are sized so the median occupied cell holds about this
/// many items.
pub const TARGET_CELL_ITEMS: usize = 1000;
/// Bytes of one hairline segment: two endpoints and a width, as `f32`.
pub const LINE_BYTES: usize = 20;

/// Number of kinds.
pub const KINDS: usize = Kind::ALL.len();

pub fn kind_index(kind: Kind) -> usize {
    match kind {
        Kind::Plane => 0,
        Kind::Keepout => 1,
        Kind::ViaKeepout => 2,
        Kind::PlaceKeepout => 3,
        Kind::Trace => 4,
        Kind::Pad => 5,
        Kind::Via => 6,
        Kind::Outline => 7,
        Kind::ComponentOutline => 8,
    }
}

/// Position of `slot` in the slot list: the copper layers, then the back
/// silk, the front silk and the board.
pub fn slot_index(slot: Slot, layers: usize) -> usize {
    use freerouting::dsn::ast::Side;
    match slot {
        Slot::Layer(i) => i,
        Slot::Silk(Side::Back) => layers,
        Slot::Silk(Side::Front) => layers + 1,
        Slot::Board => layers + 2,
    }
}

/// Number of slots for a board with `layers` copper layers.
pub fn slot_count(layers: usize) -> usize {
    layers + 3
}

/// The triangles and hairline segments of some items. Indices count from the
/// first vertex of this buffer.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MeshBuf {
    pub vertices: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
    /// `[ax, ay, bx, by, width]` in millimetres relative to the origin; a
    /// width of 0 is drawn one point wide.
    pub lines: Vec<[f32; 5]>,
}

impl MeshBuf {
    /// Bytes the buffers take on the GPU.
    pub fn byte_size(&self) -> usize {
        self.vertices.len() * 8 + self.indices.len() * 4 + self.lines.len() * LINE_BYTES
    }
}

/// Where one scene item's geometry sits in its chunk's buffers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Entry {
    pub id: i32,
    /// Position among the scene items flattened from the same engine item.
    pub seq: u32,
    pub bounds: Bounds,
    pub v0: u32,
    pub vn: u32,
    pub i0: u32,
    pub i_n: u32,
    pub l0: u32,
    pub ln: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Chunk {
    pub slot: Slot,
    pub kind: Kind,
    /// Row-major cell of the grid.
    pub cell: u32,
    /// The chunk holds ids from here up to the next chunk of its group.
    pub lo_id: i32,
    /// In ascending `(id, seq)` order.
    pub entries: Vec<Entry>,
    pub mesh: MeshBuf,
    /// The union of the entries' bounds, in board millimetres.
    pub bounds: Bounds,
}

impl Chunk {
    pub fn triangles(&self) -> usize {
        self.mesh.indices.len() / 3
    }

    fn empty(slot: Slot, kind: Kind, cell: u32, lo_id: i32) -> Chunk {
        Chunk {
            slot,
            kind,
            cell,
            lo_id,
            entries: Vec::new(),
            mesh: MeshBuf::default(),
            bounds: Bounds::EMPTY,
        }
    }

    fn recompute_bounds(&mut self) {
        let mut b = Bounds::EMPTY;
        for e in &self.entries {
            b.union(&e.bounds);
        }
        self.bounds = b;
    }
}

/// A uniform grid over the board.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grid {
    pub min: P,
    pub cell: f64,
    pub nx: u32,
    pub ny: u32,
}

impl Grid {
    /// The row-major cell holding `p`; points outside are clamped to the
    /// border cells.
    pub fn cell_of(&self, p: P) -> u32 {
        let ix = ((p.x - self.min.x) / self.cell).floor().max(0.0) as u32;
        let iy = ((p.y - self.min.y) / self.cell).floor().max(0.0) as u32;
        iy.min(self.ny - 1) * self.nx + ix.min(self.nx - 1)
    }

    fn with_cell(min: P, w: f64, h: f64, cell: f64) -> Grid {
        Grid {
            min,
            cell,
            nx: ((w / cell).ceil() as u32).max(1),
            ny: ((h / cell).ceil() as u32).max(1),
        }
    }

    /// A grid over `centers` whose median occupied cell holds about
    /// [`TARGET_CELL_ITEMS`] of them.
    fn choose(centers: &[P]) -> Grid {
        let mut b = Bounds::EMPTY;
        centers.iter().for_each(|&c| b.add(c));
        if b.is_empty() {
            return Grid::with_cell(P::new(0.0, 0.0), 1.0, 1.0, 1.0);
        }
        let (w, h) = (b.width().max(1e-6), b.height().max(1e-6));
        let target_cells = (centers.len() / TARGET_CELL_ITEMS).max(1) as f64;
        let floor = w.max(h) / 512.0;
        let mut cell = (w * h / target_cells).sqrt().max(floor);
        let mut grid = Grid::with_cell(b.min, w, h, cell);
        for _ in 0..6 {
            let mut count = vec![0u32; grid.nx as usize * grid.ny as usize];
            for &c in centers {
                count[grid.cell_of(c) as usize] += 1;
            }
            let mut occupied: Vec<u32> = count.into_iter().filter(|&n| n > 0).collect();
            occupied.sort_unstable();
            let median = f64::from(occupied[occupied.len() / 2]);
            if (700.0..=1400.0).contains(&median) {
                break;
            }
            let next =
                (cell * (TARGET_CELL_ITEMS as f64 / median).sqrt().clamp(0.5, 2.0)).max(floor);
            if next == cell {
                break;
            }
            cell = next;
            grid = Grid::with_cell(b.min, w, h, cell);
        }
        grid
    }
}

/// (slot index, kind index, cell).
type GroupKey = (u32, u32, u32);

/// What [`ChunkSet::patch`] changed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PatchReport {
    /// The chunks that were rebuilt, ascending.
    pub dirty_chunks: Vec<u32>,
    /// Bytes of their buffers after the patch, which is what the GPU
    /// uploads.
    pub dirty_bytes: u64,
    pub chunks_created: usize,
    pub splits: usize,
}

#[derive(Default)]
struct Edit {
    remove: BTreeSet<i32>,
    /// `(id, seq, index into the added items)`.
    add: Vec<(i32, u32, usize)>,
}

/// Counters of the work a [`ChunkSet`] did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ChunkStats {
    /// Calls of [`ChunkSet::build`] that made this set (always 1).
    pub full_builds: u32,
    pub patches: u32,
    pub chunks_rebuilt: u64,
    pub splits: u32,
}

/// How long the phases of a build took, in milliseconds.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BuildTimes {
    /// Bounds, grid and grouping.
    pub chunking_ms: f64,
    /// Triangulating the items into the chunks.
    pub tessellation_ms: f64,
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

pub struct ChunkSet {
    id: u64,
    origin: P,
    layers: usize,
    grid: Grid,
    chunks: Vec<Chunk>,
    /// The chunks of each group by ascending `lo_id`.
    groups: BTreeMap<GroupKey, Vec<u32>>,
    /// The chunks of each (slot, kind), indexed `slot * KINDS + kind`.
    batches: Vec<Vec<u32>>,
    /// The chunks that hold an engine item, by id.
    owners: BTreeMap<i32, Vec<u32>>,
    dirty: BTreeSet<u32>,
    pub stats: ChunkStats,
    pub times: BuildTimes,
}

impl ChunkSet {
    /// Chunks the scene. Vertices are relative to `origin`.
    pub fn build(scene: &Scene, origin: P) -> ChunkSet {
        let t0 = std::time::Instant::now();
        let layers = scene.layers.len();
        let bounds: Vec<Bounds> = scene.items.iter().map(|i| i.geom.bounds()).collect();
        let centers: Vec<P> = bounds.iter().map(Bounds::center).collect();
        let grid = Grid::choose(&centers);

        // (group, id, seq, scene index), sorted: groups together, ids
        // ascending inside.
        let mut order: Vec<(GroupKey, i32, u32, u32)> = Vec::with_capacity(scene.items.len());
        let mut seq = 0u32;
        for (n, item) in scene.items.iter().enumerate() {
            seq = if n > 0 && scene.items[n - 1].id == item.id {
                seq + 1
            } else {
                0
            };
            let slot = slot_index(item.slot, layers);
            if slot >= slot_count(layers) {
                continue;
            }
            let key = (
                slot as u32,
                kind_index(item.kind) as u32,
                grid.cell_of(centers[n]),
            );
            order.push((key, item.id.0, seq, n as u32));
        }
        order.sort_unstable();
        let group_ms = t0.elapsed().as_secs_f64() * 1000.0;

        let mut set = ChunkSet {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            origin,
            layers,
            grid,
            chunks: Vec::new(),
            groups: BTreeMap::new(),
            batches: vec![Vec::new(); slot_count(layers) * KINDS],
            owners: BTreeMap::new(),
            dirty: BTreeSet::new(),
            stats: ChunkStats {
                full_builds: 1,
                ..ChunkStats::default()
            },
            times: BuildTimes::default(),
        };
        let mut owner_pairs: Vec<(i32, u32)> = Vec::new();
        let mut start = 0;
        while start < order.len() {
            let key = order[start].0;
            let mut end = start;
            while end < order.len() && order[end].0 == key {
                end += 1;
            }
            let mut piece = start;
            while piece < end {
                // Cut at an id boundary so one engine item stays in one
                // chunk.
                let mut cut = (piece + MAX_CHUNK_ITEMS).min(end);
                while cut < end && order[cut].1 == order[cut - 1].1 {
                    cut += 1;
                }
                let lo_id = if piece == start {
                    i32::MIN
                } else {
                    order[piece].1
                };
                let slot = slot_of_index(key.0 as usize, layers);
                let kind = Kind::ALL[key.1 as usize];
                let mut chunk = Chunk::empty(slot, kind, key.2, lo_id);
                let id = set.chunks.len() as u32;
                for &(_, item_id, seq, n) in &order[piece..cut] {
                    let n = n as usize;
                    append_item(&mut chunk, origin, &scene.items[n], seq, bounds[n]);
                    if owner_pairs.last() != Some(&(item_id, id)) {
                        owner_pairs.push((item_id, id));
                    }
                }
                chunk.recompute_bounds();
                set.groups.entry(key).or_default().push(id);
                set.batches[key.0 as usize * KINDS + key.1 as usize].push(id);
                set.chunks.push(chunk);
                piece = cut;
            }
            start = end;
        }
        // Ids ascend inside a group but not across groups, so sort once.
        owner_pairs.sort_unstable();
        owner_pairs.dedup();
        for (item_id, chunk) in owner_pairs {
            set.owners.entry(item_id).or_default().push(chunk);
        }
        set.dirty = (0..set.chunks.len() as u32).collect();
        set.times = BuildTimes {
            chunking_ms: group_ms,
            tessellation_ms: (t0.elapsed().as_secs_f64() * 1000.0 - group_ms).max(0.0),
        };
        set
    }

    /// A number that differs between sets, for the GPU to tell them apart.
    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn origin(&self) -> P {
        self.origin
    }

    pub fn layer_count(&self) -> usize {
        self.layers
    }

    pub fn grid(&self) -> &Grid {
        &self.grid
    }

    pub fn chunks(&self) -> &[Chunk] {
        &self.chunks
    }

    pub fn chunk(&self, id: u32) -> &Chunk {
        &self.chunks[id as usize]
    }

    /// The chunks of one (slot, kind).
    pub fn batch(&self, slot: usize, kind: usize) -> &[u32] {
        &self.batches[slot * KINDS + kind]
    }

    /// The chunks that own the engine item `id`.
    pub fn owners_of(&self, id: ItemId) -> &[u32] {
        self.owners.get(&id.0).map_or(&[], Vec::as_slice)
    }

    /// Number of engine items with geometry in the set.
    pub fn item_count(&self) -> usize {
        self.owners.len()
    }

    pub fn triangle_count(&self) -> usize {
        self.chunks.iter().map(Chunk::triangles).sum()
    }

    /// Bytes of every chunk's buffers.
    pub fn byte_size(&self) -> u64 {
        self.chunks.iter().map(|c| c.mesh.byte_size() as u64).sum()
    }

    /// The chunks changed since the last call, ascending. A new set lists
    /// all its chunks.
    pub fn take_dirty(&mut self) -> Vec<u32> {
        std::mem::take(&mut self.dirty).into_iter().collect()
    }

    fn key_of(&self, item: &Item, center: P) -> Option<GroupKey> {
        let slot = slot_index(item.slot, self.layers);
        (slot < slot_count(self.layers)).then(|| {
            (
                slot as u32,
                kind_index(item.kind) as u32,
                self.grid.cell_of(center),
            )
        })
    }

    /// The chunk of `key` that takes item `id`, created if the group is new.
    fn chunk_for(&mut self, key: GroupKey, id: i32, created: &mut usize) -> u32 {
        if let Some(list) = self.groups.get(&key) {
            let at = list.partition_point(|&c| self.chunks[c as usize].lo_id <= id);
            return list[at.saturating_sub(1)];
        }
        let new = self.chunks.len() as u32;
        let slot = slot_of_index(key.0 as usize, self.layers);
        self.chunks.push(Chunk::empty(
            slot,
            Kind::ALL[key.1 as usize],
            key.2,
            i32::MIN,
        ));
        self.groups.insert(key, vec![new]);
        self.batches[key.0 as usize * KINDS + key.1 as usize].push(new);
        *created += 1;
        new
    }

    /// Removes the scene items of the engine items `removed` and adds
    /// `added`, rebuilding only the chunks that change. `added` holds all
    /// the scene items of an engine item together, in flatten order; ids in
    /// `added` must not be on the board already unless they are in
    /// `removed` too.
    pub fn patch(&mut self, removed: &[ItemId], added: &[Item]) -> PatchReport {
        let mut report = PatchReport::default();
        let mut edits: BTreeMap<u32, Edit> = BTreeMap::new();
        for id in removed {
            for chunk in self.owners.remove(&id.0).unwrap_or_default() {
                edits.entry(chunk).or_default().remove.insert(id.0);
            }
        }
        let mut seq = 0u32;
        for (n, item) in added.iter().enumerate() {
            seq = if n > 0 && added[n - 1].id == item.id {
                seq + 1
            } else {
                0
            };
            let bounds = item.geom.bounds();
            let Some(key) = self.key_of(item, bounds.center()) else {
                continue;
            };
            let chunk = self.chunk_for(key, item.id.0, &mut report.chunks_created);
            edits
                .entry(chunk)
                .or_default()
                .add
                .push((item.id.0, seq, n));
            let owners = self.owners.entry(item.id.0).or_default();
            if !owners.contains(&chunk) {
                owners.push(chunk);
            }
        }
        for (chunk, mut edit) in edits {
            edit.add.sort_unstable();
            self.rebuild(chunk, &edit, added);
            self.dirty.insert(chunk);
            report.dirty_chunks.push(chunk);
            let mut oversize = vec![chunk];
            while let Some(c) = oversize.pop() {
                if self.chunks[c as usize].entries.len() > SPLIT_ITEMS
                    && let Some(new) = self.split(c)
                {
                    report.splits += 1;
                    report.dirty_chunks.push(new);
                    oversize.extend([c, new]);
                }
            }
        }
        report.dirty_chunks.sort_unstable();
        report.dirty_bytes = report
            .dirty_chunks
            .iter()
            .map(|&c| self.chunks[c as usize].mesh.byte_size() as u64)
            .sum();
        self.stats.patches += 1;
        self.stats.chunks_rebuilt += report.dirty_chunks.len() as u64;
        self.stats.splits += report.splits as u32;
        report
    }

    /// Rebuilds one chunk: its entries minus the removed ids, merged with
    /// the added items, copying the geometry of the ones that stay.
    fn rebuild(&mut self, id: u32, edit: &Edit, added: &[Item]) {
        let origin = self.origin;
        let old = &self.chunks[id as usize];
        let mut chunk = Chunk::empty(old.slot, old.kind, old.cell, old.lo_id);
        chunk.mesh.vertices.reserve(old.mesh.vertices.len());
        chunk.mesh.indices.reserve(old.mesh.indices.len());
        let mut keep = old
            .entries
            .iter()
            .filter(|e| !edit.remove.contains(&e.id))
            .peekable();
        let mut add = edit.add.iter().peekable();
        loop {
            let take_old = match (keep.peek(), add.peek()) {
                (None, None) => break,
                (Some(_), None) => true,
                (None, Some(_)) => false,
                (Some(e), Some(&&(aid, aseq, _))) => (e.id, e.seq) < (aid, aseq),
            };
            if take_old {
                let e = keep.next().expect("peeked");
                copy_entry(&mut chunk, &old.mesh, e);
            } else {
                let &(_, seq, n) = add.next().expect("peeked");
                let item = &added[n];
                append_item(&mut chunk, origin, item, seq, item.geom.bounds());
            }
        }
        chunk.recompute_bounds();
        self.chunks[id as usize] = chunk;
    }

    /// Cuts a chunk in two at an id boundary near its middle. Returns the
    /// new chunk, or `None` if there is no boundary.
    fn split(&mut self, id: u32) -> Option<u32> {
        let chunk = &mut self.chunks[id as usize];
        let n = chunk.entries.len();
        let boundary = |at: usize| chunk.entries[at].id != chunk.entries[at - 1].id;
        let mid = (n / 2..n)
            .find(|&at| boundary(at))
            .or_else(|| (1..n / 2).rev().find(|&at| boundary(at)))?;
        let first = chunk.entries[mid];
        let mut tail = Chunk::empty(chunk.slot, chunk.kind, chunk.cell, first.id);
        tail.entries = chunk.entries.split_off(mid);
        for e in &mut tail.entries {
            e.v0 -= first.v0;
            e.i0 -= first.i0;
            e.l0 -= first.l0;
        }
        tail.mesh.vertices = chunk.mesh.vertices.split_off(first.v0 as usize);
        tail.mesh.indices = chunk
            .mesh
            .indices
            .split_off(first.i0 as usize)
            .into_iter()
            .map(|i| i - first.v0)
            .collect();
        tail.mesh.lines = chunk.mesh.lines.split_off(first.l0 as usize);
        chunk.recompute_bounds();
        tail.recompute_bounds();
        let (slot, kind, cell) = (chunk.slot, chunk.kind, chunk.cell);
        let new = self.chunks.len() as u32;
        let moved: BTreeSet<i32> = tail.entries.iter().map(|e| e.id).collect();
        self.chunks.push(tail);
        let key = (
            slot_index(slot, self.layers) as u32,
            kind_index(kind) as u32,
            cell,
        );
        let list = self.groups.get_mut(&key).expect("group of a chunk");
        let at = list.iter().position(|&c| c == id).expect("chunk in group") + 1;
        list.insert(at, new);
        self.batches[key.0 as usize * KINDS + key.1 as usize].push(new);
        for item in moved {
            if let Some(owners) = self.owners.get_mut(&item) {
                for o in owners.iter_mut().filter(|o| **o == id) {
                    *o = new;
                }
            }
        }
        self.dirty.insert(id);
        self.dirty.insert(new);
        Some(new)
    }
}

/// The slot at position `i` of the slot list.
pub fn slot_of_index(i: usize, layers: usize) -> Slot {
    use freerouting::dsn::ast::Side;
    match i.checked_sub(layers) {
        None => Slot::Layer(i),
        Some(0) => Slot::Silk(Side::Back),
        Some(1) => Slot::Silk(Side::Front),
        Some(_) => Slot::Board,
    }
}

/// Tessellates `item` onto the end of `chunk`.
fn append_item(chunk: &mut Chunk, origin: P, item: &Item, seq: u32, bounds: Bounds) {
    let m = &mut chunk.mesh;
    let (v0, i0, l0) = (m.vertices.len(), m.indices.len(), m.lines.len());
    tessellate_item(origin, item, m);
    // Indices are relative to the item's first vertex until here.
    for i in &mut m.indices[i0..] {
        *i += v0 as u32;
    }
    chunk.entries.push(Entry {
        id: item.id.0,
        seq,
        bounds,
        v0: v0 as u32,
        vn: (m.vertices.len() - v0) as u32,
        i0: i0 as u32,
        i_n: (m.indices.len() - i0) as u32,
        l0: l0 as u32,
        ln: (m.lines.len() - l0) as u32,
    });
}

/// Copies the geometry of `e` from `from` onto the end of `chunk`.
fn copy_entry(chunk: &mut Chunk, from: &MeshBuf, e: &Entry) {
    let m = &mut chunk.mesh;
    let nv = m.vertices.len() as u32;
    let (v0, vn) = (e.v0 as usize, e.vn as usize);
    let (i0, i_n) = (e.i0 as usize, e.i_n as usize);
    let (l0, ln) = (e.l0 as usize, e.ln as usize);
    let entry = Entry {
        v0: nv,
        i0: m.indices.len() as u32,
        l0: m.lines.len() as u32,
        ..*e
    };
    m.vertices.extend_from_slice(&from.vertices[v0..v0 + vn]);
    m.indices
        .extend(from.indices[i0..i0 + i_n].iter().map(|&i| i - e.v0 + nv));
    m.lines.extend_from_slice(&from.lines[l0..l0 + ln]);
    chunk.entries.push(entry);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::Geom;
    use freerouting::geometry::jmath::JavaRandom;

    fn read_scene(name: &str) -> Scene {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/").to_string() + name;
        let bytes = std::fs::read(path).unwrap();
        let result = freerouting::io::specctra::read_board(&bytes);
        Scene::from_board("t", result.board().expect("board"))
    }

    const FIXTURES: [&str; 2] = [
        "Issue159-setonix_2hp-pcb.dsn",
        "Issue690-kit-dev-coldfire-xilinx_5213.dsn",
    ];

    fn assert_same(a: &ChunkSet, b: &ChunkSet) {
        let key = |c: &Chunk| {
            (
                slot_index(c.slot, a.layers),
                kind_index(c.kind),
                c.cell,
                c.lo_id,
            )
        };
        let mut left: Vec<&Chunk> = a.chunks.iter().filter(|c| !c.entries.is_empty()).collect();
        let mut right: Vec<&Chunk> = b.chunks.iter().filter(|c| !c.entries.is_empty()).collect();
        left.sort_by_key(|c| key(c));
        right.sort_by_key(|c| key(c));
        assert_eq!(left.len(), right.len());
        for (x, y) in left.iter().zip(&right) {
            assert_eq!(key(x), key(y));
            assert_eq!(x, y, "chunk {:?}", key(x));
        }
    }

    #[test]
    fn every_item_in_one_chunk() {
        for name in FIXTURES {
            let scene = read_scene(name);
            let set = ChunkSet::build(&scene, scene.bounds.center());
            let total: usize = set.chunks.iter().map(|c| c.entries.len()).sum();
            assert_eq!(total, scene.items.len(), "{name}");
            // Each (id, seq) of the scene once, in a chunk of its slot and
            // kind, whose box holds the item's box.
            let mut seen = BTreeSet::new();
            let mut seq = 0u32;
            let mut expect: BTreeMap<(i32, u32), usize> = BTreeMap::new();
            for (n, item) in scene.items.iter().enumerate() {
                seq = if n > 0 && scene.items[n - 1].id == item.id {
                    seq + 1
                } else {
                    0
                };
                expect.insert((item.id.0, seq), n);
            }
            for (ci, chunk) in set.chunks.iter().enumerate() {
                assert!(chunk.entries.len() <= MAX_CHUNK_ITEMS + 64);
                for e in &chunk.entries {
                    let n = expect[&(e.id, e.seq)];
                    let item = &scene.items[n];
                    assert!(seen.insert((e.id, e.seq)), "{name}: twice");
                    assert_eq!((item.slot, item.kind), (chunk.slot, chunk.kind));
                    let b = item.geom.bounds();
                    assert_eq!(e.bounds, b);
                    assert!(
                        chunk.bounds.min.x <= b.min.x
                            && chunk.bounds.min.y <= b.min.y
                            && chunk.bounds.max.x >= b.max.x
                            && chunk.bounds.max.y >= b.max.y,
                        "{name}: chunk {ci} box misses an item"
                    );
                    assert!(set.owners_of(item.id).contains(&(ci as u32)));
                }
            }
            assert_eq!(seen.len(), scene.items.len(), "{name}");
        }
    }

    #[test]
    fn patch_equals_fresh_build() {
        let scene = read_scene("Issue690-kit-dev-coldfire-xilinx_5213.dsn");
        let origin = scene.bounds.center();
        let mut set = ChunkSet::build(&scene, origin);
        set.take_dirty();
        let mut ids: Vec<ItemId> = scene.items.iter().map(|i| i.id).collect();
        ids.dedup();
        ids.sort_by_key(|i| i.0);
        ids.dedup();
        let mut rng = JavaRandom::new(7);
        let mut picked = BTreeSet::new();
        while picked.len() < 200 {
            picked.insert(ids[rng.next_int(ids.len() as i32) as usize].0);
        }
        let removed: Vec<ItemId> = picked.iter().map(|&i| ItemId(i)).collect();
        let mut added: Vec<Item> = Vec::new();
        let mut run = 0usize;
        let mut sorted_items: Vec<&Item> = scene.items.iter().collect();
        sorted_items.sort_by_key(|i| i.id.0);
        let _ = &mut run;
        for item in sorted_items {
            if picked.contains(&item.id.0) {
                added.push(item.clone());
            }
        }
        // Remove them all: the set must equal a build without them.
        let report = set.patch(&removed, &[]);
        assert!(!report.dirty_chunks.is_empty());
        let without = Scene {
            items: scene
                .items
                .iter()
                .filter(|i| !picked.contains(&i.id.0))
                .cloned()
                .collect(),
            layers: scene.layers.clone(),
            ..Scene::default()
        };
        let removed_only = ChunkSet::build(&without, origin);
        assert_eq!(set.item_count(), removed_only.item_count());
        // Put them back: bit-identical to the fresh build of everything.
        let report = set.patch(&[], &added);
        assert!(report.dirty_bytes > 0);
        assert_eq!(set.stats.full_builds, 1);
        let fresh = ChunkSet::build(&scene, origin);
        assert_same(&set, &fresh);
        // And patching in one go (remove and add together) is the same.
        let mut again = ChunkSet::build(&scene, origin);
        again.patch(&removed, &added);
        assert_same(&again, &fresh);
        assert_eq!(set.triangle_count(), fresh.triangle_count());
    }

    fn square(id: i32, at: f64) -> Item {
        Item {
            id: ItemId(id),
            kind: Kind::Pad,
            slot: Slot::Layer(0),
            geom: Geom::Polygon {
                outer: vec![
                    P::new(at, 0.0),
                    P::new(at + 0.5, 0.0),
                    P::new(at + 0.5, 0.5),
                    P::new(at, 0.5),
                ],
                holes: Vec::new(),
            },
            net: None,
            owner: None,
        }
    }

    fn tiny_scene(n: i32) -> Scene {
        Scene {
            layers: vec![crate::scene::Layer {
                name: "F.Cu".into(),
                signal: true,
            }],
            items: (0..n).map(|i| square(i + 1, f64::from(i))).collect(),
            ..Scene::default()
        }
    }

    #[test]
    fn big_chunks_split_and_stay_consistent() {
        let n = SPLIT_ITEMS as i32 + 100;
        let mut scene = tiny_scene(n - 500);
        let origin = P::new(0.0, 0.0);
        let mut set = ChunkSet::build(&scene, origin);
        // All in one cell? Whatever the grid is, force growth in one chunk by
        // adding items next to item 1.
        let extra: Vec<Item> = (0..2 * SPLIT_ITEMS as i32)
            .map(|i| square(100_000 + i, 0.0))
            .collect();
        let report = set.patch(&[], &extra);
        assert!(report.splits >= 1, "{report:?}");
        scene.items.extend(extra);
        let total: usize = set.chunks.iter().map(|c| c.entries.len()).sum();
        assert_eq!(total, scene.items.len());
        for (ci, c) in set.chunks.iter().enumerate() {
            assert!(c.entries.len() <= SPLIT_ITEMS, "{}", c.entries.len());
            let mut prev = None;
            for e in &c.entries {
                assert!(prev.is_none_or(|p| p <= (e.id, e.seq)));
                prev = Some((e.id, e.seq));
                assert!(set.owners_of(ItemId(e.id)).contains(&(ci as u32)));
                assert!(e.v0 + e.vn <= c.mesh.vertices.len() as u32);
            }
            assert!(
                c.mesh
                    .indices
                    .iter()
                    .all(|&i| (i as usize) < c.mesh.vertices.len())
            );
        }
        // Removing everything again empties the set.
        let all: Vec<ItemId> = scene.items.iter().map(|i| i.id).collect();
        set.patch(&all, &[]);
        assert_eq!(set.item_count(), 0);
        assert_eq!(set.triangle_count(), 0);
    }

    #[test]
    fn grid_median_cell_is_near_target() {
        let scene = tiny_scene(5000);
        let centers: Vec<P> = scene
            .items
            .iter()
            .map(|i| i.geom.bounds().center())
            .collect();
        let grid = Grid::choose(&centers);
        let mut count = BTreeMap::new();
        for &c in &centers {
            *count.entry(grid.cell_of(c)).or_insert(0usize) += 1;
        }
        let mut v: Vec<usize> = count.values().copied().collect();
        v.sort_unstable();
        let m = v[v.len() / 2];
        assert!((300..=2500).contains(&m), "median {m} in {} cells", v.len());
    }
}
