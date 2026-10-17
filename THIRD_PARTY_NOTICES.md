# Third-party notices

freerouting-rs is distributed under GPL-3.0 (see `LICENSE`). Release archives
and the KiCad plugin package must include this file and `LICENSES/`.
Per-file provenance is in `docs/PROVENANCE.md`.

## KiCadRoutingTools

- Licence: MIT (`LICENSES/MIT-KiCadRoutingTools.txt`)
- Copyright: Copyright (c) 2026 drandyhaas
- Project: https://github.com/drandyhaas/KiCadRoutingTools
- Licence text recorded from commit `0486e799144050d4e826f71bb31acdc7275af2a2`
- Where its code lives here: files carrying a `Portions ported from` line and
  an SPDX header `GPL-3.0-only AND MIT`; each is listed in
  `docs/PROVENANCE.md`.
- Vendored whole: `crates/krt-grid-router` is KRT's `rust_router/` grid router
  (crate `grid_router` 0.23.0 at `0486e799`) with the Python binding removed.
  It stays MIT (`license = "MIT"`, KRT's `LICENSE` beside the code);
  `crates/krt-grid-router/VENDORED.diff` lists every change against KRT.

## Freerouting (Java)

- Licence: GPL-3.0, the same as this project (it is a derivative work).
- Project: https://github.com/freerouting/freerouting
- Where its code lives here: the whole engine; see `README.md`.
