# N01 — Routing profiles: `java` and `best`

Opened 2026-10-17 (D10). Defaults can now move away from Java, but the parity
baseline must stay reachable and tested. A profile is a named set of settings
defaults, applied before any settings file or CLI flag. A user's explicit
setting always wins over the profile.

Prerequisites: C02 (the settings table), C03 (the apply order), A10 (stage keys),
A14 (the parity gate).

## Decisions made here

- **One key, `router.profile`, with the values `java` and `best`.** The default is
  `best`. The profile only changes defaults, so a settings file that sets every
  key routes the same under both profiles.
- **`java` is the parity profile.** Every parity test and gate sets it
  explicitly: A14, connection-parity, `reference_load` and `reference_drc`, the
  probes, and `settings_defaults.rs` for Java's defaults. Run without
  `router.profile = java`, a parity test must fail loudly.
- **`best` starts equal to `java`.** Its table of differences starts empty. Each
  entry is added by N05 from an accepted measurement and names that
  measurement.
- **Settings dumps** (`--dump-settings`, the GUI's settings panel) show the
  profile and mark each value that came from it.

## Checklist

- [ ] **N01.1 — The profile key and table.** Add `router.profile` and a profile
  table (`settings/profile.rs`) applied first in C03's order. An unknown profile is
  a settings error naming the valid values.
  - **Accept:**
    - unit tests: the defaults under `java` equal today's defaults key by key;
    - an explicit key beats the profile;
    - an unknown profile fails with the message.
- [ ] **N01.2 — Pin the parity tests to `java`.**
  - **Accept:**
    - A14's exact set is unchanged with `java`;
    - a test sets a dummy `best`-only difference and shows that the A14 harness
      and `settings_defaults.rs` still pass, because they pin `java`.
- [ ] **N01.3 — CLI, GUI and plugin default to `best`.**
  - `--profile java` (or `--router.profile=java`) on the CLI.
  - A profile selector in the GUI's autoroute settings.
  - The plugin's options get a profile field (default `best`), and its summary
    names the profile.
  - **Accept:** each interface's tests cover both values; `kicad-plugin.md` and
    the CLI help document it.
- [ ] **N01.4 — Documentation.**
  - A DIVERGENCES section, "Profile `best`", that lists each difference from Java
    with its measurement.
  - `docs/development/engine.md` explains how to run in parity mode.
  - **Accept:** the section exists and is empty until N05 adds the first entry.
