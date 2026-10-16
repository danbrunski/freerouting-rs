# C02 — Settings model: one table, Java's names, Java's merge

Opened 2026-10-15. Java's router reads every setting from `RouterSettings`. Its
values come from defaults, `freerouting.json`, the DSN, a rules file,
environment variables and the CLI, merged in a fixed order. Rust has none of
this. Only the V2 score weights exist (`scoring::RouterScoreSettings`,
`OptimizerScoreSettings`). This checklist ports the settings model and its
sources. It does not apply settings to a board; that is C03.

It comes early because other series need it:
- B06 drives the router and optimizer scores from it;
- A01 builds `AutorouteControl` from it, through C03's board view;
- A10 appends the D9 stage keys to it.

**Prerequisites:** none. C02.8's comparison needs C01.1's `settings` dumps.
Every other item can land first.

## What exists and what this adds

| Exists | Where |
|---|---|
| V2 score weights, without `version` or `unrouted_connection_weight` | `crates/freerouting/src/scoring/board_statistics.rs` (`RouterScoreSettings`, `OptimizerScoreSettings`) |
| A JSON writer (`JsonValue`, numbers kept as text) | `crates/freerouting/src/drc/json.rs` |
| A test-only JSON parser | `crates/freerouting/tests/reference_load.rs` (`JsonParser`) |
| The DSN/rules `autoroute_settings` syntax tree | `freerouting_dsn::ast::AutorouteSettings`, `AutorouteLayerRule`; `RulesFile.items` keeps file order |
| `autoroute_settings` noticed but not applied | `io/specctra/parser/structure.rs:156` (sets `autoroute_settings_read` only) |
| Net class flag the router will read | `rules::NetClass::is_ignored_by_autorouter`, `BasicBoard::has_ignored_nets` |
| CLI parsing of `-de`, `-drc`, `-dr` only | `crates/freerouting-cli/src/main.rs` |
| Java at `cda540f` | `settings/{RouterSettings, AutorouterSettings, FanoutSettings, OptimizerSettings, RoutingCostSettings, RouterScoreSettings, OptimizerScoreSettings, LayerSettings, LegacyRouterSettingsBridge, SettingsMerger}.java`, `settings/sources/{DefaultSettings, JsonFileSettings, DsnFileSettings, RulesFileSettings, EnvironmentVariablesSource, CliSettings}.java`, `util/ReflectionUtil.java`, `util/gson/RouterSettingsTypeAdapterFactory.java`, `io/specctra/parser/AutorouteSettings.java` (`readScope`, `readLayerRule`), `util/TextManager.parseTimespanString` |

What this adds: `crates/freerouting/src/settings/`, with the field table, the
patch and resolved types, the merge, every source except the CLI argument
parser (that is C07), validation, test profiles and the comparison dump.

## Decisions made here

- **One table defines every field.** A `macro_rules!` table in
  `settings/fields.rs` lists each field once, with:
  - its canonical JSON path (`scoring.start_ripup_costs`);
  - the Java field name (`startRipupCosts`) and Gson alternates;
  - the flat legacy alias, if any (`start_ripup_costs` at router level);
  - its Java type, default, clamp and doc line.

  The macro generates `SettingsPatch`, `RouterSettings` and `FIELDS: &[FieldDesc]`.
  The CLI (C07), environment variables, JSON files, the comparison dump, the
  help text and later the GUI settings form (G06, G07) all use `FIELDS`. That
  is KRT's "one definition holds every default" rule.
- **Java's types.** `Integer` → `i32`, `Long` → `i64`, `Double` → `f64`,
  `Float` → `f32`, `Boolean` → `bool`, `String[]` → `Vec<String>`. Enums
  (`RouterScoringVersion`, `OptimizerScoringVersion`, `BoardUpdateStrategy`,
  `ItemSelectionStrategy`) become Rust enums with Java's constant names. Score
  weights stay `f32`, because B06's scores are `float` arithmetic.
- **`SettingsPatch` is Java's nullable `RouterSettings`, field for field.** It
  includes the transient `layers` array, the two transient cost arrays
  (`scoring.preferred_direction_trace_cost`,
  `scoring.undesired_direction_trace_cost`) and the transient flag
  `boardSpecificTraceCostsApplied` (here `costs_initialized: Option<bool>`).
  C03's cost tuning and the merge rules read and write exactly these. This is
  a place where Java's structure decides results (D1). `RouterSettings` is the
  resolved getter view that everything else uses.
- **Java's precedence, quirks included.** Sources are sorted by priority
  with a stable sort, so ties keep insertion order:

  | Source | Priority |
  |---|---|
  | Defaults | 0 |
  | Config file | 10 |
  | DSN | 20 |
  | Rules file (first merge only through `-dr`) | 40 |
  | Environment | 55 |
  | CLI | 60 |
  | Second merge (`ApiSettings`) | 70 |
  | Testing | 80 |

  A rules file's `autoroute_settings` are then applied *on top* of the merged
  settings (C04.3), so they override the CLI. That quirk is reproduced, not
  fixed.
- **No new dependency.**
  - No serde and no TOML.
  - Config files are JSON in Java's shape, with a `"router"` member, so
    Dan's `freerouting.json` works unchanged.
  - The JSON parser is appended to `drc/json.rs` as `JsonValue::parse`.
    Numbers keep their source text, so they convert exactly as Java's
    `Integer.parseInt` and `Double.parseDouble` would.
