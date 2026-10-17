# freerouting-krt

Routing stages drawn from [KiCadRoutingTools](https://github.com/drandyhaas/KiCadRoutingTools)
(KRT, MIT, read at `0486e799`) for the freerouting-rs pipeline (decision D9, the
X series in `docs/roadmap/tasks/X00-krt-algorithms-README.md`). Each stage
implements one of the stage traits of `freerouting::autoroute::stages` and is
registered under its id by [`register`](src/lib.rs); a job selects it with the
settings keys of A10 (`router.special.stages` for board stages). With no KRT
stage selected, routing is exactly Freerouting's.

## Licence

- The code written in this crate is **GPL-3.0-only**, like the rest of
  freerouting-rs.
- A file that carries KRT code structure is **GPL-3.0-only AND MIT**: it starts
  with `// SPDX-License-Identifier: GPL-3.0-only AND MIT`, KRT's copyright line
  and the F04 provenance line naming the KRT path, symbol and commit
  (`0486e799`), and has a row in `docs/PROVENANCE.md` (the header convention is
  there; `tools/check-provenance.sh` checks it). KRT's licence text is
  `LICENSES/MIT-KiCadRoutingTools.txt`; see `THIRD_PARTY_NOTICES.md`.
- A file written from a KRT idea alone credits it in one line of its module docs
  and is GPL-3.0-only.

The crate's `license` field is therefore `GPL-3.0-only AND MIT`. KRT's boards
are not in this repository; the test boards in `tests/data/` are synthetic.

## Contents

- `planes` (X02): plane islands, anchors and main components; plane-tap
  verification (a report); pour region repair (`krt-region-repair`). All three
  are written from KRT ideas; no KRT code is ported.
- `grid` (X04): the conservative obstacle raster of one connection for the
  vendored grid A* (`crates/krt-grid-router`, MIT, a dependency of this
  crate): the frame and window, trace and via cells from the board's shapes,
  keepouts, the outline and terminal cells. Written from KRT's design; no KRT
  code is ported, so these files are GPL-3.0-only.
- `grid` (X05): the `krt-grid` connection router
  (`router.autorouter.connection_router = krt-grid`): one connection on the
  raster at 0.1, then 0.05, then 0.025 mm, the path turned into traces and vias
  (`grid/path.rs`, which ports KRT's `simplify_path` and
  `_path_to_segments_vias` and so is GPL-3.0-only AND MIT), exact connectors to
  the pins, and one DRC-checked transaction per attempt; on failure the
  frontier's blocking items. The other X05 files are written from KRT ideas.
- `length` (X07): length and time matching, the `krt-meander` board stage
  (`router.special.length_matching = krt-meander`, groups in
  `router.length_matching.groups`), run after the optimizer: net lengths with
  via barrels and pad-to-die from the KiCad reader's stackup (`length/measure.rs`,
  which ports KRT's barrel and route-length sums), delays per layer from εr
  (`length/timing.rs`, which ports KRT's `impedance.py` delay functions; both
  GPL-3.0-only AND MIT), and trombone bumps on straight runs, each inserted in one
  DRC-checked transaction (`length/meander.rs`, `length/matching.rs`,
  `length/targets.rs`, written from KRT ideas).
- `diffpair` (X08): differential pairs, the `krt-diffpair` board stage
  (`router.special.diff_pairs = krt-diffpair`, name inference behind
  `router.diff_pairs.infer_from_names`), run before the first pass: pairs from
  the KiCad project's diff-pair classes or by name (`diffpair/model.rs` ports
  KRT's `extract_diff_pair_base` and `find_differential_pairs` and so is
  GPL-3.0-only AND MIT), a centreline through Freerouting's maze search at the
  pair's outer width, the P and N tracks offset from it with miter corners, and
  single-ended legs to the pins, all in one DRC-checked transaction, with KRT's
  1, 2 and 4 mm setback fallback (`centreline.rs`, `offset.rs`, `legs.rs`,
  `report.rs`, written from KRT ideas). X09 adds coupled via pairs where the
  pair's ends sit on different layers (`vias.rs`), the crossing-polarity report,
  intra-pair skew compensation with X07's bumps (`router.diff_pairs.skew_um`,
  `skew.rs`) and the set of pair items the batch optimizer leaves alone
  (`optimizer.rs`), all written from KRT ideas.
- `fanout` (X10): the `krt-qfn-escape` fanout stage
  (`router.fanout.algorithm = krt-qfn-escape`): the part frame and the sides of
  QFN, QFP, SOIC and SOP parts at a pitch of at most 0.65 mm
  (`fanout/frame.rs`, which ports KRT's `analyze_qfn_layout` and `analyze_pad`
  and so is GPL-3.0-only AND MIT), straight escape stubs with a taper or a
  plane via, each in one DRC-checked transaction (`fanout/qfn.rs`, written from
  KRT ideas), then Freerouting's fanout on the other parts.
- `bus` (X11): bus detection (`bus/detect.rs`, which ports KRT's
  `detect_bus_groups` and so is GPL-3.0-only AND MIT), the `krt-bus` net
  ordering (`router.autorouter.ordering = krt-bus`: bus groups middle out in
  pass 1, `bus/order.rs`, which ports `get_bus_routing_order`), the
  `krt-bus-attraction` connection router (Freerouting's with a non-negative
  maze-search cost towards the routed neighbour, `bus/attraction.rs`, written
  from a KRT idea) and the `krt-bus` bundle of both
  (`router.autorouter.algorithm = krt-bus`).
