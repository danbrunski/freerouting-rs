# R01 — Release builds for Windows, macOS and Linux

Opened 2026-10-15. "Complete" includes release builds for Windows, macOS
(universal) and Linux (item 7 in [port-completion.md](../../port-completion.md)).
Today there are none: Dan builds the Windows exes by hand with the gnu
toolchain, and nothing is versioned beyond `0.1.0` in `Cargo.toml`. This
checklist adds one tag-triggered workflow that builds, packages, checks and
drafts a GitHub release, plus the version gate and version stamp it needs.
It is the GUI report's REL-1 with KRT's "version gate first" amendment
(KRT report, "R and C — Release and settings"). CI itself (fmt, clippy,
tests on three systems) is F01.2; signing is [R02](../R02-signing.md).

**Prerequisites:**
- F01.2: CI installed, with the action versions it pins (reuse them here).
- F04.1: `THIRD_PARTY_NOTICES.md` and `LICENSES/`, which every archive
  carries.
- C08: `freerouting --version`, which R01.2's stamp feeds.
- K01.3 and K03 (plugin `version.py`, `metadata.json`, the PCM zip) for
  R01.1's plugin checks and R01.7's PCM asset; until they exist those parts
  are skipped with a note.
- Pushing workflows and tags needs Dan's credentials (the local `gh` token
  lacks the `workflow` scope; GUI report §0 item 10). The implementer
  prepares files; the orchestrator pushes.

R01.1–R01.6 can be built and tested on today's binaries (viewer and DRC
CLI). The first public release waits for the CLI to route (R00).

## What exists and what this adds

| Exists (at `70389b7`) | State |
|---|---|
| Workspace `Cargo.toml` | `version = "0.1.0"`, `license = "GPL-3.0-only"`, `rust-version = "1.85"`, members `crates/*`; no `authors`, no `description` except on `freerouting` |
| `crates/freerouting-gui/src/main.rs` | `#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]` hides the console in release |
| Binaries | `freerouting` (CLI, `crates/freerouting-cli`) and `freerouting-gui`. Release sizes on 2026-10-15: Linux 0.44 MB and 18.4 MB; Windows gnu 1.2 MB and 27.8 MB. The Windows exes import only system DLLs (`kicad-plugin.md`) |
| `docs/development/ci-workflow.yml` | The CI draft F01.2 installs |
| Icons | only `kicad-plugin/freerouting_rs/icon.png`, 24 × 24 |
| Tool facts (GUI report §7, research of 2026-10; versions not re-checked here) | cargo-packager 0.11.8 makes MSI, DMG and AppImage from one config; cargo-dist 0.33 makes no DMG, no universal macOS build and no AppImage; cargo-wix 0.3.9 uses WiX v3, preinstalled (3.14.1) on `windows-latest`. Under Xvfb, `WGPU_BACKEND=gl` panicked with "Invalid surface"; `vulkan` (lavapipe) works |
| KRT | `check_release_version.py` (fails a tag in seconds when `VERSION`, `metadata.json` and the tag disagree; added after a release failed 10 minutes in), `.github/workflows/release.yml` (validate, build, package, release), macOS x86_64 cross-compiled from `macos-14` because `macos-13` runners queue for 30+ minutes |

What this adds: a version gate, a build stamp, cargo-packager config and an
icon set, `.github/workflows/release.yml` with Linux, Windows and macOS
jobs, checksums and a draft release, and `docs/development/release.md`.

## Decisions made here

- **cargo-packager 0.11.8, pinned** (`cargo install cargo-packager --version
  0.11.8 --locked`), with native fallbacks if it fails on a platform:
  `appimagetool`, `cargo-wix`, `hdiutil`. Its last crates.io release was 11
  months before this plan, so the pin and the fallbacks both matter.
- **Linux builds on `ubuntu-22.04`** (glibc 2.35 floor) and bundles no
  system library: X11, Wayland, xkbcommon, Vulkan and EGL come from the
  host at run time.
