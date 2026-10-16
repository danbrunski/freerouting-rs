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
