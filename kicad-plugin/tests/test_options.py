import json
from dataclasses import replace

import options
import pytest
from options import RouteOptions


def test_defaults_pass_only_the_board_minimum_width():
    assert RouteOptions().to_args() == []
    assert RouteOptions().to_args(board_min_trace_width_um=150.0) == ["--router.min_trace_width_um=150"]
    assert RouteOptions().to_args(board_min_trace_width_um=127.5) == ["--router.min_trace_width_um=127.5"]
    # A board without a minimum, or a zero one, adds nothing.
    assert RouteOptions().to_args(board_min_trace_width_um=0) == []


@pytest.mark.parametrize(
    "kwargs, expected",
    [
        ({"max_passes": 30}, ["--max-passes", "30"]),
        ({"max_passes": 9999}, ["--max-passes", "9999"]),
        ({"threads": 4}, ["--threads", "4"]),
        ({"optimizer": False}, ["--router.optimizer.enabled=false"]),
        ({"improvement_threshold": 5.0}, ["--router.optimizer.improvement_threshold=5"]),
        ({"improvement_threshold": 2.5}, ["--router.optimizer.improvement_threshold=2.5"]),
        ({"ignore_net_classes": ["GND", "Power"]}, ["--router.autorouter.ignore_net_classes=GND,Power"]),
        ({"ignore_net_classes": ["Power,Default", "GND"]}, ["-inc", "Power\\,Default,GND"]),
        ({"seed": 7}, ["--seed", "7"]),
        ({"min_trace_width_um": 200.0}, ["--router.min_trace_width_um=200"]),
        ({"timeout": "10m"}, ["--timeout", "10m"]),
        ({"rule_area_fixup": False}, []),  # a plugin option, not a router flag
    ],
)
def test_each_option_has_its_golden_argument_list(kwargs, expected):
    assert RouteOptions(**kwargs).to_args() == expected


def test_every_option_set_has_a_fixed_order():
    everything = RouteOptions(
        max_passes=30,
        threads=4,
        optimizer=False,
        improvement_threshold=5.0,
        ignore_net_classes=["GND", "Power"],
        seed=3,
        min_trace_width_um=200.0,
        timeout="1:30:00",
        rule_area_fixup=False,
    )
    golden = [
        "--max-passes", "30",
        "--threads", "4",
        "--router.optimizer.enabled=false",
        "--router.optimizer.improvement_threshold=5",
        "--router.autorouter.ignore_net_classes=GND,Power",
        "--seed", "3",
        "--router.min_trace_width_um=200",
        "--timeout", "1:30:00",
    ]  # fmt: skip
    assert everything.to_args(board_min_trace_width_um=150.0) == golden
    # The option beats the board's value; the GUI does not read --seed.
    assert "--seed" not in everything.to_args(for_gui=True)
    assert everything.to_args(for_gui=True) == [a for a in golden if a not in ("--seed", "3")]
    # An unset width falls back to the board's.
    everything.min_trace_width_um = None
    assert everything.to_args(board_min_trace_width_um=150.0)[-3] == "--router.min_trace_width_um=150"


def test_net_class_names_round_trip_commas():
    for names in (["GND"], ["a,b", "c"], ["x y", "z,,w"]):
        assert options.split_net_classes(options.join_net_classes(names)) == names
    assert options.split_net_classes("Power\\,Default, GND ,,") == ["Power,Default", "GND"]
    assert options.join_net_classes(["Power,Default", "GND"]) == "Power\\,Default,GND"


# --- saving and restoring ---------------------------------------------------


def test_save_load_save_gives_equal_json(tmp_path):
    chosen = RouteOptions(
        max_passes=12,
        threads=2,
        optimizer=False,
        improvement_threshold=4.0,
        ignore_net_classes=["A", "B,C"],
        seed=0,
        min_trace_width_um=180.0,
        timeout="30m",
        rule_area_fixup=False,
    )
    first, second = tmp_path / "a" / "route-options.json", tmp_path / "b" / "route-options.json"
    options.save(first, chosen)
    loaded = options.load(first)
    assert loaded == chosen
    options.save(second, loaded)
    assert first.read_text(encoding="utf-8") == second.read_text(encoding="utf-8")
    assert json.loads(first.read_text(encoding="utf-8"))["schema"] == options.SCHEMA == 6


