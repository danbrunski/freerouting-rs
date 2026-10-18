# Releases

How a release is cut, what is in it, and what a user sees on first launch. The pipeline is
`docs/development/release-workflow.yml` (R01); like the CI draft it sits here because GitHub CI
is disabled. **Until Dan enables it, nothing here has run on GitHub**: the scripts and the
Linux package were exercised locally (see "Verified so far"); the Windows and macOS jobs and
the draft release are untried.

## Cut a release

1. Bump the version in **all** of: `[workspace.package] version` in `Cargo.toml`, `version` in
   `Packager.toml`, `__version__` in `kicad-plugin/freerouting_rs/version.py`, and (K03) the
   version entry and `download_url` of `kicad-plugin/metadata.json`. Four-part versions are
   refused: KiCad's plugin manager accepts `MAJOR.MINOR.PATCH` only.
2. Run the gate: `python3 tools/release/check_version.py` (add `--tag vX.Y.Z` for the tag you
   are about to push). It prints each version and exits 1 on any difference. The release
   workflow runs it first with the tag, so a mismatch fails in seconds.
3. Test the scripts: `python3 -m unittest discover -s tools/release -p 'test_*.py'`.
4. Tag and push: `git tag vX.Y.Z && git push origin vX.Y.Z`. Pushing the workflow file or a tag
   needs Dan's credentials (the `gh` token lacks the `workflow` scope: `gh auth refresh -h
   github.com -s workflow`, or push over SSH).
5. The workflow builds Linux, Windows and macOS, smoke-tests them and creates a **draft**
   release. Open it, check the assets and `SHA256SUMS`, edit the notes, and publish it by hand.
   Nothing is published automatically.

`workflow_dispatch` runs the build and package jobs on a branch without a tag and makes no
release.

## Assets

| File | What |
|---|---|
| `freerouting-rs-<v>-linux-x86_64.tar.gz` | `freerouting`, `freerouting-gui`, `kicad-plugin/`, `README.md`, `LICENSE`, `THIRD_PARTY_NOTICES.md`, `LICENSES/` |
| `freerouting-rs-<v>-linux-x86_64.AppImage` | The GUI (and the CLI beside it inside the image) as one file; built by `tools/release/make_appimage.sh` (appimagetool) |
| `freerouting-rs-<v>-windows-x86_64.zip` | The two exes, `kicad-plugin\`, README, LICENSE, notices |
| `freerouting-rs-<v>-windows-x86_64.msi` | Installer for both exes (WiX through cargo-packager) |
| `freerouting-rs-<v>-macos-universal.dmg` | `freerouting-rs.app` (arm64 and x86_64) |
| `freerouting-rs-<v>-macos-universal.tar.gz` | The two universal binaries, the plugin, README, LICENSE, notices |
| `SHA256SUMS` | SHA-256 of every asset above |
| `freerouting-rs-<v>-pcm.zip` and `metadata.json` | The KiCad plugin-manager package (plugin plus the binaries of all three platforms; `kicad-plugin/package.py`) and the repository `metadata.json` with its checksum and sizes patched in (`update_metadata.py`); the job also runs KiCad's validator on the zip. Submitting it to KiCad's repository is a separate step that needs Dan's go-ahead |

Every archive carries `LICENSE` and `THIRD_PARTY_NOTICES.md`.

## Verify a download

Put the assets and `SHA256SUMS` in one directory, then `sha256sum -c SHA256SUMS` (Linux),
`shasum -a 256 -c SHA256SUMS` (macOS) or, in PowerShell,
`(Get-FileHash .\<file> -Algorithm SHA256).Hash` and compare with the line in the file.
`--ignore-missing` checks only the files you downloaded.

## Platform notes

- **Linux.** Built on Ubuntu 22.04, so the glibc floor is 2.35 (Ubuntu 22.04 or newer, Debian 12
  or newer). `tools/release/glibc_floor.sh` fails the build if either binary needs a newer
  symbol. No system library is bundled: X11 or Wayland, `libxkbcommon`, and a Vulkan (or EGL)
  driver come from the host. The AppImage needs FUSE 2 (`libfuse2`) or, without it,
  `APPIMAGE_EXTRACT_AND_RUN=1`. Headless: `xvfb-run -a` with `WGPU_BACKEND=vulkan` and Mesa's
  lavapipe (`mesa-vulkan-drivers`); the `gl` backend panics under Xvfb ("Invalid surface").
- **Windows.** MSVC build with a static C runtime (`-C target-feature=+crt-static`), so no Visual
  C++ redistributable is needed. The exes are **unsigned**: SmartScreen shows "Windows protected
  your PC"; More info > Run anyway. The MSI shows the same for an unsigned publisher. Dan's local
  KiCad loop keeps the gnu toolchain (its `build.rs` skips the icon resource unless
  `FREEROUTING_WINRES=1`).
- **macOS.** One universal build (the x86_64 half is cross-compiled on the arm64 runner). The app
  is **ad-hoc signed** only (`codesign -s -`), not notarized. A downloaded copy is quarantined;
  on first launch open System Settings > Privacy & Security, scroll to the message about
  "freerouting-rs" and choose **Open Anyway**. macOS 15 removed the Control-click > Open
  shortcut; on macOS 14 it still works. (This is from the GUI report and Apple's documentation,
  not yet seen on a machine here; R01.6 stays partial until someone does it.) The builds stay
  unsigned: Dan decided on 2026-10-18 that the project does not need code signing (R02, not
  planned), so these warnings are the expected first-run steps on Windows and macOS.

## Tools and pins

| Tool | Pin |
|---|---|
| cargo-packager | 0.11.8 (`cargo install cargo-packager --version 0.11.8 --locked`) |
| `actions/checkout` | v7 |
| `actions/setup-python` | v6 |
| `actions/upload-artifact` | v7.0.2 |
| `actions/download-artifact` | v8.0.2 |
| `dtolnay/rust-toolchain` | `stable` |
| `Swatinem/rust-cache` | v2.9.2 |
| appimagetool | continuous build 313 (2026-10-04), sha256 `95cbe7cc...` in `make_appimage.sh` |
| Runners | `ubuntu-22.04` (build), `ubuntu-24.04` (smoke), `windows-latest`, `macos-14` |

cargo-packager is used for the MSI (`-f wix`) and the macOS app and DMG (`-f app,dmg`) with the
root `Packager.toml`. Two quirks of 0.11.8, both handled in the config: `-c` must be an absolute
path, and `name` must be set (else it fails with "Not a directory", which its auto-detection
reports as "no config found").

**The AppImage does not use cargo-packager.** On 2026-10-17 (Ubuntu 24.04, Rust stable)
`cargo packager -f appimage` built an AppImage whose GUI binary was rewritten by linuxdeploy to
18 MB (the input is 32 MB; a plain `strip` or `patchelf` on the same input is fine and
`NO_STRIP=1` made no difference) and hung before the first frame: no screenshot, no error. So
the AppImage comes from `tools/release/make_appimage.sh`: an AppDir assembled by hand (binaries
untouched, an `AppRun` shell script that keeps the working directory, so relative board paths
work) and `appimagetool` (the "continuous" build of 2026-10-04, checked against a SHA-256 in
the script). cargo-packager's AppImage AppRun, by contrast, changes into the AppDir, so a
relative board path opens nothing.

Other fallbacks: MSI through `cargo-wix` 0.3.9 (WiX v3 is on `windows-latest`); DMG through
`tools/release/macos_dmg.sh` (`hdiutil create -format UDZO`), which the macOS job runs by itself
if the packager step fails. cargo-packager's last release was in late 2025, so keep the pin and
the fallbacks.

The GUI has no `--version` (it would open a window); the smoke checks use the CLI's
`freerouting --version` (in the tar, in the installed MSI, inside the `.app`) and the GUI's
`--screenshot ... --exit`.

## The build stamp

`freerouting::build_info::{VERSION, GIT_SHA}` (engine crate `build.rs`) feed `freerouting
--version` (`freerouting <version> (git <12 hex>)`) and the GUI's About. The sha comes from
`FREEROUTING_GIT_SHA` (the workflow sets it to `github.sha`), else `git rev-parse --short=12
HEAD` in a checkout, else `unknown` (a source tarball). Release (non-debug) builds re-stamp when
HEAD moves; a debug build keeps the sha of its first build so a commit does not recompile the
engine. The stamp is never read by routing code.

## Icons

`crates/freerouting-gui/icons/icon.svg` is the single source; `render.py` makes the PNGs, `.ico`
and `.icns` (see its README). Dan approves the artwork before the first public release.

## Verified so far

See the done notes of `docs/roadmap/tasks/R01-release-builds.md` for what ran, where and with
which numbers.
