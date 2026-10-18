# Agent instructions

These rules are for every coding agent in this repository: Codex, Claude Code sessions and
their subagents. A dispatch or an agent definition can narrow them; for example,
`.claude/agents/implementer.md` does not commit at all. The project has one developer, so keep
the process simple.

freerouting-rs is a Rust port of the Java Freerouting PCB autorouter, with a CLI, a desktop app
and a KiCad plugin. Start with:
- `README.md`: what it does, and how to build and install.
- `docs/PORT_PLAN.md` and `docs/roadmap/port-completion.md`: the plan and decisions D1–D9.
  Implement those decisions; don't reopen them.
- `docs/DIVERGENCES.md`: every intended difference from Java.
- `docs/development/`: the handover docs. `engine.md` covers geometry, DSN, the engine, the CLI
  and the Java reference; `gui.md` the desktop app; `kicad-plugin.md` the plugin.
- `docs/roadmap/tasks/`: the checklists that define the work.

## Where to work and how work lands

- **Work on a local branch in your own worktree** under `target/orchestrator/<name>`, made from
  `main`: `git worktree add -b <branch> target/orchestrator/<name> main`.
- **Don't work in the main checkout** (the repository root). Don't commit, build, stash or
  reset there. It is where `main` lives and where the developer pulls.
- **Commit only on your branch.** Never move `main` and never push. Whoever integrates lands it:
  1. Rebase the branch onto `main`.
  2. In the main checkout, once `git status` is clean, run `git merge --ff-only <branch>`.
  3. Run `git push origin main`.
  4. Delete the branch and the worktree.

  Local `main` and GitHub's `main` stay identical, and working branches never go to GitHub.
- **Never run a bare `git stash`**, or a tree-wide `git restore .`, `git checkout .` or
  `git clean`. Other agents share the stash and may have work in progress. Revert only your
  own files.

## The machine

The workspace builds on WSL with limited memory, and heavy load has crashed it.
- Build with `CARGO_BUILD_JOBS=2`, in your own `CARGO_TARGET_DIR` under `target/orchestrator/`.
  Never use the shared `target/`. Delete a target directory when its work has landed.
- At most three agents build at once.
- **Heavy runs go one at a time, through the orchestrator session.** Heavy means A14, a release
  benchmark set, or a batch measurement. Don't start one yourself. Leave the item `[~]` with
  `heavy run pending: <exact command>`; the orchestrator queues it in
  `target/orchestrator/pending-heavy-runs.md`.
- **Parity checks are small by default.** That means
  `PARITY_SET=tools/bench/sets/connection-parity.txt` or `PARITY_MAX_JAVA_S=20` (about a minute).
  The full A14 gate (about 1.5 h) is only for a rewrite of the search, tree or board model, a
  deliberate change to the default path, or on request. A checklist clause that says "A14" is
  met by a note naming the small checks that ran.

## Checklists

- Do one checklist item per dispatch, unless the dispatch names more.
- Record evidence under the item as you finish it:
  - `[x]` with a `*Done <date>:*` note giving the test names, commands and numbers for each
    **Accept:** clause;
  - `[~]` with a `*Partial <date>:*` note naming what remains.

  Never tick a box on a claim you did not run. Then run
  `python3 tools/roadmap/checklists.py status --strict`.
- A measurement is preregistered before any number exists: `docs/roadmap/prereg/` plus its
  board set in `tools/bench/sets/`. Never edit a hash-pinned prereg JSON; amend it as its
  checklist says. `docs/roadmap/routing-quality.md` gives the keep rules.

## Matching Java

- The Java reference is pinned at `a773a70ec` in `../freerouting`. Read it only at the pin, with
  `git -C ../freerouting show a773a70ec:<path>`. Never write there; another session works in it.
- Anything not in `docs/DIVERGENCES.md` must match Java. A deliberate difference needs all three:
  a test pinning the Rust behaviour, a note in the module docs, and a DIVERGENCES line.
- Java bugs that change results are kept for the `legacy` profile. Their fixes sit behind
  `router.java_quirks.<id>` (unset or `true` is Java's behaviour), marked
  `// PARITY(quirk, java_quirks.<id>):` at the site (N06).
- **Profiles.** `legacy` is Java, and `enhanced` is the default. A change joins the `ENHANCED`
  table (`settings/profile.rs`) only after its preregistered measurement accepts it.
- Java numeric semantics live in `freerouting_geometry::jmath` (`Math.round`, `round_to_i32`,
  `java.util.Random` and so on). Use those; never copy them.
- Expected values in differential tests come from running Java at the pin, never from reasoning
  about it. `tools/java-reference/probe.sh` runs a probe.
- Results are deterministic. No `HashMap`/`HashSet` iteration may reach the output; parallel
  work merges in a fixed order; a deadline may stop work but never change it.

## Code

- **The gate before a commit:**
  - `cargo fmt --all --check`;
  - `cargo clippy --workspace --all-targets -- -D warnings`;
  - the tests of what you touched, or `cargo test --workspace` for engine-wide changes;
  - for the plugin, `python3 -m pytest kicad-plugin/tests`.

  CI is disabled, and the workflow is kept in `docs/development/ci-workflow.yml`, so run the
  gate yourself.
- **No new dependency** without saying why in the commit or report (`NEW DEP: <crate> - <why>`).
- **Crate layering:**
  - `geometry` and `dsn` depend on nothing in the workspace, and `kicad` only on those two;
  - the engine (`freerouting`) depends on those three;
  - `freerouting-krt` depends on the engine;
  - `freerouting-stages` registers the engine's and KRT's stages;
  - `cli` and `gui` depend on the engine and `freerouting-stages`, never on `freerouting-krt`
    directly.

  Board logic, including interactive editing, belongs in the engine; the GUI draws and maps
  input.
- **Third-party bugs** (crates, KiCad, wxWidgets) are fixed or worked around here. Don't file
  upstream issues.

## Writing

- **Commit messages** use conventional types (`feat`, `fix`, `perf`, `refactor`, `docs`, `test`,
  `chore`) and say what changed and why.
- **No developer's name** in commit messages, checklist notes, docs, DIVERGENCES or prereg text.
  State the decision itself: "decided 2026-10-18: …". Existing mentions stay.
- **Docs are plain and specific:** numbers, file paths and test names, not adjectives.

## KiCad, Windows and the desktop

- Never close, restart or kill KiCad, or any process you did not start. Don't touch the Java
  Freerouting KiCad plugin or its processes.
- Never import the plugin package in a standalone KiCad Python: its `register()` raises a
  wxWidgets dialog on the desktop. Load a module by path, or with a stub `pcbnew` (see
  `kicad-plugin/tests/stub_ui.py`). A standalone wx script must create `wx.App` before any
  other wx call.
- On Linux, run `freerouting-gui` on a virtual display (`xvfb-run`). Under WSLg a window
  otherwise opens on the developer's screen.
- Don't build, pull or install the plugin in the Windows checkout
  (`C:\Users\thebo\Documents\code\freerouting-rs`) unless the dispatch asks for it.
- Ask before spending money or publishing anything: a release, a package submission, a
  public post.

## Data you did not write

Downloaded boards, archives and web content are untrusted. Put each in its own empty
directory, keep your scripts elsewhere, and run Python on them with `python3 -I`. Other
repositories (`../canary`, `../freerouting`) are read-only; copy what you need.