def test_defaults_round_trip_too(tmp_path):
    path = tmp_path / "o.json"
    options.save(path, RouteOptions())
    assert options.load(path) == RouteOptions()
    options.save(path, options.profile_defaults())
    assert options.load(path) == options.profile_defaults()


def test_a_damaged_stored_dict_restores_defaults_without_an_exception():
    good = RouteOptions(max_passes=20, threads=3, optimizer=False, timeout="5m").to_dict()
    stored = dict(good)
    del stored["threads"]  # a removed key
    stored["colour"] = "blue"  # an unknown key
    stored["max_passes"] = 123456  # out of range
    stored["timeout"] = "soon"  # not a duration
    stored["improvement_threshold"] = 0.5  # below the range
    stored["seed"] = "x"
    restored = RouteOptions.from_dict(stored)
    assert restored.threads == 0  # the default for the removed key
    assert restored.max_passes == 0  # the default for the bad value
    assert restored.timeout == ""
    assert restored.improvement_threshold is None
    assert restored.seed == 0
    assert restored.optimizer is False  # the valid field survives

    # Another schema: nothing is trusted.
    future = dict(good, schema=99)
    assert RouteOptions.from_dict(future) == options.profile_defaults()
    # Not even a dict.
    for junk in (None, [], "x", 3, {"schema": "1"}):
        assert RouteOptions.from_dict(junk) == options.profile_defaults()


def test_booleans_are_not_numbers():
    assert RouteOptions.from_dict({"schema": 1, "max_passes": True}).max_passes == 0
    assert RouteOptions.from_dict({"schema": 1, "threads": 2.5}).threads == 0
    assert RouteOptions.from_dict({"schema": 1, "optimizer": 1}).optimizer is True
    assert RouteOptions.from_dict({"schema": 1, "ignore_net_classes": ["a", 3]}).ignore_net_classes == []


def test_load_of_missing_or_broken_files(tmp_path):
    # The default profile's options (K13.7: the snapshot, and K09.9's sweep)
    expected = options.profile_defaults(options.DEFAULT_PROFILE)
    assert options.load(tmp_path / "missing.json") == expected
    broken = tmp_path / "broken.json"
    broken.write_text("{not json", encoding="utf-8")
    assert options.load(broken) == expected


def test_rule_area_fixup_comes_from_the_k01_setting_until_options_are_saved(tmp_path):
    work = tmp_path / "freerouting-rs"
    assert options.load_for_board(work, {"fix_rule_areas": False}).rule_area_fixup is False
    assert options.load_for_board(work, None).rule_area_fixup is True
    options.save(options.options_path(work), RouteOptions(rule_area_fixup=True))
    assert options.load_for_board(work, {"fix_rule_areas": False}).rule_area_fixup is True


# --- the dialog's form ------------------------------------------------------


def test_the_form_builds_options_and_reports_every_bad_field():
    chosen, errors = options.from_form(
        {
            "max_passes": 5,
            "threads": 0,
            "optimizer": True,
            "improvement_threshold": "",
            "ignore_net_classes": "GND, Power",
            "seed": 0,
            "min_trace_width_um": "150",
            "timeout": "10m",
            "rule_area_fixup": False,
        }
    )
    assert errors == []
    assert chosen == replace(
        options.profile_defaults(),
        max_passes=5,
        ignore_net_classes=["GND", "Power"],
        min_trace_width_um=150.0,
        timeout="10m",
        rule_area_fixup=False,
    )
    _, errors = options.from_form(
        {"max_passes": 99999, "improvement_threshold": "abc", "min_trace_width_um": "-3", "timeout": "10 minutes"}
    )
    assert len(errors) == 4, errors


@pytest.mark.parametrize("text", ["", "90", "90s", "10m", "2H", "5:30", "1:02:03"])
def test_valid_timeouts(text):
    assert options.valid_timeout(text)


@pytest.mark.parametrize("text", ["soon", "-5", "1:2:3:4", "m", "1.5h", None, 5])
def test_invalid_timeouts(text):
    assert not options.valid_timeout(text)


