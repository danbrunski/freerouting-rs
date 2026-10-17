import json

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
    assert json.loads(first.read_text(encoding="utf-8"))["schema"] == 1


def test_defaults_round_trip_too(tmp_path):
    path = tmp_path / "o.json"
    options.save(path, RouteOptions())
    assert options.load(path) == RouteOptions()


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
    assert RouteOptions.from_dict(future) == RouteOptions()
    # Not even a dict.
    for junk in (None, [], "x", 3, {"schema": "1"}):
        assert RouteOptions.from_dict(junk) == RouteOptions()


def test_booleans_are_not_numbers():
    assert RouteOptions.from_dict({"schema": 1, "max_passes": True}).max_passes == 0
    assert RouteOptions.from_dict({"schema": 1, "threads": 2.5}).threads == 0
    assert RouteOptions.from_dict({"schema": 1, "optimizer": 1}).optimizer is True
    assert RouteOptions.from_dict({"schema": 1, "ignore_net_classes": ["a", 3]}).ignore_net_classes == []


def test_load_of_missing_or_broken_files(tmp_path):
    assert options.load(tmp_path / "missing.json") == RouteOptions()
    broken = tmp_path / "broken.json"
    broken.write_text("{not json", encoding="utf-8")
    assert options.load(broken) == RouteOptions()


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
    assert chosen == RouteOptions(
        max_passes=5, ignore_net_classes=["GND", "Power"], min_trace_width_um=150.0, timeout="10m", rule_area_fixup=False
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
