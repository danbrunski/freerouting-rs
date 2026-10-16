# F00 — Foundations

Opened 2026-10-15. These two checklists come first because every other series
uses them. One makes the test loop fast and puts CI on every push. The other
builds the tools that the verification ladder in
[port-completion.md](../port-completion.md) relies on: Java probes, structural
SES comparison, benchmarking and the determinism check.

## Task index

| Task | Outcome | Size / risk | Prerequisite |
|---|---|---|---|
| [F01](archive/F01-dev-loop-and-ci.md) ✅ | Tests at opt-level 1 (151 s → 18 s for the reference tests); CI on Linux, Windows, macOS | S / build config; needs Dan's OK and a push | none |
| [F02](archive/F02-parity-and-benchmark-harness.md) ✅ | `probe.sh`, `freerouting-dev ses-diff` and `bench`, `determinism-check.sh`, board sets, the KiCad DRC oracle | M / new dev crate | none |
| [F03](F03-measurement-gate-and-corpus.md) | `freerouting-dev gate` (baseline comparator, ≥3-board rule), failed-vs-human records, an out-of-repo corpus of open KiCad boards, preregistered A/B runs | M / tooling | F02.2, F02.3, F02.6 |
| [F04](archive/F04-provenance.md) ✅ | Third-party notices, a provenance ledger, SPDX headers and a CI check for ported MIT code | S / licensing | none; before the first X-series port |

## Order

- F01 and F02 are independent and touch disjoint files, so they can run in
  parallel.
- [J01](archive/J01-repin-deterministic-java.md) (the re-pin) needs F02.4's
  determinism script.
- The A series needs F02.1–F02.3 for its parity checks.
- F03 is needed before the first Q, X or P04 measurement.
- F04 is needed before the first line copied from KiCadRoutingTools.