# --- the profile and the stage choices (N01.3, N01.4) -------------------------

LISTING = {
    "slots": [
        {"key": "router.autorouter.connection_router", "label": "Connection router", "arity": "one",
         "ids": ["freerouting-connection", "x-grid"], "default": ["freerouting-connection"]},
        {"key": "router.special.length_matching", "label": "Length matching", "arity": "optional",
         "ids": ["krt-meander", "x-lab"], "default": [], "experimental_ids": ["x-lab"]},
        {"key": "router.special.diagnostics", "label": "Diagnostics", "arity": "list", "ids": ["verdicts"]},
    ]
}  # fmt: skip


@pytest.mark.parametrize("profile, expected", [("legacy", ["--profile", "legacy"]), ("enhanced", [])])
def test_each_profile_has_its_argument_list(profile, expected):
    assert RouteOptions(profile=profile).to_args() == expected
    assert RouteOptions(profile=profile).to_args(for_gui=True) == expected


def test_stage_choices_become_router_flags_in_key_order():
    chosen = RouteOptions(profile="legacy", max_passes=3, stages={"special.length_matching": "krt-meander", "autorouter.connection_router": "x-grid"})
    assert chosen.to_args() == [
        "--profile", "legacy",
        "--max-passes", "3",
        "--router.autorouter.connection_router=x-grid",
        "--router.special.length_matching=krt-meander",
    ]  # fmt: skip


def test_the_listing_gives_the_single_choice_slots():
    slots = options.parse_stage_listing(LISTING)
    assert slots == [
        ("autorouter.connection_router", "Connection router", ["freerouting-connection", "x-grid"], "freerouting-connection"),
        ("special.length_matching", "Length matching", ["", "krt-meander", "x-lab"], ""),
    ]  # fmt: skip
    assert options.stage_defaults(slots) == {"autorouter.connection_router": "freerouting-connection", "special.length_matching": ""}
    for junk in (None, [], {"slots": 3}, {"slots": [3, {"key": 5}, {"arity": "one"}]}):
        assert options.parse_stage_listing(junk) == []


def test_the_advanced_group_marks_experimental_stage_ids():
    found = options.experimental_stage_ids(LISTING)
    assert found == {"special.length_matching": {"x-lab"}}
    labels = [options.stage_label(i, found["special.length_matching"]) for i in ["", "krt-meander", "x-lab"]]
    assert labels == ["(none)", "krt-meander", "x-lab (experimental)"]
    for junk in (None, [], {"slots": 3}, {"slots": [3, {"key": "router.a", "experimental_ids": "x"}]}):
        assert options.experimental_stage_ids(junk) == {}


def test_the_options_file_stores_only_the_differences(tmp_path):
    defaults = options.stage_defaults(options.parse_stage_listing(LISTING))
    path = tmp_path / "route-options.json"
    # Everything at the profile's value: only the schema and the profile.
    at_profile = replace(options.profile_defaults(), stages={"autorouter.connection_router": "freerouting-connection"})
    options.save(path, at_profile, defaults)
    assert json.loads(path.read_text(encoding="utf-8")) == {"schema": options.SCHEMA, "profile": "enhanced"}
    assert options.load(path) == options.profile_defaults()
    # One option and one stage differ: only those are stored.
    chosen = RouteOptions(
        profile="legacy",
        max_passes=12,
        stages={"autorouter.connection_router": "x-grid", "special.length_matching": ""},
    )
    options.save(path, chosen, defaults)
    assert json.loads(path.read_text(encoding="utf-8")) == {
        "schema": options.SCHEMA,
        "profile": "legacy",
        "max_passes": 12,
        "stages": {"autorouter.connection_router": "x-grid"},
    }
    loaded = options.load(path)
    assert loaded == RouteOptions(profile="legacy", max_passes=12, stages={"autorouter.connection_router": "x-grid"})
    # Saved again, the file is the same.
    first = path.read_text(encoding="utf-8")
    options.save(path, loaded, defaults)
    assert path.read_text(encoding="utf-8") == first


