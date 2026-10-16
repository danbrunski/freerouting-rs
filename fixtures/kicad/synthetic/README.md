# Synthetic boards (K05.8)

Generated once with pcbnew (KiCad 10.0.7, Ubuntu package, WSL) by
`tools/kicad-reader/synthetic.py`; the exports `*.kicad10.dsn` come from
`tools/kicad-reader/export.sh` on the same KiCad. Not from the Java repository,
so they are not in `MANIFEST.tsv`. The board files hold random UUIDs: re-running
the script gives different files, so they are committed, not regenerated.

| Board | Covers |
|---|---|
| `pads` | one pad of each shape (circle, rect, oval, trapezoid, round rect, chamfered rect, custom with two primitives), rotated 0/30/90 degrees by three footprint rotations, a pad offset, an NPTH hole, a repeated pad number |
| `outline` | a concave notched outline with an arc, and a slot (inner contour) |
| `backside` | back-side parts at 0, 30, 90 and 225 degrees and front parts at 30 and 225 |
| `classes` | project schema 5: a wildcard pattern, an explicit assignment, a regular expression and a net matching two classes |
| `classes6` | project schema 2 (KiCad 6): a class's `nets` list, an explicit assignment |
| `rule-areas` (K06) | 4 layers: 13 board rule areas (tracks and vias, vias only, tracks only, footprints only, pads only, copper pour only, placement only, tracks and footprints, pads and footprints, `*.Cu`, `F&B.Cu`, one with a hole, inner layers), four copper zones (one with a hole, two netless on inner layers), six footprints that own rule areas at 0/30/135 degrees on both sides, and a footprint that owns a copper zone |
| `arcs` (K06) | 2 layers: a segment, an arc and a segment at radii 0.1, 1, 5 and 20 mm with sweeps of 10, 90, 270 and -90 degrees, and a locked segment |
| `vias` (K06) | 6 layers: a through via, blind (from the top and from the bottom), buried and micro vias, a locked via and two vias that share a padstack |
