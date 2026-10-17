# N01 — Routing profiles: `legacy` and `enhanced`

Opened 2026-10-17 (D10). Defaults can now move away from Java, but the parity
baseline must stay reachable and tested. A profile is a named set of settings
defaults, applied before any settings file or CLI flag. A user's explicit
setting always wins over the profile.

Prerequisites: C02 (the settings table), C03 (the apply order), A10 (stage keys),
A14 (the parity gate).

## Decisions made here

- **A profile is a visible, selectable starting point, not a hidden mode.** Users
  pick `legacy` for results identical to Java Freerouting, or `enhanced` (the
  default) for the accepted improvements. Below the profile, every stage (D9:
  fanout, net ordering, connection router, rip-up policy, optimizer, the special
  stages) and every option stays individually configurable. The profile only
  decides what each one starts at, and the UI shows which values differ from it.

- **One key, `router.profile`, with the values `legacy` and `enhanced`.** The default is
  `enhanced`. The profile only changes defaults, so a settings file that sets every
  key routes the same under both profiles.
- **`legacy` is the parity profile.** Every parity test and gate sets it
  explicitly: A14, connection-parity, `reference_load` and `reference_drc`, the
  probes, and `settings_defaults.rs` for Java's defaults. Run without
  `router.profile = legacy`, a parity test must fail loudly.
- **`enhanced` starts equal to `legacy`.** Its table of differences starts empty. Each
  entry is added by N05 from an accepted measurement and names that
  measurement.
- **Settings dumps** (`--dump-settings`, the GUI's settings panel) show the
  profile and mark each value that came from it.

## Checklist

- [ ] **N01.1 — The profile key and table.** Add `router.profile` and a profile
  table (`settings/profile.rs`) applied first in C03's order. An unknown profile is
  a settings error naming the valid values.
  - **Accept:**
    - unit tests: the defaults under `legacy` equal today's defaults key by key;
    - an explicit key beats the profile;
    - an unknown profile fails with the message.
- [ ] **N01.2 — Pin the parity tests to `legacy`.**
  - **Accept:**
    - A14's exact set is unchanged with `legacy`;
    - a test sets a dummy `enhanced`-only difference and shows that the A14 harness
      and `settings_defaults.rs` still pass, because they pin `legacy`.
- [ ] **N01.3 — CLI, GUI and plugin default to `enhanced`.**
  - `--profile legacy` (or `--router.profile=legacy`) on the CLI.
  - A profile selector (Legacy / Enhanced) in the GUI's autoroute settings.
  - The plugin's options get a profile field (default `enhanced`), and its summary
    names the profile.
  - **Accept:** each interface's tests cover both values; `kicad-plugin.md` and
    the CLI help document it.
- [ ] **N01.4 — The algorithm and option settings are visible.** One place in
  each interface lists the routing stages, the implementation chosen for each
  (by registered id), and that implementation's options, with their profile
  default:
  - GUI: an "Algorithms" section in the autoroute settings, below the profile
    selector; changing a value marks it as differing from the profile, and a
    reset returns it to the profile's value.
  - CLI: `--list-stages` prints the registered stages, ids and options as JSON
    and as a table; `--dump-settings` shows the effective values.
  - Plugin: the options dialog has the profile and an "Advanced" group for the
    stage choices; the options file stores only the values that differ from the
    profile.
  - **Accept:**
    - every registered stage id appears in each listing (a test compares them
      with the registry);
    - a GUI harness test changes one stage and resets it;
    - a plugin options round-trip stores only the differences.
- [ ] **N01.5 — Documentation.**
  - A DIVERGENCES section, "Profile `enhanced`", that lists each difference from Java
    with its measurement.
  - `docs/development/engine.md` explains how to run in parity mode.
  - **Accept:** the section exists and is empty until N05 adds the first entry.
