# K03 — KiCad plugin manager (PCM) package

Opened 2026-10-15. Today the plugin is installed by running `install.py`
and pointing `settings.json` at a build folder. Users of a release need one
file that KiCad's Plugin and Content Manager installs, with the executables
inside. KRT ships exactly that: one zip per version holding every
platform's binary, plus a resolver that picks the right one at startup
(KRT report §8). This checklist builds the same for freerouting-rs. It is
the report's PLG-1.5, extended with KRT's packaging lessons.

**Prerequisites:** K01 (tests, `version.py`, install guards); R01 (release
binaries for Windows, macOS universal and Linux). K03.1–K03.3 can be built
and tested before R01 with local binaries.

## What exists and what this adds

| Exists | State |
|---|---|
| `kicad-plugin/` | `freerouting_rs/{__init__,plugin,tools}.py`, `icon.png`, `install.py`. No `metadata.json`, no package script |
| Binary sizes (release, 2026-10-15) | Linux: `freerouting` 0.44 MB, `freerouting-gui` 18.4 MB; Windows (gnu): `freerouting.exe` 1.2 MB, `freerouting-gui.exe` 27.8 MB. A macOS universal GUI binary will be about twice a single-arch one |
| Java plugin metadata at `cda540f` | `integrations/KiCad/kicad-freerouting/metadata.json`: schema `https://go.kicad.org/pcm/schemas/v1`, identifier `app.freerouting.kicad-plugin`, `"license": "GPL-3.0"`, one version entry with `"runtime": "ipc"` |
| KRT | `package_pcm.py` (one zip, `ALL_BINARIES`, denylist `ROOT_EXCLUDE`/`IGNORE_PATTERNS`; an allowlist once dropped a new package, KRT #49), `update_metadata.py` (sha256, `download_size`, `install_size` patched in from a sidecar), root `__init__.py` `_resolve_rust_binary()` (`sys.platform` + `platform.machine()`), `check_release_version.py`, `docs/release-pipeline.md` ("Common pitfalls": the PCM validator rejects two `versions[]` entries with the same version string even for different platforms; the regex rejects 4-part versions) |
| KiCad's validator | `ci/validate.sh` in `gitlab.com/kicad/addons/metadata` (per KRT's release docs; not run here yet) |

What this adds: `metadata.json`, a deterministic package script, a binary
resolver in `tools.py`, the sidecar and metadata patch R01 uses, validation,
and install checks on the three operating systems.

## Decisions made here

- **One zip per version, all platforms inside**, because the PCM validator
  rejects per-platform entries with the same version (KRT). Layout:
  `metadata.json`, `resources/icon.png` (64 × 64), `plugins/` (the
  package) and `plugins/bin/<os>-<arch>/` with `freerouting` and
  `freerouting-gui` for `windows-x86_64`, `macos-universal` and
  `linux-x86_64`.
- **Identifier `com.github.danbrunski.freerouting-rs`**, distinct from the
  Java plugin's `app.freerouting.kicad-plugin` (K00 question 4).
  `"runtime": "swig"`, `"status": "testing"`, `kicad_version` the lowest
  version actually tested (10.0 until K01.5 checks 9.0).
- **Deterministic zip:** entries sorted by path, fixed timestamps
  (1980-01-01), fixed permissions (0755 for executables, 0644 otherwise).
  Two runs on the same inputs give byte-identical zips, so the sha256 in
  the metadata is reproducible.
