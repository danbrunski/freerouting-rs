# R00 — Release builds

Opened 2026-10-15. The R series produces the release builds that "complete"
requires (item 7 of [port-completion.md](../port-completion.md)): Windows,
macOS universal and Linux, from one tag-triggered workflow, with a version
gate and checksums. CI (fmt, clippy and tests on three systems) is
[F01.2](archive/F01-dev-loop-and-ci.md), not part of this series. The narrative is
in [desktop-app.md](../desktop-app.md).

## Task index

| Task | Outcome | Size / risk | Prerequisite |
|---|---|---|---|
| [R01](archive/R01-release-builds.md) ✅ | Version gate, build stamp, icons, `release.yml` with Linux (AppImage, tar), Windows (MSI, zip) and macOS (universal DMG, tar) jobs, checksums, draft release, release docs | M / medium: young tooling, needs pushes | F01.2, F04.1; C08 (`--version`); K01.3 and K03 for the plugin parts |
| [R02](R02-signing.md) | *Optional.* Developer ID signing and notarization on macOS, Authenticode on Windows | S / low, but costs money and needs secrets | R01; Dan's decision |

## Order

- R01 can start once F01.2 is installed. R01.1–R01.6 work on today's
  binaries (viewer and DRC CLI), so the pipeline can be proven early.
- The first published release waits until the CLI routes (A13, C05, C07,
  C08) and the plugin routes (K02). Until then only draft test releases.
- K03's PCM zip joins R01.7's assets when K03 lands; K03 needs R01's
  binaries, so the two meet at R01.7.
- R02 only after Dan decides (question 1).

## Lanes and file ownership

R01 owns `.github/workflows/release.yml`, `tools/release/`,
`crates/freerouting-gui/icons/`, the packager config,
`crates/freerouting/build.rs` and `crates/freerouting/src/build_info.rs`,
and `docs/development/release.md`. It appends one step to F01.2's
`ci.yml`. R02 edits only `release.yml` and the docs. The K series owns
everything under `kicad-plugin/`; R01 only runs K03's scripts.

## Rules for this series

- **Pushing workflow files and tags is the orchestrator's job, with Dan's
  credentials.** The local `gh` token lacks the `workflow` scope; Dan runs
  `gh auth refresh -h github.com -s workflow` or pushes over SSH (GUI
  report §0 item 10). Until a push, workflow items are partial with
  "needs push".
- **Releases are drafts** until Dan publishes them.
- **Pin every tool and action** to an exact version, and reuse F01.2's
  action pins. Record the versions in `release.md`.
- **Every archive carries `LICENSE` and `THIRD_PARTY_NOTICES.md`** (F04.1).
- **No secrets in the repository or logs** (R02).

## Open questions for Dan

1. **Signing.** Apple's Developer Program costs $99 per year; Windows
   signing needs a certificate or a signing service. *Recommendation:*
   ad-hoc on macOS and unsigned on Windows, with the "Open Anyway" and
   SmartScreen steps documented, until there are users on those systems;
   then R02.
2. **Windows toolchain for releases.** *Recommendation:* MSVC with a
   static C runtime on CI (no mingw, no redistributable); Dan keeps the gnu
   toolchain for his local KiCad loop. Check once that the two builds give
   the same routing output (they should: no float or ordering difference
   is expected between toolchains, and D2 forbids hash order).
3. **The icon.** The only icon today is the plugin's 24 × 24 PNG. R01.3
   draws an original SVG icon. *Recommendation:* Dan approves it before the
   first public release; it must not imitate the Java Freerouting logo.
4. **First public version number.** *Recommendation:* `0.1.0` stays until
   the CLI routes; the first public release is `0.2.0`, and versions stay
   `0.x` until "complete" holds.
