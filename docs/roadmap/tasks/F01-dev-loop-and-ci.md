# F01 — Dev loop and CI

Opened 2026-10-15. The workspace tests take 1.5–4 min in a plain debug build,
and there is no CI: the cloud sessions' GitHub token couldn't push
`.github/workflows/`, so the workflow sits in `docs/development/ci-workflow.yml`.
Every later checklist runs the gate many times, so a faster loop and a real CI
pay for themselves first.

## What exists and what this adds

| Exists | State |
|---|---|
| `docs/development/ci-workflow.yml` | fmt, `clippy -D warnings`, `cargo test --workspace` on Linux, Windows, macOS. Not installed |
| Workspace `Cargo.toml` | no `[profile.*]` sections, so tests run at `opt-level = 0` |
| Implementer agent | already runs its tests with `CARGO_PROFILE_DEV_OPT_LEVEL=1` in its own target dir |

Measured 2026-10-15 at `b119467` on this machine, quiet:

| Run | opt-level 0 | opt-level 1 | opt-level 2 |
|---|---|---|---|
| `reference_drc` + `reference_load` | 94 + 57 = 151 s | 10.6 + 7.0 = 17.6 s | 9.6 + 6.6 = 16.2 s |
| whole workspace, warm | 1.5–4 min (load-dependent) | 23 s | — |
| first build of the engine crates | — | ~23 s incl. tests | ~23 s |
| GUI tessellation test | 36–41 s | 3.1 s | — |

## Decisions made here

- **Opt-level 1 for dev and test builds.** This is recommended, and Dan
  confirms it at F01.1. Opt-level 2 adds almost nothing over 1. Overflow
  checks and debug assertions stay on, because only `opt-level` changes. Rust
  doesn't change float semantics with optimisation, so test results are the
  same. The cost is worse debugger stepping. Anyone who needs that can build
  with `CARGO_PROFILE_DEV_OPT_LEVEL=0`.
- **CI matches the local gate exactly**: the same three commands, so a green
  local gate predicts a green CI.

## File ownership

| Item | New files | May edit |
|---|---|---|
| F01.1 | — | `Cargo.toml` (workspace), `docs/development/engine.md` (test timing line), `.claude/agents/implementer.md` (test command) |
| F01.2 | `.github/workflows/ci.yml` | `docs/development/engine.md` ("Working practices"), delete `docs/development/ci-workflow.yml` |

## Checklist

- [ ] **F01.1 — Build dev and test profiles at opt-level 1.**
  *Operator decision:* Dan confirms opt-level 1 (recommended above). Until he
  does, this item is partial and nothing changes.
  - Add `[profile.dev] opt-level = 1` to the workspace `Cargo.toml`, with a
    one-line comment giving the measured reason.
  - Measure the incremental rebuild after touching
    `crates/freerouting/src/board/basic_board.rs`
    (`touch` + `cargo test --no-run -p freerouting`, three times each, before
    and after). The faster test runs must not be paid for with a much slower
    edit-compile loop.
  - Update the test-timing line in `engine.md` ("about a minute") and the
    implementer agent's test command. The agent no longer needs its private
    `CARGO_PROFILE_DEV_OPT_LEVEL` target dir. Keep its own `CARGO_TARGET_DIR`
    only if parallel agents still contend for `target/`.
  - **Accept:**
    - The workspace warm test time and the incremental rebuild time, before and
      after, are in the done note.
    - Warm tests are at least 4× faster.
    - The incremental rebuild is at most 1.5× slower. If it is slower than
      that, record the numbers and leave the profile unchanged.

- [ ] **F01.2 — Install CI.**
  - Move `docs/development/ci-workflow.yml` to `.github/workflows/ci.yml` and
    add:
    - `permissions: contents: read`
    - `timeout-minutes: 30` per job
    - `Swatinem/rust-cache` (pinned to a release tag)
    - `CARGO_TERM_COLOR: always`
    - on Linux, the system packages the GUI crate needs to build, if any (find
      out from the first run; don't guess)
  - Keep the three gate commands identical to the local gate.
  - The workflow only reaches GitHub on a push, and pushing is the
    orchestrator's job. Pushing `.github/workflows/` needs Dan's credentials,
    which the local git remote (`git@github.com:danbrunski/freerouting-rs.git`)
    uses. So the implementer prepares the file and validates it with
    `python3 -c 'import yaml,sys; yaml.safe_load(open(sys.argv[1]))'`, and
    the CI evidence is added after the push.
  - **Accept:**
    - A pushed commit shows the workflow green on all three operating systems;
      the done note has the run URL.
    - `engine.md`'s note that "there is no CI yet" is replaced with the
      workflow's location.
    - Until the push, this item stays partial with "needs push".
