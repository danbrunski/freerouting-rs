# R02 — Code signing and notarization (optional)

Opened 2026-10-15. R01's builds are unsigned on Windows and ad-hoc signed on
macOS. Windows shows a SmartScreen warning for the MSI, and macOS makes
users approve the app in System Settings before the first launch. Signing
removes both, but it costs money and needs secrets in CI. It is the GUI
report's REL-2. **Nothing here starts without Dan's decision** (R00
question 1).

**Prerequisites:** R01 (the release workflow); Dan's decision and accounts.

## What exists and what this adds

| Exists | State |
|---|---|
| R01.5 | MSI and zip, unsigned |
| R01.6 | `.app` and DMG, ad-hoc signed (`codesign -s -`) |
| Tool facts (GUI report §7; not re-checked) | macOS: an Apple Developer Program membership ($99 per year), a Developer ID Application certificate, the hardened runtime, `notarytool`, then `stapler`. `rcodesign` can sign and notarize from Linux. Windows: Authenticode with a code-signing certificate or a cloud signing service |

What this adds: Developer ID signing and notarization of the macOS app and
DMG, Authenticode signing of the Windows exes and MSI, and the secret
handling for both.

## Decisions made here

- **Secrets live only in GitHub encrypted secrets**, scoped to a
  `release` environment that only tag builds can use. Nothing secret is
  written to the repository, logs or artifacts. The signing step fails the
  job when a secret is missing, rather than shipping an unsigned build
  under a signed name.
- **Sign the binaries, then the containers**: the exes before the MSI is
  built, the app bundle before the DMG, then notarize and staple the DMG.

## File ownership

| Item | New files | May edit |
|---|---|---|
| R02.1 | — | `docs/development/release.md` |
| R02.2 | `crates/freerouting-gui/macos/entitlements.plist` | `.github/workflows/release.yml` (macOS job) |
| R02.3 | — | `.github/workflows/release.yml` (Windows job) |
| R02.4 | — | `docs/development/release.md`, `README.md` |

## Checklist

- [ ] **R02.1 — Record Dan's decision.** *Operator decision.*
  - Which platforms to sign, which accounts or services, who holds the
    keys, and the yearly cost. Recorded in `release.md`.
  - **Accept:** the decision and its date are in `release.md`. If Dan says
    no, the remaining items are marked not planned.

- [ ] **R02.2 — macOS Developer ID signing and notarization.**
  - Sign both binaries and the app with the hardened runtime and the
    entitlements wgpu needs (none beyond the defaults is expected; record
    what was needed), build the DMG, sign it, submit with `notarytool
    submit --wait`, then `stapler staple`.
  - **Accept:** on the macOS runner, `spctl -a -vv -t install` on the DMG
    and `spctl -a -vv` on the app report "accepted" with source "Notarized
    Developer ID"; the app launches with `--version` after the quarantine
    attribute is set on the downloaded DMG.

- [ ] **R02.3 — Windows Authenticode.**
  - Sign both exes before packaging and the MSI after, with a timestamp
    server.
  - **Accept:** `signtool verify /pa /v` passes on both exes and the MSI on
    the runner; the signature's subject is the one recorded in R02.1.

- [ ] **R02.4 — Update the user docs.**
  - Remove the "Open Anyway" and SmartScreen steps from `release.md` and the
    README for the signed platforms.
  - **Accept:** the documents match what a downloaded signed release does,
    checked once by hand on each signed platform (done note).