- **Windows release builds use MSVC with a static C runtime**
  (`-C target-feature=+crt-static`), so users need no Visual C++
  redistributable. Dan's local KiCad loop keeps the gnu toolchain.
- **macOS ships one universal build**: `aarch64-apple-darwin` and
  `x86_64-apple-darwin` (cross-compiled on the arm64 runner), joined with
  `lipo -create`. Ad-hoc signature only until R02.
- **The version gate runs first** in the release workflow and also in CI,
  so a mismatch fails in seconds, not after the builds.
- **The build stamp lives in the engine crate** (`freerouting::build_info`)
  so the CLI's `--version` (C08) and the GUI's About (G04.8) print the same
  thing. It never changes behaviour.
- **One original icon**, drawn as SVG and rendered to every size. It must
  not imitate the Java Freerouting logo (R00 question 3).

## File ownership

| Item | New files | May edit |
|---|---|---|
| R01.1 | `tools/release/check_version.py`, `tools/release/test_check_version.py` | `.github/workflows/ci.yml` (append a step, after F01.2) |
| R01.2 | `crates/freerouting/build.rs`, `crates/freerouting/src/build_info.rs` | `crates/freerouting/src/lib.rs` (one `pub mod`), `crates/freerouting-cli/src/main.rs` (version line only, if C08 has landed) |
| R01.3 | `crates/freerouting-gui/icons/**`, `Packager.toml` or `[package.metadata.packager]` | `Cargo.toml` (workspace `authors`, `description`), `crates/freerouting-gui/Cargo.toml` |
| R01.4 | `.github/workflows/release.yml` (Linux job) | — |
| R01.5 | `crates/freerouting-gui/build.rs`, `crates/freerouting-cli/build.rs` (Windows resources) | `.github/workflows/release.yml`, the two crates' `Cargo.toml` (`winresource` build-dependency) |
| R01.6 | — | `.github/workflows/release.yml` |
| R01.7 | — | `.github/workflows/release.yml` |
| R01.8 | `docs/development/release.md` | `README.md` (an "Install" section) |

## Checklist

- [x] **R01.1 — Version gate.**
  - `check_version.py [--tag vX.Y.Z]` reads the workspace version from
    `Cargo.toml`, `kicad-plugin/freerouting_rs/version.py` (K01.3),
    `kicad-plugin/metadata.json`'s single version entry and the suffix of
    its `download_url` (K03.1), and the tag when given. It prints each and
    exits 1 on any mismatch, or on a version the PCM regex rejects (four
    parts). Missing plugin files are reported as "not yet present" and
    skipped.
  - The release workflow's first job runs it with the tag; CI runs it
    without.
  - **Accept:** `python3 -m pytest tools/release`: all equal passes; each
    single mismatch (workspace, plugin, metadata version, `download_url`,
    tag) fails and names the file; `0.1.0.1` fails; a planted mismatch in a
    temp copy of the repo fails the gate (negative control).

  *Done 2026-10-17:* `tools/release/check_version.py [--tag vX.Y.Z] [--root DIR]` reads the
  workspace version, `Packager.toml`'s version (added: the MSI/DMG carry it), `version.py`,
  `metadata.json`'s single entry and its `download_url` (both "not yet present" until K03), and the tag;
  exits 1 naming the file on any difference or a non-`MAJOR.MINOR.PATCH` version. Tests
  `tools/release/test_check_version.py` (plain `unittest`, so `python3 -m pytest tools/release`
  collects them too; pytest is not installed on this machine, so they ran as
  `python3 -m unittest discover -s tools/release -p 'test_*.py'`: 29 tests in total across the five test files, OK):
  all equal passes; each single mismatch (workspace, packager, plugin, metadata, download_url, tag) fails and
  names its file (`test_each_single_mismatch_fails_and_names_the_file`); `0.1.0.1` and `0.1.0-rc1` fail;
  negative control `test_planted_mismatch_in_a_copy_of_the_repository_fails` copies the real `Cargo.toml`,
  `Packager.toml` and plugin package into a temp dir, plants 9.9.9 in `version.py` and the script exits 1.
  `python3 tools/release/check_version.py --tag v0.1.0` on the repo: ok. The CI step is the `version-gate`
  job appended to `docs/development/ci-workflow.yml` (CI is disabled by Dan, so no `.github/`; a green
  GitHub run is not claimed); the release workflow draft's first job (`gate`) runs it with the tag.