def test_a_schema_1_file_loads_as_enhanced():
    old = {"schema": 1, "max_passes": 7, "threads": 0, "optimizer": True, "improvement_threshold": None,
           "ignore_net_classes": [], "seed": 0, "min_trace_width_um": None, "timeout": "", "rule_area_fixup": False}  # fmt: skip
    restored = RouteOptions.from_dict(old)
    assert restored == replace(options.profile_defaults("enhanced"), max_passes=7, rule_area_fixup=False)
    assert restored.profile == "enhanced"


def test_a_bad_profile_or_stage_is_dropped_on_load_and_refused_by_the_form():
    restored = RouteOptions.from_dict(
        {"schema": 2, "profile": "best", "stages": {"autorouter.connection_router": "x grid", "BAD": "a", "special.diff_pairs": "krt-diffpair"}}
    )
    assert restored.profile == "enhanced"
    assert restored.stages == {"special.diff_pairs": "krt-diffpair"}
    chosen, errors = options.from_form({"profile": "Legacy", "stages": {"autorouter.connection_router": "x-grid", "special.length_matching": ""}})
    assert errors == [] and chosen.profile == "legacy"
    assert chosen.stages == {"autorouter.connection_router": "x-grid"}
    _, errors = options.from_form({"profile": "best", "stages": {"autorouter.ordering": "a b"}})
    assert errors == [
        "profile: 'best' is not a profile (use legacy or enhanced)",
        "autorouter.ordering: 'a b' is not a stage id",
    ]


# --- K09 ---------------------------------------------------------------------


@pytest.mark.parametrize(
    "kwargs, expected",
    [
        ({"start_ripup_costs": 1500}, ["--router.scoring.start_ripup_costs=1500"]),
        ({"sweep": "free"}, ["--router.autorouter.sweep=free"]),
        ({"sweep": "fixed", "sweep_passes": 2}, ["--router.autorouter.sweep=fixed", "--router.autorouter.sweep_passes=2"]),
        ({"sweep_passes": 2}, []),  # passes without a sweep do nothing
        ({"pass_hint": 3}, []),  # remembered, used only when asked
        ({"pass_hint": 3, "use_pass_hint": True}, ["--max-passes", "3"]),
        ({"pass_hint": 3, "use_pass_hint": True, "max_passes": 5}, ["--max-passes", "5"]),
        ({"class_widths": {"Power": 0.3}, "fixed_pour_classes": ["Power"], "drop_pour_nets": ["GND"]}, []),
        ({"fix_existing": True, "route_on_plane_layers": ["In2.Cu"], "edge_and_pad_keepouts": False}, []),
    ],
)
def test_each_k09_option_has_its_golden_argument_list(kwargs, expected):
    assert RouteOptions(**kwargs).to_args() == expected


def test_the_fix_ups_engine_flags_go_before_the_stages():
    args = RouteOptions(sweep="free", stages={"autorouter.connection_router": "x"}).to_args(
        150, extra_args=["--router.plane_obstacle_net_classes=Power_High"]
    )
    assert args == [
        "--router.min_trace_width_um=150",
        "--router.autorouter.sweep=free",
        "--router.plane_obstacle_net_classes=Power_High",
        "--router.autorouter.connection_router=x",
    ]


def test_the_k09_options_round_trip(tmp_path):
    from conftest import canary_preset_options

    chosen = canary_preset_options()
    chosen.fix_existing, chosen.pass_hint = True, 4
    chosen.route_on_plane_layers = ["Power.Cu"]
    chosen.fixed_pour_mode = "engine"
    path = tmp_path / "route-options.json"
    options.save(path, chosen)
    assert options.load(path) == chosen
    stored = json.loads(path.read_text(encoding="utf-8"))
    assert stored["class_widths"] == {"Power_High": 0.3, "Switch_Node": 0.3, "Power": 0.3, "GND": 0.4}
    assert stored["fixed_pour_classes"] == ["Power_High", "Switch_Node"] and stored["pass_hint"] == 4
    # "auto" round-trips as itself
    auto = RouteOptions(class_widths="auto", fixed_pour_classes="auto", drop_pour_nets="auto")
    options.save(path, auto)
    assert options.load(path) == auto


