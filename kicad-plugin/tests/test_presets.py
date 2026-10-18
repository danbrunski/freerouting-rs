"""Option presets (presets.py): files in the board's freerouting-rs/presets
folder or the user's, applied over the dialog's options."""

import json
from pathlib import Path

import options
import presets
import pytest
from conftest import CANARY_PRESET_FILE, PLUGIN_SRC, canary_preset_options
from options import RouteOptions


def write(folder, name, data):
    folder.mkdir(parents=True, exist_ok=True)
    text = data if isinstance(data, str) else json.dumps(data)
    (folder / name).write_text(text, encoding="utf-8")


def preset(name, **fields):
    return {"schema": options.SCHEMA, "name": name, **fields}


def test_the_plugin_ships_no_preset():
    assert not (PLUGIN_SRC / "presets").exists()
    assert not hasattr(options, "CANARY_PRESET") and not hasattr(options, "with_preset")


def test_presets_load_from_the_board_folder_and_the_user_folder(tmp_path):
    work, user = tmp_path / "freerouting-rs", tmp_path / "user"
    write(work / "presets", "b.json", preset("Beta", description="two passes", max_passes=2))
    write(work / "presets", "a.json", preset("Alpha", sweep="fixed"))
    write(user, "c.json", preset("Gamma", threads=2))
    write(user, "dup.json", preset("Alpha", threads=9))  # hidden by the board's Alpha
    found, warnings = presets.discover(work, user)
    assert warnings == []
    assert [(p.name, p.source) for p in found] == [("Alpha", "board"), ("Beta", "board"), ("Gamma", "user")]
    assert found[1].description == "two passes" and found[1].data == {"max_passes": 2}
    assert found[0].label() == "Alpha (board)"
    # a folder that does not exist is no preset and no warning
    assert presets.discover(tmp_path / "nowhere", None) == ([], [])


def test_the_user_folder_is_only_used_when_it_exists(tmp_path):
    home = tmp_path / "home"
    assert presets.user_dir(env={}, home=home, windows=False) is None
    folder = home / ".config" / "freerouting-rs" / "presets"
    folder.mkdir(parents=True)
    assert presets.user_dir(env={}, home=home, windows=False) == folder
    other = tmp_path / "elsewhere"
    assert presets.user_dir(env={presets.USER_ENV: str(other)}, home=home) is None  # named, but absent
    other.mkdir()
    assert presets.user_dir(env={presets.USER_ENV: str(other)}, home=home) == other
    appdata = tmp_path / "appdata"
    (appdata / "freerouting-rs" / "presets").mkdir(parents=True)
    assert presets.user_dir(env={"APPDATA": str(appdata)}, home=home, windows=True) == appdata / "freerouting-rs" / "presets"


def test_a_malformed_preset_is_skipped_with_a_warning_and_the_rest_load(tmp_path):
    folder = tmp_path / "freerouting-rs" / "presets"
    write(folder, "1-bad-json.json", "{not json")
    write(folder, "2-array.json", "[1, 2]")
    write(folder, "3-no-name.json", {"schema": options.SCHEMA, "max_passes": 3})
    write(folder, "4-blank-name.json", preset("  "))
    write(folder, "5-old-schema.json", {"schema": 99, "name": "Future"})
    write(folder, "6-no-schema.json", {"name": "Bare"})
    write(folder, "7-good.json", preset("Good", max_passes=3))
    (folder / "8-bytes.json").write_bytes(b"\xff\xfe\x00")
    found, warnings = presets.discover(tmp_path / "freerouting-rs", None)
    assert [p.name for p in found] == ["Good"]
    skipped = {w.split()[1] for w in warnings}
    assert skipped == {f"{n}.json" for n in ("1-bad-json", "2-array", "3-no-name", "4-blank-name", "5-old-schema", "6-no-schema", "8-bytes")}
    assert all("skipped" in w for w in warnings)


def test_applying_sets_only_the_options_the_preset_holds():
    current = RouteOptions(threads=4, max_passes=9, sweep="fixed")
    loaded, _ = presets.parse(json.dumps(preset("p", max_passes=1, sweep="")), Path("p.json"), "board")
    merged, warnings = presets.apply(loaded, current)
    assert warnings == []
    assert (merged.max_passes, merged.sweep, merged.threads) == (1, "", 4)
    assert current.max_passes == 9  # the caller's options are not changed