- [x] **R01.2 — Build stamp.**
  - `crates/freerouting/build.rs` sets `FREEROUTING_GIT_SHA` from the
    environment variable of the same name if CI sets it, else from
    `git rev-parse --short=12 HEAD`, else `unknown`; it reruns when
    `.git/HEAD` or the ref it points to changes.
    `freerouting::build_info::{VERSION, GIT_SHA}` expose it.
  - C08's `--version` prints `freerouting <VERSION> (<GIT_SHA>)`; G04.8's
    About shows both. Whichever lands later wires it.
  - **Accept:** a unit test checks `VERSION == env!("CARGO_PKG_VERSION")`
    and that `GIT_SHA` is 12 hex digits or `unknown`; building from a
    source tarball without `.git` gives `unknown` and does not fail.

  *Done 2026-10-17:* `crates/freerouting/build.rs` (`FREEROUTING_GIT_SHA` if >= 12 hex digits, truncated
  to 12; else `git rev-parse --short=12 HEAD` only when the root has its own `.git`; else `unknown`;
  reruns on `.git/HEAD` and its ref, worktrees included; debug-profile builds do not watch HEAD so a commit
  does not recompile the engine) and `crates/freerouting/src/build_info.rs` (`VERSION`, `GIT_SHA`,
  `version_with_sha()`; `pub mod build_info` in `lib.rs`). C08 had already landed a CLI-local `build.rs`
  (9 digits): reused, not duplicated: it is deleted and `freerouting-cli/src/version.rs` now re-exports
  `freerouting::build_info` (the `--version` line is `freerouting 0.1.0 (git 813663cea882)`, 12 digits
  instead of 9; `tests/version.rs` and the unit test in `version.rs` updated). Tests:
  `cargo test -p freerouting --lib build_info` 4 passed (VERSION == CARGO_PKG_VERSION, GIT_SHA is 12 hex or
  `unknown`); `tools/release/test_build_stamp.py` compiles `build.rs` on its own and checks: no `.git` ->
  `unknown`, a tarball unpacked inside another repo -> `unknown`, a checkout -> HEAD[:12], CI override
  truncated, bad override ignored, a linked worktree, the debug profile. A real tarball copy (no `.git`,
  workspace crates only) builds and `cargo test --locked -p freerouting --lib build_info` passes there.
  `cargo test -p freerouting-cli`: all pass. Behaviour: the stamp is read only by `build_info`, the CLI's
  `--version`/`--capabilities`/manifest; the SES of `Issue159-setonix_2hp-pcb.dsn` (`-mp 2 -mt 1`) from
  two release CLIs stamped `813663cea882` and `000000000000` is byte-identical (`cmp`).
  G04.8's About does not exist yet; it should print `freerouting::build_info::version_with_sha()`.