- **Config file location.**
  - `--settings FILE` replaces the default config-file source; it does not
    stack.
  - The default is `<config dir>/freerouting-rs/freerouting.json`:
    - Windows: `%APPDATA%\freerouting-rs\`;
    - macOS: `~/Library/Application Support/freerouting-rs/`;
    - Linux: `$XDG_CONFIG_HOME/freerouting-rs/`, defaulting to
      `~/.config/`.
  - `--user_data_path=DIR` and `FREEROUTING__USER_DATA_PATH` point at
    `DIR/freerouting.json`, as in Java.
  - Java's own file is read only through `--settings`; see C00 question 1.
  - A WSL process never sees the Windows file. On this machine
    `~/.config/freerouting/freerouting.json` exists with empty router
    sections, while `/mnt/c/Users/thebo/AppData/Roaming/freerouting/freerouting.json`
    holds `start_ripup_costs = 1500`.
  - A malformed default file gives a warning and is ignored, as Java's
    `JsonFileSettings` does. A malformed or missing `--settings` file is an
    error.
- **Machine-dependent defaults take an explicit `cores`.** `resolve` takes a
  `ResolveContext { cores: usize }`. `max_threads` defaults to
  `max(1, cores - 1)`. Nothing that decides results may read a thread count
  (D2). Tests pass a fixed `cores`.
- **Work bounds and deadlines are different types (D2).**
  - `RouterSettings::work_bounds() -> WorkBounds` carries:
    - `autorouter.max_passes` (0 = unlimited) and `autorouter.max_items`;
    - `fanout.max_passes`, `fanout.max_items` and
      `fanout.max_milliseconds_per_pin`. Since Java `30baad7e2` that is a
      work budget in nominal milliseconds: one nominal ms is 64,000
      search-tree node visits on a `WorkMeter` (J01; the A series counts
      the same unit). C02 stores the number and never converts it to time;
    - `optimizer.max_passes` (0 = none) and `optimizer.max_items`.
  - `RouterSettings::deadlines() -> Deadlines` carries `job` (default
    `12:00:00`, capped at 24 h), `fanout: Option<Duration>` and
    `optimizer: Option<Duration>`.
  - Stage code takes `&WorkBounds` only. Only C07's job runner sees
    `Deadlines`, and a deadline can only stop work.
  - Durations parse as Java's `TextManager.parseTimespanString` does:
    `HH:mm:ss`, `mm:ss` or `ss`. Anything else is `None` with a warning,
    which makes Java's `"5m"` a no-op.

## File ownership

| Item | New files | May edit |
|---|---|---|
| C02.1 | `crates/freerouting/src/settings/{mod.rs, fields.rs, types.rs}`, `crates/freerouting/tests/settings_defaults.rs` | `crates/freerouting/src/lib.rs` (append `pub mod settings;`) |
| C02.2 | `settings/merge.rs`, `tests/settings_merge.rs` | `settings/mod.rs` |
| C02.3 | `settings/path.rs`, `tests/settings_path.rs` | `settings/mod.rs` |
| C02.4 | `settings/json_file.rs`, `tests/settings_json.rs`, `crates/freerouting/tests/data/settings/*.json` | `drc/json.rs` (append `parse` and its error type only), `settings/mod.rs` |
| C02.5 | `settings/config_path.rs` | `settings/mod.rs` |
| C02.6 | `settings/env.rs`, `tests/settings_env.rs` | `settings/mod.rs` |
| C02.7 | `settings/autoroute_scope.rs`, `tests/settings_autoroute_scope.rs` | `settings/mod.rs` |
| C02.8 | `settings/{validate.rs, profiles.rs, dump.rs}`, `tests/settings_reference.rs` | `settings/mod.rs` |

Paths are under `crates/freerouting/src/` unless they start with
`crates/`. A10 later appends the D9 stage keys to `settings/fields.rs`
(A10.1); nobody else edits the table without a checklist item.

## Checklist

- [x] **C02.1 — The field table, `SettingsPatch` and `RouterSettings`.**
  Java: the field declarations in the settings classes listed above;
  defaults from `DefaultSettings.getSettings()`.
  - Top level:
    - `job_timeout` `"12:00:00"`;
    - `trace_pull_tight_accuracy` (alias `tracePullTightAccuracy`) 500;
    - `allowed_via_types` true;
    - `automatic_neckdown` (alias `automaticNeckdown`) true;
    - `max_threads` cores − 1;
    - `copper_to_edge_clearance_um` 250.0;
    - `hole_clearance_um` 0.0;
    - `clearance_tolerance_um` 1.0;
    - `plane_nets` `[]`;
    - `plane_as_obstacle` (aliases `conduction_is_obstacle`,
      `planeAsObstacle`, `conductionIsObstacle`) false;
    - `neck_width_um` 0.0;
    - `min_trace_width_um` unset (Java fork `46bf9e2ef`, ledger "Port: yes").
      Opt-in. When set, it floors every narrowing: pin neck-downs in the maze
      search and the inserter, the inserter's micro-neckdown, and
      `neck_width_um`. Unset gives byte-identical results to `a773a70ec`.
      This is how D8's board minimum reaches the router when the input is a
      DSN, which doesn't carry `min_track_width` (Q02, K02);
    - `strict_drc` false;
    - `result_json` null;
    - `layers` null.
  - `autorouter`:
    - `enabled` true;
    - `algorithm` `freerouting-router`;
    - `max_passes` 0;
    - `max_items` `i32::MAX`;
    - `max_threads` cores − 1;
    - `save_intermediate_stages` false;
    - `ignore_net_classes` `[]`.
  - `fanout`:
    - `enabled` true;
    - `max_passes` 20;
    - `max_items` `i32::MAX`;
    - `max_milliseconds_per_pin` 10000 (`i64`);
    - `ripup_allowed` (alias `ripupAllowed`) true;
    - `min_escape_length_mm` 2.5;
    - `max_escape_length_mm` 4.5;
    - `start_via_diameter_mm` and `end_via_diameter_mm` 0.25 (read by
      nothing in Java);
    - `pin_sorting_order` `outer_first`;
    - `fallback_to_board_vias` true;
    - `timeout` null.
  - `optimizer`:
    - `enabled` true;
    - `algorithm` `freerouting-optimizer`;
    - `max_passes` 100;
    - `max_items` `i32::MAX`;
    - `max_threads` cores − 1;
    - `improvement_threshold` 2.5 (`f32`);
    - `board_update_strategy` `GLOBAL_OPTIMAL`;
    - `item_selection_strategy` `SEQUENTIAL`;
    - `additional_ripup_cost_factor_at_start` 10;
    - `trace_ripup_cost_factor` 0.6 (`f32`);
    - `max_autoroute_passes` 6;
    - `enable_preflight_guards` true;
    - `max_consecutive_failures` 50;
    - `max_consecutive_failures_pass1` 12;
    - `timeout` null.
  - `scoring`:
    - `via_costs` (alias `viaCosts`) 50;
    - `plane_via_costs` 5;
    - `start_ripup_costs` (alias `startRipupCosts`) 100;
    - `default_preferred_direction_trace_cost` and
      `default_undesired_direction_trace_cost` 1.0;
    - `default_bend_cost` 0.0;
    - `unrouted_net_penalty` 5e6, `clearance_violation_penalty` 1e6 and
      `bend_penalty` 10 (all `f32`);
    - the two cost arrays, null.
  - `router_scoring`:
    - `version` `V2_CONTINUOUS`;
    - `unrouted_connection_weight` 1000;
    - `unrouted_free_fraction` 0.5;
    - `unrouted_first_half_weight` `1000f/3f`;
    - `unrouted_second_half_weight` `2000f/3f`;
    - `clearance_violation_count_weight` 25,
      `clearance_violation_depth_weight` 300 and
      `clearance_violation_depth_scale` 1000.
  - `optimizer_scoring`:
    - `version` `V2_LOWER_BOUND`;
    - `excess_wire_length_weight` 1000, `excess_via_weight` 2000 and
      `excess_bend_weight` 500;
    - `length_floor` 1 and `difficulty_scale_floor` 1.
  - `LayerSettings`:
    - `routable`, `preferred_direction_horizontal` and `bend_cost`;
    - `preferred_direction_trace_cost` and
      `undesired_direction_trace_cost`.

  Resolved `RouterSettings` reuses `scoring::RouterScoreSettings` and
  `OptimizerScoreSettings` for the V2 weights. It wraps each in a struct that
  adds `version` (and `unrouted_connection_weight`), so `scoring/` is not
  edited. Getter fallbacks for values still unset after a merge follow Java:
  - `getStartRipupCosts`, `getViaCosts` and `getPlaneViaCosts` → 1;
  - `getRunOptimizer`, `isFanoutEnabled` and `getAutomaticNeckdown` →
    false;
  - `getRunRouter` → true.
  - **Accept:**
    - `settings_defaults.rs` asserts every default above, with `cores = 8`.
    - `every_field_has_a_table_entry` fails if a resolved field or a
      patch field is missing from `FIELDS`. Its negative control adds an
      untabled field behind `#[cfg(test)]`.
    - Port of `BendCostSettingsTest.nullScoringSafety`: an empty
      `SettingsPatch` resolves through the fallbacks to 1, 1 and 1.0.
  - *Done 2026-10-15:* `crates/freerouting/src/settings/{fields.rs, types.rs, mod.rs, layers.rs}`.
    One `settings_table!` invocation in `fields.rs` lists 76 scalar fields once (JSON
    name, Java name, Gson alternates, flat alias, Java type, default, unset-fallback,
    doc line) and generates `SettingsPatch`, `RouterSettings`, the six section
    structs of each, `SECTIONS`, `FIELDS` and `default_patch(cores)`. `LayerSettings`,
    `layers`, `costs_initialized` and `validation_warnings` are written by hand; the
    cost arrays and the optimizer strategies are table entries marked not-JSON.
    Getter fallbacks (`via_costs`/`plane_via_costs`/`start_ripup_costs` 1,
    `optimizer.enabled`/`fanout.enabled`/`automatic_neckdown` false,
    `autorouter.enabled` true) are the table's `fb` kind. The resolved sections
    reuse `scoring::{RouterScoreSettings, OptimizerScoreSettings}` through
    `score_weights()` (checked equal to their `Default`), so `scoring/` is unedited.
    `RouterSettings::work_bounds()` / `deadlines()` (D2) are in `mod.rs`.
    `tests/settings_defaults.rs` (8 tests): `defaults_with_eight_cores` asserts all
    76 defaults with `cores = 8` and fails if it omits a table entry;
    `every_field_has_a_table_entry` walks the `Debug` output of `SettingsPatch` and
    `RouterSettings` and compares it with `FIELDS` (negative control
    `table_check_flags_an_untabled_member` adds an untabled member to a probe
    struct and sees it); `null_scoring_safety` ports
    `BendCostSettingsTest.nullScoringSafety` (settings half: 1, 1, 5, 3, 2.0 and
    1.0 through the fallbacks; the `applyBoardSpecificOptimizations` half is
    C03); `work_bounds_carry_counts_and_deadlines_carry_durations` (24 h cap,
    `"5m"` no-op). Differential: `tests/settings_java_probe.rs` scenario `defaults`
    equals Java 25's `DefaultSettings` merge at `cda540f`.

- [x] **C02.2 — Merge with Java's `copyFields` semantics.**
  Java: `SettingsMerger.merge` (the first non-null source is cloned as the
  base, the rest applied in priority order, then `validate()`) and
  `ReflectionUtil.copyFields`. Rust:
  `SettingsPatch::apply_from(&mut self, src: &SettingsPatch)` and
  `resolve(&[(Source, SettingsPatch)], &ResolveContext) -> Resolved {
  patch, settings, provenance, warnings }`. Rules to reproduce:
  - `Some` overwrites and `None` never does. That includes `Some(false)`
    and `Some(0)`: wrapper types copy any non-null value.
  - String lists and primitive arrays (`plane_nets`,
    `ignore_net_classes`, the cost arrays) are replaced whole, even by
    `Some(vec![])`.
  - **`layers` merges element by element when the target is at least as
    long as the source.** Otherwise it is rebuilt from the source alone,
    and the lower-priority per-layer values are lost. Name the test after
    that asymmetry.
  - `costs_initialized` is copied whenever it is `Some`.

  `Provenance` maps each canonical path to the last `Source` that set it, for
  one debug line per field (`start_ripup_costs = 1500 (config file)`).
  - **Accept:**
    - Ports of every test method in `SettingsMergerTest`, named after
      Java's methods. That covers `priorityOrdering`, `partialOverrides`,
      `emptySourcesList`, `nullValueHandling`, `layersArrayNotShrunkOnMerge`,
      the legacy-batch cases (needing C02.3's CLI patch builder) and
      `cliNestedAutorouterMaxThreadsIsIndependentOfOptimizer`.
    - `Issue885SettingsInitializationTest.reflectionUtilCopiesExplicitEmptyArray`.
    - A test that a shorter target array is rebuilt from the source only.
  - *Done 2026-10-15:* `settings/merge.rs`: `SettingsPatch::apply_from`
    (`copyFields`), `resolve(&[(Source, SettingsPatch)], &ResolveContext) -> Resolved
    { patch, settings, provenance, warnings, cores }`, `Source` with Java's
    priorities (0, 10, 20, 40, 55, 60, 70, 80, stable sort), `Resolved::provenance_lines`
    (`scoring.start_ripup_costs = 1500 (config file)`). The merge base is cloned
    like `RouterSettings.clone()`, which drops `result_json`
    (`merge_base_clone_drops_result_json`).
    `tests/settings_merge.rs` (24 tests): every `SettingsMergerTest` method, named
    after Java's (`default_settings_only`, `empty_sources_list`, `source_priorities`,
    `multiple_sources_merging`, `priority_ordering`, `null_value_handling` (an empty
    patch, since a patch can't be null), `partial_overrides`,
    `environment_variables_priority`, `complex_merging`,
    `cli_can_disable_router_and_optimizer`,
    `legacy_batch_mode_enables_router_when_json_disables_it`,
    `explicit_router_enabled_false_overrides_legacy_batch_mode`,
    `cli_routable_layers_does_not_disable_router`, `layers_array_not_shrunk_on_merge`,
    `cli_optimizer_improvement_threshold_parses_and_scales_fraction`,
    `cli_nested_autorouter_max_threads_is_independent_of_optimizer`);
    `reflection_util_copies_explicit_empty_array`
    (`Issue885...`), `copy_fields_overwrites_existing_non_empty_array`;
    `shorter_target_layers_are_rebuilt_from_the_source_only` names the layers
    asymmetry; also `some_false_and_some_zero_overwrite_and_none_never_does`,
    `equal_priorities_keep_insertion_order`, provenance. Differential:
    `settings_java_probe.rs` merges Defaults with CLI, env, JSON, DSN, rules and the
    `Api` second merge on Java 25 at the pin and compares the whole tree: 17 of 18
    scenarios equal, `json-every` differs only in a `null` layer element (listed,
    DIVERGENCES.md).

- [x] **C02.3 — Path setter for CLI and environment values.**
  Java: `ReflectionUtil.setFieldValue` and `convertValue`;
  `LegacyRouterSettingsBridge.canonicalCliPath` and
  `isDeprecatedFlatAutorouterPath`; `CliSettings.applyRouterSetting`. Rust:
  `SettingsPatch::set_path(&mut self, path: &str, value: &str) ->
  Result<(), SetError>`, plus `cli_property(name, value)` for CliSettings'
  rewrites.
  - **Name matching.** Path separators are `.`, `:` and `-`. A segment
    matches, ignoring case, the JSON name, its lowerCamel form, the Java
    field name or a Gson alternate.
  - **Flat aliases.**
    - `enabled`, `algorithm`, `max_passes`, `max_items`,
      `save_intermediate_stages` and `ignore_net_classes` map to
      `autorouter.*`, with Java's deprecation warning.
    - `via_costs`, `plane_via_costs`, `start_ripup_costs`,
      `default_preferred_direction_trace_cost` and
      `default_undesired_direction_trace_cost` map to `scoring.*` with no
      warning. These are D8's flat cost keys (fork commit `77d6158a8`).
    - `max_threads` stays the flat top-level field, which headless routing
      never reads.
  - **Value conversion.**
    - Booleans: `"0"` → false and `"1"` → true; otherwise only `true`, in
      any case, is true.
    - Integers: `Integer.parseInt` rules.
    - Doubles: `Double.parseDouble` rules. That means surrounding
      whitespace is trimmed and a trailing `d`, `D`, `f` or `F` is
      accepted, which Rust's `parse` does not do.
    - Enums: the constant name, ignoring case.
    - String and number lists: split on `,`, trim each element; an empty
      value gives an empty list.
    - `layers.<field>=a,b`: creates an array of the list's length when
      `layers` is null; otherwise sets the first `min(len, n)` elements.
  - **CliSettings rewrites.**
    - `router.scoring.version` → `router_scoring.version`;
      `optimizer.scoring.version` → `optimizer_scoring.version`.
    - `scoring-version` sets both.
    - Version aliases: `v1`/`legacy` → `V1_LEGACY`; `v2`/`continuous` →
      `V2_CONTINUOUS` (router) or `V2_LOWER_BOUND` (optimizer);
      `lower_bound`/`lower-bound` → `V2_LOWER_BOUND`.
    - `improvement_threshold` values strictly inside (0, 1) are multiplied
      by `100f` in `f32`. This applies on the CLI only, not to env or
      JSON.
  - **Accept:**
    - `flat_cost_keys_reach_scoring` (the CLI half of `FlatCostSettingsTest`)
      is a port of `flatCliCostKeysReachTheScoringSettings` and
      `nestedCliCostKeysStillWork`.
    - Ports of `ReflectionUtilArrayTest`, `ScoringVersionSettingsTest`,
      `NeckWidthSettingsTest` and
      `SettingsMergerTest.cliOptimizerImprovementThresholdParsesAndScalesFraction`.
      Under it, `-oit 0.035` → 3.5, `4.0` → 4.0 and `0.015` → 1.5.
    - An unknown path is an error, which C07 turns into Java's warning
      text.
  - *Done 2026-10-15:* `settings/path.rs` (`SettingsPatch::set_path`,
    `cli_property`, `canonical_cli_path`, `is_deprecated_flat_autorouter_path`,
    `SetError::{UnknownPath, BadValue}`), `settings/text.rs` (Java's
    `Integer.parseInt`, `Double.parseDouble` with the `d`/`f` suffix and trimming,
    `Float.parseFloat`, `Double.toString`, `Float.toString`, `String.split`),
    `settings/cli_args.rs` (`parse_cli_arguments`: the settings half of
    `CliSettings`, which C07 replaces with the real parser). Flat aliases come
    from the table's `Flat::{Deprecated, Quiet}`.
    `tests/settings_path.rs` (21 tests): `flat_cost_keys_reach_scoring` (ports
    `flatCliCostKeysReachTheScoringSettings` and `nestedCliCostKeysStillWork`, no
    warning), `set_simple_property`, `set_nested_array_properties_when_null`,
    `..._when_initialized`, `case_insensitive_and_serialized_name_matching`
    (`ReflectionUtilArrayTest`), the five `ScoringVersionSettingsTest` methods
    that apply, `neck_width_*` (`NeckWidthSettingsTest`),
    `improvement_threshold_scales_only_inside_zero_and_one_and_only_on_the_cli`
    (`-oit 0.035` -> 3.5, `4.0` -> 4.0, `0.015` -> 1.5 also in
    `settings_merge.rs`), `unknown_paths_and_bad_values_are_errors`,
    `no_name_matches_two_fields` (every JSON name, Java name and alternate of
    every table field sets exactly that field). `tests/settings_text.rs` compares
    the text helpers with Java 25: 630 `Double.toString`, 420 `Float.toString`,
    62 parse and 36 timespan probe lines equal (it found `4.9E-324`; two hex-float
    inputs and one non-ASCII digit string are listed as expected mismatches).

- [x] **C02.4 — JSON settings files and Dan's `freerouting.json`.**
  Java: `JsonFileSettings` (reads the `"router"` member; a missing file,
  missing member, non-object or malformed JSON gives empty settings, no
  error), `RouterSettingsTypeAdapterFactory` (absorbs legacy flat keys,
  reads `layers`), `LegacyRouterSettingsBridge.absorbLegacyJson` (flat
  autorouter and cost keys apply only where the nested value is null, with
  one warning).
  - Gson's lenient mode accepts numbers given as strings.
  - Gson matches names exactly (JSON name or alternate), with no case
    folding, unlike the CLI.
  - Unknown keys are ignored.
  - Other top-level sections (`gui`, `profile`, `version`, …) are ignored.

  `JsonValue::parse` is appended to `drc/json.rs`.
  - **Accept:**
    - Ports of `JsonFileSettingsTest` (flat `max_passes` 42 → 42; nested 7
      → 7; the four empty-settings cases) and the JSON half of
      `FlatCostSettingsTest` (`flatJsonCostKeysReachTheScoringSettings`,
      `nestedJsonCostKeysWinOverFlatOnes`: flat 25 with nested 30 →
      30).
    - Port of the deserialize half of `RouterSettingsSerializationTest`.
    - `owner_settings_file_sets_start_ripup_costs`: the test data file
      copies the shape of Dan's file. Its `router` member holds empty
      `autorouter`, `fanout`, `optimizer`, `router_scoring` and
      `optimizer_scoring` objects and `"scoring": {"start_ripup_costs":
      1500}`, and the top level holds `"version": "2.5.1"` and a `gui`
      object. Resolving gives `start_ripup_costs == 1500`, with provenance
      "config file", and every other field at its default.
  - *Done 2026-10-15:* `JsonValue::parse` and `JsonParseError` appended to
    `drc/json.rs` (numbers keep their source text; strict JSON plus comments; 2 unit
    tests). `settings/json_file.rs`: `patch_from_router_json`, `patch_from_config_text`
    (the `"router"` member), `load_config_file` (default file: missing or malformed
    gives empty settings, malformed with a warning; `--settings`: an error),
    `load_config`, with Gson's string-for-number leniency, `42.0` as an int, float
    fields read as `(float) double`, exact names and alternates, `transient` fields
    ignored, and `absorbLegacyJson` (flat keys only where the nested value is unset,
    one warning).
    `tests/settings_json.rs` (16 tests): the `JsonFileSettingsTest` cases
    (`loads_max_passes_from_valid_router_section` 42,
    `..._nested_autorouter_section` 7, the four empty-settings cases),
    `flat_cost_keys_reach_scoring` and `nested_json_cost_keys_win_over_flat_ones`
    (flat 25 with nested 30 -> 30), `router_settings_deserialization` (the
    deserialize half of `RouterSettingsSerializationTest`, both payloads),
    `every_section_file`, `a_type_error_anywhere_fails_the_whole_read`, and
    `owner_settings_file_sets_start_ripup_costs` (data file
    `tests/data/settings/owner_freerouting.json` has Dan's shape: `version`
    "2.5.1", `gui`, empty sections, `scoring.start_ripup_costs = 1500`; resolving
    gives 1500 with provenance `ConfigFile` and every other table field at its
    default). Differential: `settings_java_probe.rs` scenarios `json-every`,
    `json-owner`, `json-cli-validation`, `dsn-every-json` equal Java except the
    listed null layer element.

- [x] **C02.5 — Config-file lookup.**
  `config_path::default_config_file(env: &dyn EnvLookup, os: Os) ->
  Option<PathBuf>` and `ConfigChoice { Default, UserDataPath(PathBuf),
  Explicit(PathBuf) }`. The precedence is:
  1. `--settings`;
  2. `--user_data_path=`;
  3. `FREEROUTING__USER_DATA_PATH`;
  4. the OS default.

  The Java variable `FREEROUTING__LOGGING__FILE__LOCATION` also moves the
  user-data path (`UserDataPathResolver.java:33`). Honour it after
  `FREEROUTING__USER_DATA_PATH`.
  - **Accept:**
    - Table tests with injected `APPDATA`, `HOME` and `XDG_CONFIG_HOME`
      for Windows, macOS and Linux.
    - `--settings` replaces the default file instead of stacking, checked
      with both files present and different values.
    - A missing `--settings` file is an error naming the path.
  - *Done 2026-10-15:* `settings/config_path.rs`: `Os`, `EnvLookup` (+ `ProcessEnv`),
    `default_config_file(env, os)`, `ConfigChoice::{Default, UserDataPath, Explicit}`
    with `from_args`, `config_file`; `json_file::load_config`.
    `tests/settings_config_path.rs` (6 tests): `os_defaults` (injected `APPDATA`,
    `HOME`, `XDG_CONFIG_HOME` for Windows, macOS, Linux, empty and relative XDG
    ignored), `precedence_of_the_choices` (`--settings`, `--user_data_path=`,
    `FREEROUTING__USER_DATA_PATH`, then `FREEROUTING__LOGGING__FILE__LOCATION`, then
    the OS default), `settings_flag_replaces_the_default_file` (both files exist
    with different values; the default file's `via_costs` does not leak into the
    explicit load), `a_missing_settings_file_is_an_error_naming_the_path...`
    (also `settings_json.rs::explicit_settings_file_errors_name_the_path`).

- [x] **C02.6 — Environment variables.**
  Java: `EnvironmentVariablesSource`.
  - Names are upper-cased.
  - Only names starting with `FREEROUTING__ROUTER__` count.
  - The rest of the name has `__` replaced by `.`, goes through
    `canonicalCliPath`, and is set with C02.3's setter.
  - An unknown or unparsable value gives a warning, and the field stays
    unset.
  - Java iterates `System.getenv()` in hash order, so when two variables
    set the same field, the winner depends on the platform. Rust processes
    them in sorted order, so the later name wins. Add a DIVERGENCES line.
  - **Accept:**
    - Port of `EnvironmentVariablesSourceTest`, with all its cases
      including enums, comma lists and lower-case names.
    - `env_collision_is_deterministic`.
    - The env case of `flat_cost_keys_reach_scoring`:
      `FREEROUTING__ROUTER__START_RIPUP_COSTS=1500` sets
      `scoring.start_ripup_costs`.
    - Env (55) loses to CLI (60) and beats the config file (10)
      (`SettingsMergerTest.environmentVariablesPriority`).
  - *Done 2026-10-15:* `settings/env.rs` (`parse_environment`,
    `parse_process_environment`, `EnvParse { patch, parsed, warnings }`).
    `tests/settings_env.rs` (21 tests): all 19 `EnvironmentVariablesSourceTest`
    cases (enums, comma lists, lower-case names, trace cost arrays; the
    priority/name cases are in `settings_merge.rs`), `env_collision_is_deterministic`
    (both argument orders, and `FREEROUTING...` vs `freerouting...`),
    `flat_cost_keys_reach_scoring` (`FREEROUTING__ROUTER__START_RIPUP_COSTS=1500`
    sets `scoring.start_ripup_costs`, no warning),
    `deprecated_flat_autorouter_variables_warn`,
    `every_field_is_reachable_through_its_variable` (all 76 table fields). Env (55)
    loses to CLI (60) and beats the config file (10):
    `settings_merge.rs::environment_variables_priority`. Differential:
    `settings_java_probe.rs` scenario `env` (11 variables incl. an unknown one)
    equals Java. DIVERGENCES.md has the sorted-order line.

- [x] **C02.7 — `autoroute_settings` from a DSN or rules file → patch.**
  Java: `AutorouteSettings.readScope` and `readLayerRule`; `DsnFileSettings`
  and `RulesFileSettings` (priorities 20 and 40). The patch:
  - **Seeding.** Start with `setLayerCount(n)`: every layer `routable =
    true` with the other layer fields null; both cost arrays `[1.0; n]`;
    `costs_initialized = Some(false)`.
  - `vias` → `allowed_via_types`.
  - `via_costs`, `plane_via_costs` and `start_ripup_costs` are clamped to
    at least 1.
  - **`layer_rule`.** Look the layer up by name; an unknown layer aborts
    the whole scope.
    - `active` → `routable`; `preferred_direction` sets the direction.
    - `preferred_direction_trace_costs` and
      `against_preferred_direction_trace_costs` are clamped to at least
      0.1. Each writes both the array entry and the layer field, and sets
      `costs_initialized = Some(true)`.
  - **Always** set `autorouter.enabled` from `autoroute` and
    `optimizer.enabled` from `postroute`, both on when absent.
  - A DSN *without* the scope still yields the seeded patch, with only the
    layer count.
  - **Accept:**
    - Port of `DsnFileSettingsTest`: layer count `Issue413-test.dsn` 2,
      `Issue066-Project_GP8B.dsn` 4.
    - Port of `RulesFileSettingsTest`:
      - `fixtures/Issue191-processor.Z80/processor.rules` → vias on,
        50/5/100, router and optimizer on, L0 horizontal 1.0/2.5,
        L1 vertical 1.0/1.7;
      - `fixtures/Issue029-hw48na_valid.rules` → L0 vertical 1.0/2.0,
        L1 horizontal 1.0/2.0.
    - The patches of the three DSN fixtures that carry the scope
      (`Issue103-Board-Routed.dsn`, `Issue187-processor.Z80.dsn`,
      `Issue191-processor.Z80/processor.Z80.dsn`) equal C01.1's `settings`
      dump for those fields.
  - *Done 2026-10-15:* `settings/autoroute_scope.rs` (`read_scope`,
    `patch_from_dsn`, `patch_from_rules`, `rules_layer_names`, `layer_number`);
    seeding via `SettingsPatch::set_layer_count` and the clamping setters in
    `layers.rs`. `tests/settings_autoroute_scope.rs` (14 tests): both
    `DsnFileSettingsTest` layer counts (`Issue413-test.dsn` 2,
    `Issue066-Project_GP8B.dsn` 4), `dsn_without_the_scope_yields_only_the_seed`,
    both `RulesFileSettingsTest` cases (processor.rules: vias on, 50/5/100, router
    and optimizer on, L0 horizontal 1.0/2.5, L1 vertical 1.0/1.7; hw48na_valid:
    L0 vertical 1.0/2.0, L1 horizontal 1.0/2.0), clamps, unknown-layer abort,
    `Top`/`Bottom` lookup, rules layer discovery, a layer-count sweep over all (more than 100)
    parseable fixture DSNs and every rules fixture. Third clause: the patches of
    `Issue103-Board-Routed.dsn`, `Issue187-processor.Z80.dsn` and
    `Issue191-processor.Z80/processor.Z80.dsn` merged with the defaults equal a
    direct Java 25 run at the pin (`settings_java_probe.rs`, scenarios
    `dsn-Issue103...`, `dsn-Issue187...`, `dsn-Issue191...`: layers, both cost
    arrays, `costs_initialized`, run flags, via costs).
  - *Done 2026-10-15 (third clause):* `settings_reference.rs::settings_match_java_dumps`
    now runs against C01.1's `fixtures/reference/settings/**` and
    `cases/settings/**` (present in the working tree, written by C01's
    `ReferenceDump settings`): the DSN-derived settings of every fixture DSN
    (including the three that carry the scope) equal the dump in all four
    scenarios; see C02.8 for the numbers.

- [x] **C02.8 — Validation, test profiles and the comparison dump.**
  Java: `RouterSettings.validateAgainstBoard(null)` as `merge()` runs it.
  - **Clamps:**
    - `max_passes` < 0, or > 9999 and ≠ `i32::MAX`, becomes 0 (unlimited);
      so `-mp 10000` means unlimited.
    - A null or negative `max_threads` becomes cores − 1; a value above
      `cores` becomes `cores`.
    - `trace_pull_tight_accuracy` < 1 becomes 500.
    - A layer `bend_cost` is clamped to [0, 9.9].
    - Layer trace costs below 0.1 become 0.1.
  - **Warnings.** Each clamp gives Java's warning text in
    `validation_warnings`.
  - **Unknown `ignore_net_classes` names.** Java checks these only when a
    board is given (the API path), so the CLI never warns. Rust warns in
    C03.5, which is an addition with no DIVERGENCES line, since it only
    logs.
  - **Profiles** (`settings/profiles.rs`):
    - `java_fixture_test()` is Java's `TestingSettings`: a priority-80
      patch with `copper_to_edge_clearance_um = 0.0`. Its `max_passes 100`
      applies only when nothing else set it (first write wins). It leaves
      the optimizer on. A14's ports of Java fixture tests use it.
    - `java_cli_default()` is the empty profile, which C01's `default`
      scenario models.
  - **Dump.** `settings::dump(&Resolved) -> JsonValue` writes the static
    part of C01.1's `settings` JSON shape: the same keys, with
    `max_threads` normalized the same way.
  - **Accept:**
    - A table test of every clamp and its warning text.
    - `java_fixture_profile` asserts edge clearance 0 and optimizer on.
    - `settings_reference.rs` compares the static fields with C01.1 for
      the `default`, `fixture`, `cli` and `owner-config` scenarios on
      every fixture without an adjacent rules file. "Static" means
      everything except `layers`, the cost arrays and `costs_initialized`,
      which C03 compares after tuning. The five fixtures with an adjacent
      rules file, and the `explicit-cost-rules` case, are listed in the
      test as waiting on C04.
      The `cli` scenario patch is built here through C02.3 from C01's
      fixed argument list. C07 replaces that with the real parser. Any
      mismatch is listed in the test with its reason.
  - *Done 2026-10-15:* `settings/validate.rs` (`validate`), `settings/profiles.rs`
    (`java_fixture_test`, `TestingProfile` with Java's `TestingSettings` setters,
    first write wins for `set_max_passes`/`set_job_timeout`/`set_enabled`;
    `java_cli_default`), `settings/dump.rs` (`dump(&Resolved)`, `dump_patch`),
    `tests/settings_reference.rs` (12 tests): `max_passes_clamps` (also `-mp 10000`
    -> unlimited), `max_threads_clamps`, `trace_pull_tight_accuracy_clamps`,
    `layer_costs_clamp` (every clamp with Java's exact warning text, checked
    against Java's output in `settings_java_probe.rs::json-cli-validation`),
    `unknown_ignore_net_classes_never_warn_without_a_board`,
    `java_fixture_profile` (edge clearance 0, optimizer on, priority 80 beats the
    CLI), `dump_has_the_gson_keys_and_normalised_threads`. The comparison harness
    (`settings_match_java_dumps`: scenarios `default`, `fixture`, `cli`,
    `owner-config`; the two merges of `RoutingJobScheduler`; the `cli` patch built
    through C02.3 from C01's fixed argument list; boards with an adjacent rules file
    listed as waiting on C04; `EXPECTED_MISMATCHES`) is written and its tree diff is
    tested on a hand-written tree.
  - *Done 2026-10-15 (comparison):* C01.1's dumps landed in the working tree
    (`fixtures/reference/settings/` 131 files, `cases/settings/` 4). `cargo test -p
    freerouting --test settings_reference settings_match -- --nocapture` compares
    **464 board scenarios** (`default`, `fixture`, `cli`, `owner-config` on 131
    fixture DSNs and 4 synthetic cases: static settings = everything except `layers`,
    the cost arrays and `costs_initialized`, after the scheduler's two merges, the
    `cli` patch built through C02.3 from C01's argument list) with **0 mismatches**
    (`EXPECTED_MISMATCHES` is empty) and **20 scenarios waiting on C04**: the boards
    with an adjacent rules file, listed in the test as `WAITING_ON_C04`
    (`Issue029-hw48na`, `Issue107-freq_teiler_200kHz_kicad`,
    `Issue107-freq_teiler_200kHz_kicad_bad`, `Issue593-BBD_Mars-64`,
    `explicit-cost-rules`). The dump files are uncommitted in the tree; if C01
    changes their shape the test says so (it also runs without them, then it
    prints "nothing compared"). Also ran: `settings_java_probe.rs`, 18 scenarios
    against Java 25 at `cda540f` (defaults, CLI, env, JSON, DSN, rules, `Api` second
    merge, validation clamps), 17 equal and one listed.
