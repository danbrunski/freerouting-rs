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
  code is ported, so these files are GPL-3.0-only. No stage uses it yet; X05
  adds `krt-grid`.
