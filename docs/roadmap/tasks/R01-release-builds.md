# R01 — Release builds for Windows, macOS and Linux

Opened 2026-10-15. "Complete" includes release builds for Windows, macOS
(universal) and Linux (item 7 in [port-completion.md](../port-completion.md)).
Today there are none: Dan builds the Windows exes by hand with the gnu
toolchain, and nothing is versioned beyond `0.1.0` in `Cargo.toml`. This
checklist adds one tag-triggered workflow that builds, packages, checks and
drafts a GitHub release, plus the version gate and version stamp it needs.
It is the GUI report's REL-1 with KRT's "version gate first" amendment
(KRT report, "R and C — Release and settings"). CI itself (fmt, clippy,
tests on three systems) is F01.2; signing is [R02](R02-signing.md).

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

- [ ] **R01.1 — Version gate.**
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

- [ ] **R01.2 — Build stamp.**
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

- [ ] **R01.3 — Packager config and icons.**
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

- [ ] **R01.4 — Linux release job.**
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

- [ ] **R01.5 — Windows release job.**
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

- [ ] **R01.6 — macOS release job.**
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

- [ ] **R01.7 — Checksums and draft release.**
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

- [ ] **R01.8 — Release documentation.**
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