def test_bad_k09_values_keep_the_profile_default():
    bad = {
        "schema": 3,
        "profile": "legacy",
        "class_widths": {"Power": -1},
        "fixed_pour_classes": "some",
        "fixed_pour_mode": "magic",
        "drop_pour_nets": [1],
        "edge_and_pad_keepouts": "yes",
        "start_ripup_costs": 0,
        "sweep": "always",
        "sweep_passes": 0,
        "pass_hint": True,
    }
    assert RouteOptions.from_dict(bad) == options.profile_defaults("legacy")


def test_enhanced_keeps_the_board_setup_widths_and_leaves_the_neck_down_to_the_engine():
    # N05.3 (docs/roadmap/prereg/N05-neck-down.json): no class narrowing by
    # default; the engine's `enhanced` profile sets router.neck_width_um itself.
    enhanced = options.profile_defaults("enhanced")
    assert enhanced.class_widths == {}
    assert options.dsn_settings(enhanced)["class_widths"] == {}
    assert not any("neck_width" in a for a in enhanced.to_args(board_min_trace_width_um=150))


def test_enhanced_adds_the_completion_sweep_and_legacy_does_not():
    # K09.9 (docs/roadmap/prereg/K09.9-rerun.json, arm `+sweep`): a default
    # enhanced run sweeps `free` with the router's 2 passes, no pass limit,
    # no ripup cost and no pour fix-up; legacy keeps the plugin's defaults.
    enhanced = options.profile_defaults("enhanced")
    assert enhanced.sweep == "free" and enhanced.sweep_passes is None
    assert enhanced.to_args(board_min_trace_width_um=150) == [
        "--router.min_trace_width_um=150",
        "--router.autorouter.sweep=free",
    ]
    assert options.dsn_settings(enhanced) == options.dsn_settings(RouteOptions())
    legacy = options.profile_defaults("legacy")
    assert legacy == RouteOptions(profile="legacy")
    assert legacy.to_args(board_min_trace_width_um=150) == ["--profile", "legacy", "--router.min_trace_width_um=150"]


def test_a_sweep_turned_off_under_enhanced_is_stored_and_restored(tmp_path):
    path = tmp_path / "route-options.json"
    off = replace(options.profile_defaults("enhanced"), sweep="")
    options.save(path, off)
    assert json.loads(path.read_text(encoding="utf-8")) == {"schema": options.SCHEMA, "profile": "enhanced", "sweep": ""}
    assert options.load(path) == off
    assert not any("sweep" in a for a in off.to_args(board_min_trace_width_um=150))
    # A file saved before the default changed (no `sweep` key) takes the sweep.
    assert RouteOptions.from_dict({"schema": 4, "profile": "enhanced"}).sweep == "free"


def test_schema_2_files_load_and_take_the_new_defaults():
    old = {"schema": 2, "profile": "enhanced"}
    assert RouteOptions.from_dict(old) == options.profile_defaults("enhanced")
    assert options.profile_defaults("enhanced").edge_and_pad_keepouts  # a board rule
    assert options.profile_defaults("legacy").edge_and_pad_keepouts


def test_the_form_reads_the_k09_fields():
    chosen, errors = options.from_form(
        {
            "board_input": "dsn",  # these are the DSN's fix-ups
            "class_widths": "Power_High=0.3, GND=0.4",
            "fixed_pour_classes": "Power_High, Switch_Node",
            "fixed_pour_mode": "engine",
            "drop_pour_nets": "auto",
            "edge_and_pad_keepouts": False,
            "route_on_plane_layers": "Power.Cu",
            "fix_existing": True,
            "start_ripup_costs": "1500",
            "sweep": "free",
            "sweep_passes": "2",
            "use_pass_hint": True,
        }
    )
    assert errors == []
    assert chosen.class_widths == {"Power_High": 0.3, "GND": 0.4}
    assert chosen.fixed_pour_classes == ["Power_High", "Switch_Node"] and chosen.drop_pour_nets == "auto"
    assert (chosen.fixed_pour_mode, chosen.edge_and_pad_keepouts, chosen.route_on_plane_layers) == ("engine", False, ["Power.Cu"])
    assert (chosen.fix_existing, chosen.start_ripup_costs, chosen.sweep, chosen.sweep_passes, chosen.use_pass_hint) == (
        True, 1500, "free", 2, True,
    )  # fmt: skip
    _, errors = options.from_form({"board_input": "dsn", "class_widths": "Power_High", "sweep": "sometimes", "start_ripup_costs": "x"})
    assert len(errors) == 3


