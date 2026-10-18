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

- [x] **N01.1 — The profile key and table.** Add `router.profile` and a profile
  table (`settings/profile.rs`) applied first in C03's order. An unknown profile is
  a settings error naming the valid values.
  - **Accept:**
    - unit tests: the defaults under `legacy` equal today's defaults key by key;
    - an explicit key beats the profile;
    - an unknown profile fails with the message.
  - *Done 2026-10-17:* `settings/profile.rs` (`Profile::{Legacy, Enhanced}`, the `ENHANCED`
    table of `Difference { path, value, measurement }`, empty; `Profile::{parse, of_patch,
    of_settings, patch, defaults}`, `chosen`, `rebase`, `require_legacy`, `legacy_pin`,
    `with_enhanced_differences` for tests); `fields.rs` top-level `profile` (`opt`, unset is
    `enhanced`, so Java's dumps are unchanged); `merge.rs` `Source::Profile(_)` at priority 5,
    added by `resolve` from the highest-priority source that names a profile, `Resolved::profile`;
    `job/prepare.rs` returns `JobError::Settings` for an unknown profile; `cli_flags.rs`
    `--profile` alias. `tests/settings_profile.rs` 10 pass, among them
    `legacy_defaults_equal_todays_defaults_key_by_key` (every `FIELDS` path and `layers`),
    `an_explicit_key_beats_the_profile` (config file, DSN, rules file, environment, CLI, testing
    each beat a dummy difference), `an_unknown_profile_fails_with_the_valid_values`
    (`router.profile: unknown profile 'best' (valid profiles: legacy, enhanced)` from
    `Profile::parse`, `resolve` and `prepare_job`), `every_interface_spells_the_key` (flag, alias,
    environment, JSON). Adding an `enhanced` difference is one `ENHANCED` entry plus one
    DIVERGENCES line (`every_enhanced_difference_is_a_setting_with_its_divergence_line`).
- [x] **N01.2 — Pin the parity tests to `legacy`.**
  - **Accept:**
    - A14's exact set is unchanged with `legacy`;
    - a test sets a dummy `enhanced`-only difference and shows that the A14 harness
      and `settings_defaults.rs` still pass, because they pin `legacy`.
  - *Done 2026-10-17:* pins: `tests/board_parity/mod.rs` (`job_input` adds `legacy_pin()` at
    the testing priority, `load` fails through `require_legacy` without it),
    `tests/autoroute_common/mod.rs::load` (the probes, connection parity and the Java fixture
    ports), `settings_defaults.rs::java_defaults_are_the_legacy_profile`, and the CLI's
    `clirun` rows (`tests/common/mod.rs::ClirunRow::args` adds `--profile legacy`);
    `reference_load` and `reference_drc` read no router settings (documented in each). Dummy
    difference (`fanout.max_passes = 7` through `with_enhanced_differences`):
    `autoroute_board_parity.rs::the_gate_pins_legacy_beside_a_dummy_enhanced_difference` (J2
    loads with Java's settings, an unpinned load takes 7 and is refused by `require_legacy`, J2
    still routes byte-identical and equivalent to Java's session) and
    `settings_defaults.rs::java_defaults_are_the_legacy_profile` (every Java default holds
    pinned beside the dummy; unpinned shows 7). A14 in release with the pin, on
    `connection-parity.txt` + `benchmark.txt` (`PARITY_SET`, the 22 boards with a route
    reference): `test result: ok`, 2685 s; exact 16 of 16 pass and byte-identical, cutoff 6 of 6
    pass and byte-identical, 0 D8 breaches beyond Java's.
- [x] **N01.3 — CLI, GUI and plugin default to `enhanced`.**
  - `--profile legacy` (or `--router.profile=legacy`) on the CLI.
  - A profile selector (Legacy / Enhanced) in the GUI's autoroute settings.
  - The plugin's options get a profile field (default `enhanced`), and its summary
    names the profile.
  - **Accept:** each interface's tests cover both values; `kicad-plugin.md` and
    the CLI help document it.
  - *Done 2026-10-17:* CLI: `--profile <legacy|enhanced>` (an `ALIAS_FLAGS` row, so the GUI's
    front-end mode takes it too) and `--router.profile=`; `crates/freerouting-cli/tests/profiles.rs`
    `both_profiles_route_and_enhanced_is_the_default` (J2 `-mp 3`: legacy, enhanced and no flag
    write the same SES), `an_unknown_profile_is_a_usage_error` (exit 2), and
    `help_and_capabilities_document_the_profile` (help.txt "ROUTING PROFILES" and the
    `--profile` entry; `--capabilities` lists it). GUI: the Profile selector at the top of the
    autoroute settings (`dialogs/autoroute_settings/algorithms.rs`, switching calls
    `profile::rebase`); `tests/ui_routing.rs::profile_selector_covers_both_profiles` picks Legacy
    then Enhanced through the drop-down and checks the kept setting. Plugin: `options.py`
    `profile` field (default `enhanced`, `--profile legacy` otherwise, schema 2), the dialog's
    Profile choice, `summary.py` first line `Profile: Legacy|Enhanced`;
    `test_options.py::test_each_profile_has_its_argument_list[legacy|enhanced]`,
    `test_route_dialog.py::test_the_summary_names_the_profile[Legacy|Enhanced]`,
    `test_summary.py::test_the_summary_names_the_profile`, and against the real debug CLI
    `test_cli_integration.py::test_both_profiles_route_and_the_summary_names_them[legacy|enhanced]`
    (`FREEROUTING_RS_BIN=<target>/debug python -m pytest kicad-plugin/tests`: 250 passed, 3
    skipped). Documented in `kicad-plugin.md` ("Routing profile and Advanced stages (N01)") and
    `help.txt`.
- [x] **N01.4 — The algorithm and option settings are visible.** One place in
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
  - *Done 2026-10-17:* engine `autoroute/stages/catalog.rs` (`SLOTS`: 13 slots with their key,
    arity and option paths; `catalog(registry, profile, cores)` with the profile's ids and option
    defaults; `registered_ids`), `settings/dump.rs` `effective_rows`/`effective_text`/`effective_json`
    (value, source, `[profile]` mark, `*` for a value that differs from the profile).
    Registry comparison: `tests/stage_catalog.rs::{every_registered_stage_id_is_listed,
    an_added_stage_is_listed}` (4 pass); CLI `tests/profiles.rs::{list_stages_json_lists_every_registered_id,
    list_stages_table_lists_every_registered_id, dump_settings_shows_the_profile_and_the_sources}`
    (`--list-stages[=json]`, `--dump-settings[=json]`, with `-de` the design loaded as for a run);
    GUI `ui_routing.rs::an_algorithm_stage_changes_and_resets` (opens "Algorithms", checks a row
    per catalog slot, picks `freerouting-unrouted-report` for Differential pairs, sees it marked,
    clicks "Reset Differential pairs", back to unset) and `an_option_reset_returns_the_profile_value`;
    plugin `test_cli_integration.py::test_the_stage_listing_has_every_registered_id` (every
    `--capabilities` stage id is offered by the Advanced group, both profiles, real CLI) and
    `test_options.py::test_the_options_file_stores_only_the_differences` (defaults store
    `{"schema": 2, "profile": "enhanced"}` only; one option and one stage store just those) plus
    `test_route_dialog.py::test_the_profile_and_the_advanced_stages_reach_the_router`. The GUI
    section was rendered once with wgpu/lavapipe through a throwaway kittest (520 px wide: every
    slot, the "changed" mark and Reset on a changed row, list ids on one line); its look is for
    the owner.
- [x] **N01.5 — Documentation.**
  - A DIVERGENCES section, "Profile `enhanced`", that lists each difference from Java
    with its measurement.
  - `docs/development/engine.md` explains how to run in parity mode.
  - `README.md` ("Still in progress" and "What it does better than the Java version")
    names the profiles: Legacy, for results identical to Java Freerouting, and Enhanced,
    the default. It replaces "Routing profiles that go beyond Java are planned".
  - **Accept:**
    - the section exists and is empty until N05 adds the first entry;
    - the README names both profiles and how to pick one in the CLI, the GUI and the
      plugin.
  - *Done 2026-10-17:* `docs/DIVERGENCES.md` "Profile `enhanced`" section ("No entries yet:
    `enhanced` equals `legacy`"; `settings_profile.rs` checks it lists exactly the `ENHANCED`
    entries, 0 today) plus the `profile` line under Settings, the CLI flags line and the GUI dialog
    line; `docs/development/engine.md` "Parity mode: `router.profile = legacy`"; `README.md`
    "Two routing profiles" (Legacy, Enhanced, `--profile legacy`, the GUI's Profile selector, the
    plugin's Profile choice) and the "Still in progress" sentence replaced;
    `kicad-plugin.md` and `gui.md` ("What works").
