# F04 — Provenance for ported third-party code

Opened 2026-10-15. Decision D9 lets freerouting-rs port code from
KiCadRoutingTools (KRT), which is MIT-licensed, into this GPL-3.0 project. MIT
allows that only with KRT's copyright and permission notice kept. This
checklist sets up the record before the first ported line, so attribution is
never reconstructed afterwards. It covers KRT and any later MIT or
Apache-licensed source.

The Java-derived code needs no new record: the project is GPL-3.0 because it
is a derivative of Freerouting, and its README says so.

Prerequisite: none. It must land before the first X-series checklist that
copies KRT code.

## What exists and what this adds

| Exists | State |
|---|---|
| `LICENSE` | GPL-3.0 |
| `README.md` | Says the project is GPL-3.0 as a derivative of Freerouting |
| KRT | MIT, "Copyright (c) 2026 drandyhaas", at `../KiCadRoutingTools`, commit `0486e799` when the roadmap was written |

## Decisions made here

- **Ideas need credit, code needs notices.** An algorithm reimplemented from
  KRT's documentation is credited in the module docs. Code that is
  translated or copied carries the notice and a provenance row.
- **A vendored crate keeps its own licence.** If KRT's `rust_router` is
  vendored as a crate (X series), it keeps `license = "MIT"` and its own
  `LICENSE` file. The combined binaries are distributed under GPL-3.0.

## File ownership

| Item | New files | May edit |
|---|---|---|
| F04.1 | `THIRD_PARTY_NOTICES.md`, `LICENSES/MIT-KiCadRoutingTools.txt` | `README.md` (one "License" paragraph) |
| F04.2 | `docs/PROVENANCE.md`, `tools/check-provenance.sh` | `.github/workflows/ci.yml` (after F01.2; append a step) |

## Checklist

- [ ] **F04.1 — Notices.**
  - **`THIRD_PARTY_NOTICES.md`:** lists each third-party source with its
    licence, copyright line, project URL and where its code lives here.
  - **`LICENSES/MIT-KiCadRoutingTools.txt`:** holds KRT's full MIT text
    ("Copyright (c) 2026 drandyhaas").
  - **`README.md`:** the "License" section mentions both.
  - **Release archives** (R series) and the KiCad plugin's package include
    `THIRD_PARTY_NOTICES.md`.
  - **Accept:**
    - The files exist with the exact text of KRT's `LICENSE` at the
      recorded commit.
    - R01's packaging test lists `THIRD_PARTY_NOTICES.md` in each archive
      (partial until R01 lands).

- [ ] **F04.2 — A provenance ledger and a check.**
  - **`docs/PROVENANCE.md`:** one row per port: the Rust file and symbol, the
    KRT path, symbol and commit, the port date, and "translated" or
    "adapted".
  - **Ported files** carry SPDX headers:
    - `SPDX-License-Identifier: GPL-3.0-only AND MIT`
    - `SPDX-FileCopyrightText: 2026 drandyhaas (KiCadRoutingTools)`
  - **`tools/check-provenance.sh`** fails when a file carries a
    `Portions ported from` line but has no SPDX header or ledger row. It also
    fails when a ledger row names a file that doesn't exist. CI runs it.
  - **Accept:**
    - The check fails on a test fixture with a `Portions ported from` line
      and no header.
    - It passes on the tree.
    - The CI step is added (partial until F01.2's workflow is pushed).