def test_class_widths_text_round_trips():
    widths, error = options.parse_class_widths(options.format_class_widths({"GND": 0.4, "Power_High": 0.3}))
    assert error is None and widths == {"GND": 0.4, "Power_High": 0.3}
    assert options.parse_class_widths("AUTO") == ("auto", None)
    assert options.parse_class_widths("") == ({}, None)


def test_board_minimums_become_hard_floors():
    """Q18: Board Setup's minimums pass as the router's board minimums and
    copper-to-edge clearance; unknown or non-positive values pass nothing."""
    from options import board_minimum_args

    assert board_minimum_args({}) == []
    assert board_minimum_args(
        {
            "m_CopperEdgeClearance": 500000,
            "m_HoleClearance": 250000,
            "m_MinThroughDrill": 300000,
            "m_TrackMinWidth": 200000,
            "m_ViasMinAnnularWidth": 100000,
            "m_ViasMinSize": 500000,
        }
    ) == [
        "--router.board_minimums.track_width_um=200",
        "--router.board_minimums.via_diameter_um=500",
        "--router.board_minimums.via_drill_um=300",
        "--router.board_minimums.via_annular_um=100",
        "--router.copper_to_edge_clearance_um=500",
        "--router.hole_clearance_um=250",
    ]
    assert board_minimum_args({"m_TrackMinWidth": 0, "m_ViasMinSize": None, "m_MinThroughDrill": 127000}) == [
        "--router.board_minimums.via_drill_um=127"
    ]


def test_router_args_carry_the_board_minimums():
    import types

    import export_flow

    board = types.SimpleNamespace(
        GetDesignSettings=lambda: types.SimpleNamespace(m_TrackMinWidth=150000, m_CopperEdgeClearance=500000)
    )
    assert export_flow.router_args(board, RouteOptions()) == [
        "--router.min_trace_width_um=150",
        "--router.board_minimums.track_width_um=150",
        "--router.copper_to_edge_clearance_um=500",
    ]


# --- Q21: the straight pin escape ------------------------------------------------


def test_pin_escape_is_off_under_both_profiles_and_passes_its_flag_when_on():
    for profile in options.PROFILES:
        opts = options.profile_defaults(profile)
        assert opts.pin_escape is False
        assert not any("pin_escape" in a for a in opts.to_args(board_min_trace_width_um=200))
    on = replace(options.profile_defaults("enhanced"), pin_escape=True)
    args = on.to_args(board_min_trace_width_um=200)
    assert "--router.fanout.pin_escape=straight" in args
    # Before the fix-ups' engine flags and the stage choices, after the sweep.
    assert args.index("--router.autorouter.sweep=free") < args.index("--router.fanout.pin_escape=straight")
    assert "--router.fanout.pin_escape=straight" in on.to_args(for_gui=True)


def test_pin_escape_is_saved_per_board_and_restored(tmp_path):
    path = tmp_path / "route-options.json"
    on = replace(options.profile_defaults("enhanced"), pin_escape=True)
    options.save(path, on)
    stored = json.loads(path.read_text(encoding="utf-8"))
    assert stored == {"schema": 6, "profile": "enhanced", "pin_escape": True}
    assert options.load(path) == on
    # Off is the default and is left out of the file.
    options.save(path, options.profile_defaults("legacy"))
    assert "pin_escape" not in json.loads(path.read_text(encoding="utf-8"))
    # A schema 4 file (before Q21) loads with the option off; a bad value keeps it off.
    assert RouteOptions.from_dict({"schema": 4, "profile": "enhanced"}).pin_escape is False
    assert RouteOptions.from_dict({"schema": 5, "profile": "enhanced", "pin_escape": "yes"}).pin_escape is False
    assert RouteOptions.from_dict({"schema": 5, "profile": "legacy", "pin_escape": True}).pin_escape is True
    assert 5 in options.READABLE_SCHEMAS and 4 in options.READABLE_SCHEMAS


