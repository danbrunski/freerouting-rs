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
  metadata. That is a decision for the owner if K03.2 measures a problem.

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

- [x] **K03.1 — Metadata and icon.**
  - `metadata.json` with the fields in the decisions, `"license":
    "GPL-3.0"`, author and maintainer the repository owner, the repository URL, one
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

  *Done 2026-10-17:* `kicad-plugin/metadata.json` (identifier `com.github.danbrunski.freerouting-rs`, GPL-3.0, author and
  maintainer the repository owner, `swig`, `testing`, `kicad_version` 10.0, `platforms` all three, one entry `0.1.0`, `download_url` =
  `.../releases/download/v0.1.0/freerouting-rs-0.1.0-pcm.zip`) and `kicad-plugin/resources/icon.png` (64 x 64 RGBA, 3742 B,
  a copy of R01.3's `crates/freerouting-gui/icons/icon-64.png`, so no "needs icon" remainder). Deviation from the
  wording: the source file has **no** `download_sha256`/`download_size`/`install_size` placeholders (a fake sha would be
  indistinguishable from a real one); `update_metadata.py` adds them. `install.py`'s `OUR_IDENTIFIERS` now holds the
  identifier. `test_package.py::test_metadata_is_consistent` (version equals `__version__` and `Cargo.toml`, one
  entry, identifier is not the Java one and is in `OUR_IDENTIFIERS`, URL names the asset `package.py` writes) and
  `test_icon_is_64_px_png_within_the_validators_limits` pass; `tools/release/check_version.py` now reports metadata
  and download_url `0.1.0` and `version gate: ok`.

- [x] **K03.2 — Package script.**
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

  *Done 2026-10-17:* `kicad-plugin/package.py` (`--bin DIR|PLATFORM=DIR` repeatable, `--out`, `--allow-missing`; matches the
  release workflow's `--bin bins --out pcm` with `bins/bin-<platform>/`). Layout as decided, plus directory entries
  and, because KiCad's validator allows only `metadata.json`, `resources/icon.png` and `plugins/**`, `LICENSE`,
  `THIRD_PARTY_NOTICES.md` and `LICENSES/` go under `plugins/`. `test_package.py`: `test_zip_is_deterministic` (equal
  bytes and sha256 after touching all inputs), `test_zip_layout` (entry list equals the expected list; binaries 0755,
  the rest 0644; no `__pycache__`, `settings.json`, `tests/`), `test_inner_metadata` (one entry, no
  `download_*`/`install_size`, everything else equal), plus denylist, missing platform/tool, version mismatch,
  two-entry and CLI tests. Zip size with real binaries (Linux CLI 8.7 MB from the committed-tree build 813663cea and GUI
  18.4 MB from 2026-10-15, Windows exes 1.2 + 27.8 MB from 2026-10-15; no macOS binary on this machine,
  `--allow-missing`): **19,558,770 B zip, 56,261,019 B installed**, sha256 `6902b895...60c2`; a macOS universal pair adds
  about 19.6 MB, so about 40 MB for all three (validator download limit 100 MiB). The zlib version affects the bytes,
  so the release zip is built once.

- [x] **K03.3 — Binary resolver.**
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

  *Done 2026-10-17:* `tools.bundle_platform()`, `bundled_bin_dir()` (returns `bin/<os>-<arch>/` only when that folder exists;
  sets the exec bit on Linux/macOS), `missing_router_hint()` (the dialog's unsupported-platform text: which platforms the
  package carries, how to set `bin_dir`), lookup order `bin_dir`, bundle, `FREEROUTING_RS_BIN`, PATH, `~/.cargo/bin`.
  `test_resolver.py` (15 tests): the four cases (win32 AMD64/x86_64, darwin arm64/x86_64, linux x86_64) plus linux
  aarch64, win32 ARM64 and freebsd -> `None`; a file without the exec bit gets it; `test_lookup_order_with_the_bundle`;
  an unpack-and-resolve test on a real built zip. K01.1's `test_lookup_order_each_source_wins_over_later_ones` still
  passes (`test_tools.py`). Real KiCad kept the exec bit on unzip (K03.6), so the chmod is only a fallback.

- [x] **K03.4 — Sidecar and metadata patch.**
  - `package.py` also writes `<zip>.meta.json` with `sha256`,
    `download_size` and `install_size` (the sum of uncompressed sizes).
    `update_metadata.py <metadata.json> <zip.meta.json>` patches those into
    the matching version entry of the repository's `metadata.json`. R01's
    release job runs both and attaches the patched `metadata.json`.
  - **Accept:** `test_update_metadata.py`: the patched sha256 and sizes
    equal those computed from the zip; patching a metadata file whose
    version doesn't match the sidecar fails with a message naming both.

  *Done 2026-10-17:* `package.py` writes `<zip>.meta.json` (`version`, `file`, `sha256`, `download_size`, `install_size` = sum of
  uncompressed file sizes); `kicad-plugin/update_metadata.py <metadata.json> <sidecar> [--out F]` patches the matching entry
  in place and refuses (writing nothing) on no matching version (message names both: "the sidecar is for version 0.1.0 ...
  but metadata.json has version 0.2.0") or a `download_url` that does not end in the sidecar's file name.
  `test_update_metadata.py` (5 tests): patched sha256, size and install size equal values computed from the zip; failure
  messages. Simulated the release job's PCM step in a scratch dir with the workflow's own commands (`package.py --bin
  bins --out pcm`, copy, `update_metadata.py assets/metadata.json pcm/*.zip.meta.json`, `cp pcm/*-pcm.zip assets/`,
  `tools/release/sha256sums.py assets`): works unchanged. The only workflow hunk added (7 lines) is the K03.5 validator.

- [x] **K03.5 — Validate against KiCad's PCM schema.**
  - Clone `gitlab.com/kicad/addons/metadata` into the scratchpad and run
    its `ci/validate.sh` (or the schema check it wraps) on our
    `metadata.json` and on the packaged zip's inner metadata.
  - **Accept:** the validator passes; the command and its output are in the
    done note and summarized in `kicad-plugin.md`. If the validator can't
    run here, the item stays partial with the reason.

  *Done 2026-10-17:* KiCad's real `ci/validate-package.py` (what `ci/validate.sh` runs) from a clone of
  `gitlab.com/kicad/addons/metadata` at `cdd6c04105220261ae5a3cc9432075311ab95a61`, schemas
  `https://go.kicad.org/pcm/schemas/v1` and `v2` (the clone's v1 equals the fetched file). `kicad-plugin/validate_pcm.py
  --zip Z --kicad-metadata <clone> --python <venv>` serves the zip from 127.0.0.1 (nothing is submitted), runs the schema
  checks of the released metadata and the validator, which downloads the zip and checks size, sha256, every member path,
  the icon and the inner `metadata.json` against the outer one. Output for both the fake-binary zip (47 KB) and the
  real-binary zip (19.6 MB): `schema ok: schema-v1.json`, `schema ok: schema-v2.json`, `Validation passed`. Negative check:
  a zip with `LICENSE` at its root fails with `package contains extra file "LICENSE"`. Both are `test_validate_pcm.py`
  (run only with `KICAD_METADATA_REPO` set; skipped otherwise). Summarised in `kicad-plugin.md` "PCM package". The release
  workflow runs it with pinned pip packages and the pinned clone.

- [~] **K03.6 — Install from file on three systems.** *Partly manual.*
  - Windows (owner): KiCad 10 > Plugin and Content Manager > Install from
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
    done without a window on the owner's desktop; otherwise manual, by the owner.
  - **Accept:** the results per system, with the KiCad version and date,
    are in `kicad-plugin.md`. Each system not yet checked keeps the item
    partial with "needs <system> check".

  *Partial 2026-10-17:* **Linux done** with KiCad 10.0.7 (WSL) on a virtual display and a scratch `KICAD_CONFIG_HOME` and
  `KICAD_DOCUMENTS_HOME`: the PCM's Install from File... installed the real-binary zip into
  `3rdparty/plugins/com_github_danbrunski_freerouting-rs/` with the binaries executable (KiCad keeps the zip's 0755); pcbnew
  on the `pic_programmer` demo lists `freerouting-rs` under Tools > External Plugins; the dialog shows `Router:
  .../bin/linux-x86_64/freerouting`, `Router version: 0.1.0 (813663cea)`; Route with the bundled router imported the result
  (115 of 126 connections, 6 vias). Details, and the K01 bug this found (`Path.read_text(newline=)` needs Python 3.13, so
  the rule-area fix-up was skipped on KiCad's 3.12; fixed, `test_dsn_fixup_io.py`), are in `kicad-plugin.md`. Remainder:
  **needs Windows check** (owner: KiCad 10 > Plugin and Content Manager > Install from File with the release zip; remove the
  manual `scripting/plugins/freerouting_rs` copy first; toolbar button, Route, `Router:` shows `plugins/bin/windows-x86_64`)
  and **needs macOS check** (R01's macOS job: quarantine attribute, `freerouting --version` as a subprocess, Gatekeeper and
  Open Anyway). Both need the real release binaries (MSVC build, macOS universal), which exist only after R01's workflow
  runs. Submitting the package to KiCad's repository or publishing the release needs the owner's go-ahead and is not part of this item.