def test_a_bad_value_or_an_unknown_option_is_ignored_with_a_warning():
    current = RouteOptions(max_passes=5)
    body = preset("p", max_passes=-3, sweep="always", board_input="svg", colour="red", threads=2)
    loaded, _ = presets.parse(json.dumps(body), Path("p.json"), "board")
    merged, warnings = presets.apply(loaded, current)
    assert (merged.max_passes, merged.sweep, merged.board_input, merged.threads) == (5, "", "dsn", 2)
    assert len(warnings) == 4 and any("colour" in w for w in warnings)
    assert any("max_passes=-3" in w for w in warnings)


def test_save_as_round_trips(tmp_path):
    work = tmp_path / "freerouting-rs"
    chosen = RouteOptions(
        class_widths={"A": 0.3},
        fixed_pour_classes=["A"],
        drop_pour_nets=["N"],
        max_passes=3,
        sweep="free",
        sweep_passes=2,
        board_input="kicad_pcb",
        edge_and_pad_keepouts=False,
        stages={"autorouter.connection_router": "x"},
    )
    path = presets.save(work, "My flow: ä", chosen, description="d", stage_defaults={"autorouter.connection_router": "y"})
    assert path == work / "presets" / "My-flow-ä.json"
    stored = json.loads(path.read_text(encoding="utf-8"))
    assert stored["name"] == "My flow: ä" and stored["description"] == "d" and stored["schema"] == options.SCHEMA
    found, warnings = presets.discover(work, None)
    assert warnings == [] and [p.name for p in found] == ["My flow: ä"]
    merged, warnings = presets.apply(found[0], RouteOptions())
    assert warnings == [] and merged == chosen
    # an unset option is stored as null and clears a set one
    presets.save(work, "Unset", RouteOptions())
    unset = [p for p in presets.discover(work, None)[0] if p.name == "Unset"][0]
    assert presets.apply(unset, RouteOptions(improvement_threshold=5.0, start_ripup_costs=7))[0] == RouteOptions()
    # the same name replaces the file
    presets.save(work, "My flow: ä", RouteOptions(max_passes=1))
    again = presets.apply(presets.discover(work, None)[0][0], RouteOptions(sweep="fixed"))[0]
    assert again.max_passes == 1 and again.sweep == ""  # the new file; the old sweep is gone
    assert len(list((work / "presets").glob("*.json"))) == 2


def test_a_name_with_nothing_usable_is_refused(tmp_path):
    for name in ("", "  ", "///"):
        with pytest.raises(ValueError):
            presets.save(tmp_path, name, RouteOptions())
    assert not (tmp_path / "presets").exists()


def test_the_canary_fixture_preset_gives_the_old_preset_values_and_arguments():
    """fixtures/canary-pwr/presets/canary-graded-flow.json holds what
    options.CANARY_PRESET did (K09.8) and nothing else."""
    data = json.loads(CANARY_PRESET_FILE.read_text(encoding="utf-8"))
    assert data["name"] == "canary graded flow" and options.valid_timeout("")
    chosen = canary_preset_options()
    assert chosen == RouteOptions(
        class_widths={"Power_High": 0.3, "Switch_Node": 0.3, "Power": 0.3, "GND": 0.4},
        fixed_pour_classes=["Power_High", "Switch_Node"],
        fixed_pour_mode="wires",
        drop_pour_nets=["GND"],
        edge_and_pad_keepouts=True,
        max_passes=1,
        start_ripup_costs=1500,
        min_trace_width_um=150.0,
        sweep="free",
        sweep_passes=2,
    )
    assert chosen.to_args() == [
        "--max-passes", "1",
        "--router.min_trace_width_um=150",
        "--router.scoring.start_ripup_costs=1500",
        "--router.autorouter.sweep=free",
        "--router.autorouter.sweep_passes=2",
    ]  # fmt: skip
    assert options.dsn_settings(chosen) == {
        "class_widths": {"Power_High": 0.3, "Switch_Node": 0.3, "Power": 0.3, "GND": 0.4},
        "fixed_pour_classes": ["Power_High", "Switch_Node"],
        "drop_pour_nets": ["GND"],
        "edge_and_pad_keepouts": True,
        "route_on_plane_layers": [],
    }
    # and the file is a valid options file by itself
    assert RouteOptions.from_dict(data) == chosen