def test_the_form_sets_pin_escape():
    chosen, errors = options.from_form({"profile": "enhanced", "pin_escape": True})
    assert errors == [] and chosen.pin_escape is True
    chosen, errors = options.from_form({"profile": "enhanced"})
    assert errors == [] and chosen.pin_escape is False


# --- K15.14: every signal-layer pour kept clear -----------------------------------

ALL_POURS_FLAG = "--router.plane_obstacle_all_pours=true"


def test_all_pours_is_off_under_both_profiles_and_passes_its_flag_only_when_on():
    for profile in options.PROFILES:
        opts = options.profile_defaults(profile)
        assert opts.plane_obstacle_all_pours is False
        assert not any("plane_obstacle_all_pours" in a for a in opts.to_args(board_min_trace_width_um=200))
    on = replace(options.profile_defaults("enhanced"), plane_obstacle_all_pours=True)
    args = on.to_args(board_min_trace_width_um=200)
    assert args.count(ALL_POURS_FLAG) == 1
    assert ALL_POURS_FLAG in on.to_args(for_gui=True)
    # Nothing else changes: the arguments differ by that one flag.
    off = options.profile_defaults("enhanced").to_args(board_min_trace_width_um=200)
    assert [a for a in args if a != ALL_POURS_FLAG] == off


def test_all_pours_is_saved_per_board_and_restored_without_a_schema_bump(tmp_path):
    path = tmp_path / "route-options.json"
    on = replace(options.profile_defaults("enhanced"), plane_obstacle_all_pours=True)
    options.save(path, on)
    stored = json.loads(path.read_text(encoding="utf-8"))
    # A missing key is the default, so the schema stays 6 and older files load unchanged.
    assert stored == {"schema": 6, "profile": "enhanced", "plane_obstacle_all_pours": True}
    assert options.load(path) == on
    options.save(path, options.profile_defaults("legacy"))
    assert "plane_obstacle_all_pours" not in json.loads(path.read_text(encoding="utf-8"))
    # Every schema before the option loads with it off; a bad value keeps it off.
    for schema in options.READABLE_SCHEMAS:
        assert RouteOptions.from_dict({"schema": schema, "profile": "enhanced"}).plane_obstacle_all_pours is False
    bad = {"schema": 6, "profile": "enhanced", "plane_obstacle_all_pours": "yes"}
    assert RouteOptions.from_dict(bad).plane_obstacle_all_pours is False
    on_legacy = {"schema": 6, "profile": "legacy", "plane_obstacle_all_pours": True}
    assert RouteOptions.from_dict(on_legacy).plane_obstacle_all_pours is True


def test_the_form_sets_all_pours():
    chosen, errors = options.from_form({"profile": "enhanced", "plane_obstacle_all_pours": True})
    assert errors == [] and chosen.plane_obstacle_all_pours is True
    chosen, errors = options.from_form({"profile": "enhanced"})
    assert errors == [] and chosen.plane_obstacle_all_pours is False


# --- K15.30: the search aims for a pour's opened fill ----------------------------------

FILL_TARGET_FLAG = "--router.pour_fill_target=true"


def test_fill_target_is_off_under_both_profiles_and_passes_its_flag_only_when_on():
    for profile in options.PROFILES:
        opts = options.profile_defaults(profile)
        assert opts.pour_fill_target is False
        assert not any("pour_fill_target" in a for a in opts.to_args(board_min_trace_width_um=200))
    on = replace(options.profile_defaults("enhanced"), pour_fill_target=True)
    args = on.to_args(board_min_trace_width_um=200)
    assert args.count(FILL_TARGET_FLAG) == 1
    assert FILL_TARGET_FLAG in on.to_args(for_gui=True)
    # Nothing else changes: the arguments differ by that one flag.
    off = options.profile_defaults("enhanced").to_args(board_min_trace_width_um=200)
    assert [a for a in args if a != FILL_TARGET_FLAG] == off
    # It is independent of K15.14's option.
    both = replace(on, plane_obstacle_all_pours=True).to_args(board_min_trace_width_um=200)
    assert FILL_TARGET_FLAG in both and ALL_POURS_FLAG in both