- **Files go in by denylist**, not allowlist (KRT #49).
- **The resolver sits after `settings.json`'s `bin_dir`** and before
  `FREEROUTING_RS_BIN`, PATH and `~/.cargo/bin`, so a developer's explicit
  `bin_dir` still wins over the bundled binaries.
- **If the zip is too large for PCM** (the limit is not known here), the
  fallback is a Python-only package that downloads the platform's binaries
  from the GitHub release on first use, checked against the sha256 in its
  metadata. That is a decision for Dan if K03.2 measures a problem.

## File ownership

| Item | New files | May edit |
|---|---|---|
| K03.1 | `kicad-plugin/metadata.json`, `kicad-plugin/resources/icon.png` (rendered from R01.3's SVG) | `kicad-plugin/freerouting_rs/version.py` |
| K03.2 | `kicad-plugin/package.py`, `kicad-plugin/tests/test_package.py` | — |
| K03.3 | `kicad-plugin/tests/test_resolver.py` | `kicad-plugin/freerouting_rs/tools.py` |
| K03.4 | `kicad-plugin/update_metadata.py`, `kicad-plugin/tests/test_update_metadata.py` | — |
| K03.5 | — | `docs/development/kicad-plugin.md` |
| K03.6 | — | `docs/development/kicad-plugin.md` |

## Checklist

- [ ] **K03.1 — Metadata and icon.**
  - `metadata.json` with the fields in the decisions, `"license":
    "GPL-3.0"`, author and maintainer Dan, the repository URL, one
    `versions[]` entry whose `version` equals `version.__version__`
    (three parts) and whose `download_url` names the GitHub release asset.
    `download_sha256`, `download_size` and `install_size` are placeholders
    that K03.4 fills.
  - `resources/icon.png`: 64 × 64, rendered from R01.3's icon source
    (`crates/freerouting-gui/icons/icon.svg`). Today's
    `freerouting_rs/icon.png` is only 24 × 24, too small to scale up; until
    R01.3 lands, the item is partial with "needs icon".
  - **Accept:** `test_package.py::metadata_is_consistent`: the JSON parses,
    has exactly one version entry, its version equals `__version__` and the
    workspace version in `Cargo.toml`, and the identifier is not
    `app.freerouting.kicad-plugin`.

- [ ] **K03.2 — Package script.**
  - `package.py --bin <dir per platform> --out <dir>` builds
    `freerouting-rs-<version>-pcm.zip` with the layout in the decisions.
    The inner `metadata.json` has one version entry and no `download_*` or
    `install_size` fields (the PCM requires that, per KRT). It includes
    `THIRD_PARTY_NOTICES.md` and `LICENSE` once F04.1 has made them.
  - **Accept:** `test_package.py`:
    - `zip_is_deterministic`: two builds from the same inputs have equal
      sha256;
    - `zip_layout`: the entry list equals the expected list for fake
      binaries, executables have mode 0755, and no `__pycache__`,
      `settings.json` or `tests/` entry is present;
    - `inner_metadata`: one version entry, no `download_*` or
      `install_size`;
    - the done note records the zip size with real release binaries (from
      R01, or local builds for the platforms available).

- [ ] **K03.3 — Binary resolver.**
  - `tools.bundled_bin_dir()` returns `plugins/bin/<os>-<arch>/` for
    `sys.platform` and `platform.machine()` (`win32`+`AMD64`/`x86_64`,
    `darwin`+any arch → `macos-universal`, `linux`+`x86_64`), or `None`.
    On Linux and macOS it sets the executable bit if it is missing (the
    PCM unzip may drop it; unverified).
  - `find_tool` checks it after `bin_dir`. On an unsupported platform the
    dialog says which platforms the package carries and how to point
    `settings.json` at a local build.
  - **Accept:** `test_resolver.py` with monkeypatched `sys.platform` and
    `platform.machine()` for the four cases and an unsupported one
    (`linux`+`aarch64`); a file without the exec bit gets it; the lookup
    order test from K01.1 still passes with the new step.

- [ ] **K03.4 — Sidecar and metadata patch.**
  - `package.py` also writes `<zip>.meta.json` with `sha256`,
    `download_size` and `install_size` (the sum of uncompressed sizes).
    `update_metadata.py <metadata.json> <zip.meta.json>` patches those into
    the matching version entry of the repository's `metadata.json`. R01's
    release job runs both and attaches the patched `metadata.json`.
  - **Accept:** `test_update_metadata.py`: the patched sha256 and sizes
    equal those computed from the zip; patching a metadata file whose
    version doesn't match the sidecar fails with a message naming both.

- [ ] **K03.5 — Validate against KiCad's PCM schema.**
  - Clone `gitlab.com/kicad/addons/metadata` into the scratchpad and run
    its `ci/validate.sh` (or the schema check it wraps) on our
    `metadata.json` and on the packaged zip's inner metadata.
  - **Accept:** the validator passes; the command and its output are in the
    done note and summarized in `kicad-plugin.md`. If the validator can't
    run here, the item stays partial with the reason.

- [ ] **K03.6 — Install from file on three systems.** *Partly manual.*
  - Windows (Dan): KiCad 10 > Plugin and Content Manager > Install from
    File with the zip; the toolbar button appears; Route on a demo board
    imports a SES using the bundled binaries (the dialog's "Router:" line
    shows the `plugins/bin/windows-x86_64` path). Remove the manual
    `scripting/plugins/freerouting_rs` copy first, or K01.4's guard
    reports the shadow.
  - macOS (CI runner, R01's macOS job): set the quarantine attribute on the
    unpacked binary (`xattr -w com.apple.quarantine …`) and run
    `freerouting --version` the way the plugin does (a subprocess); record
    whether Gatekeeper blocks it and what "Open Anyway" changes.
  - Linux: the same install through the WSL KiCad 10.0.7 only if it can be
    done without a window on Dan's desktop; otherwise manual, by Dan.
  - **Accept:** the results per system, with the KiCad version and date,
    are in `kicad-plugin.md`. Each system not yet checked keeps the item
    partial with "needs <system> check".