- [~] **R01.3 — Packager config and icons.**
  - An original icon: `crates/freerouting-gui/icons/icon.svg`, rendered to
    PNG at 16–1024 px, `icon.ico` and `icon.icns` (the commands that render
    them are in `icons/README.md`). It also feeds K03.1's 64 px PNG.
  - cargo-packager config: product name `freerouting-rs`, identifier
    `com.github.danbrunski.freerouting-rs`, both binaries (`freerouting-gui`
    as the main one), icons, `LICENSE`, `THIRD_PARTY_NOTICES.md`; workspace
    `authors` (the MSI needs it) and `description`.
  - **Accept:** `cargo packager --release -f appimage` on this machine
    produces an AppImage; under `xvfb-run -a` with `WGPU_BACKEND=vulkan`
    and `APPIMAGE_EXTRACT_AND_RUN=1` it runs
    `--screenshot shot.png --exit fixtures/Issue159-setonix_2hp-pcb.dsn`
    (G01.5) and the implementer opens the PNG and describes it. Until
    G01.5 lands, `--version` is the check. Dan approves the icon (R00
    question 3).

  *Partial 2026-10-17:* done: `crates/freerouting-gui/icons/icon.svg` (original: two routed nets with
  45-degree bends on a dark green board; plain shapes), `render.py` + `README.md` (ImageMagick `convert` draws
  the SVG once at 1024 px, Pillow scales and writes `icon-{16,32,48,64,128,256,512,1024}.png`, `icon.ico`
  with 7 sizes, `icon.icns` written by hand; deterministic, a re-run changes no file; 270 KB in total);
  root `Packager.toml` (product `freerouting-rs`, identifier `com.github.danbrunski.freerouting-rs`, both
  binaries with the GUI as `main`, icons, `LICENSE`, `THIRD_PARTY_NOTICES.md`, `LICENSES/`; workspace
  `authors` and `description` added to `Cargo.toml` and inherited by the GUI and CLI crates). cargo-packager
  0.11.8 installed into a private root; with the config it builds a `.deb` correctly here and parses
  everything (two 0.11.8 quirks recorded in `Packager.toml` and `release.md`: `-c` needs an absolute path,
  `name` must be set). Remaining: (1) **the AppImage accept clause is not met as written**: `cargo packager
  -f appimage` produced an AppImage whose GUI binary hung before the first frame (linuxdeploy rewrote it to
  18 MB from 32 MB; plain `strip`, `patchelf` and `NO_STRIP=1` did not explain or avoid it; cause not
  found). The checklist's fallback applies: `tools/release/make_appimage.sh` (appimagetool, hash-pinned,
  own AppDir with a shell `AppRun`) produces `freerouting-rs-0.1.0-linux-x86_64.AppImage` (14.5 MB) and
  under `xvfb-run -a` with `WGPU_BACKEND=vulkan` and `APPIMAGE_EXTRACT_AND_RUN=1`
  `AppImage --screenshot shot.png --exit fixtures/Issue159-setonix_2hp-pcb.dsn` exits 0 in 0.19 s and writes a
  98,952-byte PNG (identical size to the unpackaged binary's). I opened it: the viewer window, 1280x800,
  menu bar (File..Help), a toolbar (Autoroute, Inspect, Route, Drag, Ratsnest, Violations, Undo, Redo,
  mm), a layer panel (F.Cu red, B.Cu blue), the board panel (9.6 x 108.5 mm, 15 components, 9 nets) and the
  long narrow board with LED, switch and jack outlines, yellow ratsnest lines, "Incomplete connections: 23". The
  icon is not visible in that shot (the window does not set it yet). (2) **Dan approves the icon** (R00
  question 3); the GUI does not yet use `icon-*.png` as its window icon (G-series). (3) K03.1's 64 px PNG:
  `icons/icon-64.png` is ready to copy.

- [~] **R01.4 — Linux release job.**
  - `release.yml` on tags `v*`, job `linux` on `ubuntu-22.04`: release
    build; `freerouting-rs-<ver>-linux-x86_64.tar.gz` with both binaries,
    `kicad-plugin/`, `README.md`, `LICENSE`, `THIRD_PARTY_NOTICES.md`; the
    AppImage of the GUI. `permissions: contents: read` for build jobs.
  - **Accept:**
    - The job is green on a test tag (pushed by the orchestrator, on Dan's
      repository or a fork).
    - A follow-up job on `ubuntu-24.04` runs the AppImage under `xvfb-run`
      with lavapipe and uploads the `--screenshot` PNG; it is non-empty.
    - In an `ubuntu:22.04` container, `freerouting --version` runs.
    - The highest `GLIBC_` symbol version in either binary
      (`objdump -T … | grep -o 'GLIBC_[0-9.]*' | sort -V | tail -1`) is at
      most 2.35.
    - The tar listing contains `THIRD_PARTY_NOTICES.md` (F04.1's check).

  *Partial 2026-10-17:* needs a GitHub run, "CI disabled by Dan": no `.github/`; the workflow is the
  draft `docs/development/release-workflow.yml` (jobs `gate`, `linux`, `linux-smoke`; tags `v*`,
  `permissions: contents: read`; pins in `release.md`; YAML parses). Local evidence on this Ubuntu 24.04
  box with the local release build: `tools/release/package_archive.py --platform linux-x86_64` makes
  `freerouting-rs-0.1.0-linux-x86_64.tar.gz` (14.8 MB, reproducible: `test_archive_is_reproducible`) with
  both binaries, `kicad-plugin/`, `README.md`, `LICENSE`, `THIRD_PARTY_NOTICES.md`, `LICENSES/`;
  `tar tzf | grep THIRD_PARTY_NOTICES.md` -> 1. AppImage as in R01.3. `freerouting --version` runs in an
  `ubuntu:22.04` container (`docker run`: `freerouting 0.1.0 (git 813663cea882)`). The glibc check
  (`tools/release/glibc_floor.sh 2.35 ...`): CLI highest `GLIBC_2.35` (ok); GUI `GLIBC_2.39` -> the script
  fails, as designed, because this box builds on glibc 2.39; whether the GUI stays <= 2.35 when built on
  `ubuntu-22.04` is unverified. Remaining (needs the test tag of R01.7 and Dan's push; screenshot flag from G01.5): a green run on a test tag, the `ubuntu-24.04` smoke job with the
  uploaded screenshot, the 22.04 glibc number.

- [~] **R01.5 — Windows release job.**
  - Job `windows` on `windows-latest`, target `x86_64-pc-windows-msvc`,
    static CRT. Icon and version resource for both exes through
    `winresource` (`NEW DEP`, build-dependency, Windows only).
    `freerouting-rs-<ver>-windows-x86_64.zip` (both exes, `kicad-plugin\`,
    README, LICENSE, notices) and an MSI through cargo-packager `-f wix`
    (fallback cargo-wix).
  - **Accept:** on the runner:
    - `msiexec /i <msi> /qn` installs; `"<install dir>\freerouting.exe"
      --version` prints the stamped version; `msiexec /x <msi> /qn`
      removes the install dir.
    - `dumpbin /dependents` on both exes lists no `VCRUNTIME*.dll` or
      `MSVCP*.dll`.
    - The exe's file properties carry the version (PowerShell
      `(Get-Item …).VersionInfo.ProductVersion`).

  *Partial 2026-10-17:* written, not run ("CI disabled by Dan"; no Windows runner here, no windows
  target installed, so not even `cargo check --target`). Draft job `windows` in `release-workflow.yml`
  (MSVC, `-C target-feature=+crt-static`, dumpbin dependents check, `ProductVersion` check, zip via
  `package_archive.py`, MSI via `cargo packager -c "$PWD\Packager.toml" --formats wix`, msiexec install,
  `freerouting.exe --version`, uninstall check). `NEW DEP: winresource 0.1 (Windows-only
  build-dependency of freerouting-gui and freerouting-cli, for the icon and version resource; Cargo.lock
  +42 lines: winresource, toml, serde_spanned, toml_writer, winnow 1.0.4, ...)`; both `build.rs` files run it for msvc
  targets only (or `FREEROUTING_WINRES=1`), so Dan's gnu loop is unchanged, and a failed resource is a
  warning, not an error. The `build.rs` files compile and do nothing on Linux (clippy clean).
  Remaining (a Windows runner via the test tag of R01.7; signing is R02): everything that runs on Windows, the WiX build, the install path the MSI picks (the job
  searches `%ProgramFiles%` and `%LOCALAPPDATA%\Programs`).

- [~] **R01.6 — macOS release job.**
  - Job `macos` on `macos-14` (arm64): build both targets, `lipo -create`
    both binaries, make `freerouting-rs.app` (`Info.plist`, `icon.icns`)
    and a `.dmg` through cargo-packager (`-f app,dmg`), fallback `hdiutil
    create -format UDZO`; `codesign --force --deep -s -` (ad hoc); also a
    `tar.gz` with both universal binaries and the plugin.
  - **Accept:** on the runner:
    - `lipo -archs` on both binaries prints `x86_64 arm64`;
    - `hdiutil attach` mounts the DMG, and
      `<mount>/freerouting-rs.app/Contents/MacOS/freerouting-gui --version`
      prints the version;
    - `codesign -dv` shows an ad-hoc signature.
    - The Gatekeeper behaviour for a downloaded (quarantined) app is
      recorded, with K03.6's subprocess check.

  *Partial 2026-10-17:* written, not run ("CI disabled by Dan"; no macOS here). Draft job `macos` on
  `macos-14`: both targets, `lipo -create` with an `x86_64 arm64` check, tar.gz via `package_archive.py`,
  `cargo packager --formats app,dmg --binaries-dir universal` with `tools/release/macos_dmg.sh`
  (hand-made `.app` + `hdiutil create -format UDZO`, `bash -n` clean) as the automatic fallback, ad-hoc
  `codesign` + `Signature=adhoc` check, DMG mount and `Contents/MacOS/freerouting --version` (the GUI has
  no `--version`; it would open a window). Unverified: all of it, and the Gatekeeper behaviour of a
  quarantined app (K03.6's check); `release.md` states the "Open Anyway" steps from the GUI report, not
  seen on a Mac.

- [~] **R01.7 — Checksums and draft release.**
  - Job `release` (needs all build jobs; `permissions: contents: write`
    only here): download the artifacts, write `SHA256SUMS`, add the PCM zip
    and the patched `metadata.json` (K03.2, K03.4) when K03 exists, and
    create a **draft** release with `gh release create --draft` (no
    third-party release action needed).
  - **Accept:** a test tag produces a draft release whose assets are:
    the Linux `tar.gz` and AppImage, the Windows `zip` and MSI, the macOS
    DMG and `tar.gz`, `SHA256SUMS`, and (after K03) the PCM zip and
    `metadata.json`. `sha256sum -c SHA256SUMS` passes on the downloaded
    assets. The release URL is in the done note.

  *Partial 2026-10-17:* written, not run ("CI disabled by Dan"; no draft release was created and nothing
  was pushed or uploaded). Draft job `release` (needs all build jobs, tag-only, `permissions: contents:
  write` only there): downloads the artifacts, a K03 step that runs `kicad-plugin/package.py` and
  `update_metadata.py` only if they exist (K03 has not landed: skipped with a note), `SHA256SUMS`, then
  `gh release create --draft --verify-tag`. Local: `tools/release/sha256sums.py` on the local dist dir
  writes a file that `sha256sum -c SHA256SUMS` accepts for the tar.gz and the AppImage
  (`test_sums_verify_with_sha256sum` also checks that a tampered file fails). Remaining: a test tag, the
  draft release URL, the downloaded-asset check, the PCM zip and `metadata.json` after K03.

- [~] **R01.8 — Release documentation.**
  - `docs/development/release.md`: how to cut a release (bump the
    workspace version, run the gate, tag, push, review and publish the
    draft); what each asset is; the Linux glibc floor; the Windows
    SmartScreen warning for an unsigned MSI; macOS: the app is ad-hoc
    signed, so the first launch needs System Settings > Privacy & Security
    > Open Anyway (the GUI report says macOS 15 removed the Control-click
    bypass; check that before writing it); how to verify `SHA256SUMS`.
  - `README.md`: a short "Install" section pointing at the releases page.
  - **Accept:** the document exists, and its release steps were followed
    once for the test tag in R01.7 (the done note says so).

  *Partial 2026-10-17:* `docs/development/release.md` (how to cut a release, the asset table, verifying
  `SHA256SUMS`, the glibc floor, SmartScreen, macOS "Open Anyway", tool pins, the AppImage finding, the
  build stamp) and a README "Install" section exist. Remaining: its release steps have not been followed
  for a real test tag (R01.7 owns the tag), and the macOS 15 claim (Control-click removed) is from the GUI report, not
  checked on a machine (the quarantined-app check on a Mac is K03.6's manual step).
