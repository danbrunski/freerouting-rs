# Corpus of open KiCad boards

About 30 open-source `.kicad_pcb` boards (2, 4 and 6 layers; easy to hard;
18 with inner-layer planes) for measuring the router on boards nobody tuned it
for. The boards are **not in the repository**: only the manifest is.

```sh
tools/corpus/fetch.sh            # $FR_CORPUS_DIR/src/<id>/  (default: <repo>/target/corpus)
tools/corpus/prep.sh             # $FR_CORPUS_DIR/prepped/<id>.dsn, <id>.human.dsn, grade-set.txt
freerouting-dev bench --set tools/bench/sets/kicad-corpus.txt --java <jar> --out <dir>
tools/kicad-oracle/grade-set.sh target/corpus/grade-set.txt <out> -- <engine command>
freerouting-dev failed-vs-human target/corpus/prepped/<id>.dsn <engine.ses> target/corpus/prepped/<id>.human.dsn
```

`FR_CORPUS_DIR` defaults to `target/corpus` of the repository (untracked), where
`tools/bench/sets/kicad-corpus.txt` expects the prepared boards. To keep the
corpus elsewhere, set `FR_CORPUS_DIR` and symlink `target/corpus` to it.

## `manifest.json`

`boards`: one entry per board.

| Field | Meaning |
|---|---|
| `id` | directory and file stem under `$FR_CORPUS_DIR` |
| `repo`, `commit`, `path` | GitHub repository, the **40-hex commit** the files are fetched at (never a branch) and the `.kicad_pcb` path in it |
| `sha256` | of the `.kicad_pcb`; `companions` has the `.kicad_pro`/`.kicad_dru` hashes (the board's own rules; `kicad-cli` and the DSN export need them) |
| `license` | the SPDX id read from the repo **at that commit** (`license_source` says how: GitHub's license API, or read by hand from the licence file for the five repos GitHub calls "other"), or `LicenseRef-none` when the repo has none |
| `attribution` | owner, URL, commit and date |
| `layers`, `tier`, `has_planes`, `has_zones`, `footprints`, `human_routed_items` | `tier` is easy (up to 30 footprints), medium (up to 80) or hard; `has_planes` is a zone on an inner layer; `human_routed_items` counts tracks and vias of the original (two boards are unrouted) |
| `committable` | the licence is in `allowed-licenses.txt` |

`owner_boards`: the owner's `canary-pwr` boards, already committed under
`fixtures/canary-pwr/`. They carry the note **graded by canary-09** (the CANary
layout session grades them through Windows KiCad, with `via_in_pad.py = 0` as a
hard requirement), not by this repository's oracle.

The manifest was seeded from KiCadRoutingTools' corpus manifests (candidate data
only: 479 boards, 31 pinned to a commit, 372 with licence "see repo") by
`seed.py`, which resolves each candidate's commit, licence, companion files and
hashes itself. The seed picks per layer count the licensed boards first and
fills with unlicensed ones spread over board size.

## `pcbworld-d3c.json`: PCBWorld-Bench D3-C

A second manifest, in the same format, kept apart so the 30-board corpus, its bench set and its
baselines do not change:

```sh
FR_CORPUS_DIR=target/corpus-d3c tools/corpus/fetch.sh --manifest tools/corpus/pcbworld-d3c.json
FR_CORPUS_DIR=target/corpus-d3c tools/corpus/prep.sh
freerouting-dev bench --set tools/bench/sets/pcbworld-d3c.txt --rust target/release/freerouting --out <dir>
```

Eleven boards (`left_out` names the twelfth chosen):
- PCBWorld's D3-C test split: nine two-layer boards of 84–206 connections;
- `d3c-0581-a123-battery-integration-bcm`: a four-layer battery power board with inner planes;
- `d3c-0677-kitspace-glasgow`: 912 connections on two signal layers with inner planes, and an
  11 × 11, 0.8 mm BGA (U30). Pass 1 alone takes about 12 minutes on it: fanout about 6 and
  routing about 6. The other ten take under 5 seconds each.

D3-C is the `hard` difficulty (more than 100 pins) of [PCBWorld-Bench](https://github.com/LGAI-Research/PCBWorld)
(arXiv 2607.05915, LG AI Research): `configs/datasets/d3.json` at `b3d62f5c37`. Its boards are
[PCBench](https://github.com/PCBench/PCBench)'s (He et al., 2024; `dec3be75cb`). PCBench's MIT
licence covers its own files, not the boards it collected, and neither project redistributes a
board under the board's licence. So `seed_pcbworld.py` pins every board at its **original**
repository:
- the commit is one whose tree holds PCBench's exact `raw.kicad_pcb` (the same git blob,
  recorded as `pcbench_blob`): the commit at PCBench's retrieval date, else the default branch's
  head, else the newest commit `git log --find-object` names;
- the licence is read there (GitHub's license API at that commit), exactly as for `manifest.json`.

Extra fields per entry:
- `pcbworld_id`: PCBWorld's `<NNNN>_<name>`, where NNNN is its difficulty order;
- `pcbworld_split`: `hard.test` for PCBWorld's 10-board D3-C test split, `hard.train` otherwise;
- `pcbench_blob`;
- `ball_arrays`: the footprints that pass X12's array test (`fanout/bga/array.rs`: one pitch on
  both axes, at least 6 × 6 positions, not staggered, both outer rings at least half populated),
  computed on footprint-local SMD pad centres.

`left_out` lists the boards that could not be pinned or licensed, and why.

PCBWorld converts each board to KiCad 9 and patches its rules (`drc_fix_v9.py`) before
measuring. This corpus does neither: `prep.sh` grades every board at its own rules (D8).
PCBWorld's published scores are therefore not comparable with numbers from this set.

## Licences

A board may be committed only if its licence is in `allowed-licenses.txt`
(permissive licences and CC0). `crates/freerouting-dev/tests/corpus.rs` refuses
any committed file whose content is a manifest board with another licence, and
checks that `committable` agrees with the list. Boards without a licence
(`LicenseRef-none`) and copyleft ones are fetched for measurement, never
redistributed.

## Prep

`prep.sh` strips tracks and vias but **keeps zones and layer types**
(KiCadRoutingTools' prep drops both), exports the DSN through the KiCad oracle's
steps (F02.6: `kicad_steps.py strip`, `export`, `oracle_lib.py fix-keepouts`),
and rewrites the DSN's first line to `(pcb <id>` so the file does not depend on
the directory. It also exports the human-routed original with its tracks kept
(`<id>.human.dsn`) for `failed-vs-human`. Prep is deterministic for one KiCad
version; the version is in `<id>.json`. Never mix KiCad versions in one
comparison.

## Untrusted data

The fetched boards are untrusted: the scripts only download, hash and parse
them (KiCad loads them as data); nothing from a board is executed. Run any
Python that reads them with `-I`.
