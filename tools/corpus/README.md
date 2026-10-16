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

`owner_boards`: Dan's `canary-pwr` boards, already committed under
`fixtures/canary-pwr/`. They carry the note **graded by canary-09** (the CANary
layout session grades them through Windows KiCad, with `via_in_pad.py = 0` as a
hard requirement), not by this repository's oracle.

The manifest was seeded from KiCadRoutingTools' corpus manifests (candidate data
only: 479 boards, 31 pinned to a commit, 372 with licence "see repo") by
`seed.py`, which resolves each candidate's commit, licence, companion files and
hashes itself. The seed picks per layer count the licensed boards first and
fills with unlicensed ones spread over board size.

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