def test_fill_target_is_saved_per_board_and_restored_without_a_schema_bump(tmp_path):
    path = tmp_path / "route-options.json"
    on = replace(options.profile_defaults("enhanced"), pour_fill_target=True)
    options.save(path, on)
    stored = json.loads(path.read_text(encoding="utf-8"))
    # A missing key is the default, so the schema stays 6 and older files load unchanged.
    assert stored == {"schema": 6, "profile": "enhanced", "pour_fill_target": True}
    assert options.load(path) == on
    options.save(path, options.profile_defaults("legacy"))
    assert "pour_fill_target" not in json.loads(path.read_text(encoding="utf-8"))
    # Every schema before the option loads with it off; a bad value keeps it off.
    for schema in options.READABLE_SCHEMAS:
        assert RouteOptions.from_dict({"schema": schema, "profile": "enhanced"}).pour_fill_target is False
    bad = {"schema": 6, "profile": "enhanced", "pour_fill_target": "yes"}
    assert RouteOptions.from_dict(bad).pour_fill_target is False
    on_legacy = {"schema": 6, "profile": "legacy", "pour_fill_target": True}
    assert RouteOptions.from_dict(on_legacy).pour_fill_target is True


def test_the_form_sets_fill_target():
    chosen, errors = options.from_form({"profile": "enhanced", "pour_fill_target": True})
    assert errors == [] and chosen.pour_fill_target is True
    chosen, errors = options.from_form({"profile": "enhanced"})
    assert errors == [] and chosen.pour_fill_target is False


# --- K15.37: the island of a pour counts as its own connection -------------------------

CONTACTS_FLAG = "--router.pour_fill_contacts=true"


def test_fill_contacts_is_off_under_both_profiles_and_passes_its_flag_only_when_on():
    for profile in options.PROFILES:
        opts = options.profile_defaults(profile)
        assert opts.pour_fill_contacts is False
        assert not any("pour_fill_contacts" in a for a in opts.to_args(board_min_trace_width_um=200))
    on = replace(options.profile_defaults("enhanced"), pour_fill_contacts=True)
    args = on.to_args(board_min_trace_width_um=200)
    assert args.count(CONTACTS_FLAG) == 1
    assert CONTACTS_FLAG in on.to_args(for_gui=True)
    # Nothing else changes: the arguments differ by that one flag.
    off = options.profile_defaults("enhanced").to_args(board_min_trace_width_um=200)
    assert [a for a in args if a != CONTACTS_FLAG] == off
    # It is independent of K15.30's option.
    both = replace(on, pour_fill_target=True).to_args(board_min_trace_width_um=200)
    assert CONTACTS_FLAG in both and FILL_TARGET_FLAG in both


def test_fill_contacts_is_saved_per_board_and_restored_without_a_schema_bump(tmp_path):
    path = tmp_path / "route-options.json"
    on = replace(options.profile_defaults("enhanced"), pour_fill_contacts=True)
    options.save(path, on)
    stored = json.loads(path.read_text(encoding="utf-8"))
    # A missing key is the default, so the schema stays 6 and older files load unchanged.
    assert stored == {"schema": 6, "profile": "enhanced", "pour_fill_contacts": True}
    assert options.load(path) == on
    options.save(path, options.profile_defaults("legacy"))
    assert "pour_fill_contacts" not in json.loads(path.read_text(encoding="utf-8"))
    # Every schema before the option loads with it off; a bad value keeps it off.
    for schema in options.READABLE_SCHEMAS:
        assert RouteOptions.from_dict({"schema": schema, "profile": "enhanced"}).pour_fill_contacts is False
    bad = {"schema": 6, "profile": "enhanced", "pour_fill_contacts": "yes"}
    assert RouteOptions.from_dict(bad).pour_fill_contacts is False
    on_legacy = {"schema": 6, "profile": "legacy", "pour_fill_contacts": True}
    assert RouteOptions.from_dict(on_legacy).pour_fill_contacts is True


def test_the_form_sets_fill_contacts():
    chosen, errors = options.from_form({"profile": "enhanced", "pour_fill_contacts": True})
    assert errors == [] and chosen.pour_fill_contacts is True
    chosen, errors = options.from_form({"profile": "enhanced"})
    assert errors == [] and chosen.pour_fill_contacts is False
