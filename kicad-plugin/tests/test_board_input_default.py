"""K13.7: the plugin reads the open board directly by default (`board_input =
kicad_pcb`, enhanced profile). `legacy` keeps the DSN export (Java's plugin
exports a DSN). Saved files keep an explicit choice; a file with no saved
choice takes the profile's default, whatever fix-ups it turns on (K14.5: the
reader applies all four K09 fix-ups that rewrite the DSN text, so K13.7's rules
that sent such a file to the DSN are gone). Schema 6 drops `apply_kicad_rules`."""

import json
from dataclasses import fields, replace
from pathlib import Path

import options
import presets
import pytest
from options import RouteOptions

# The K09 options that rewrite the DSN text, each with a value that turns it on, as the
# options file stores it. Both inputs apply them (K14): the DSN path edits the exported
# text, the router's reader edits its tree for the snapshot.
FIXUPS = {
    "class_widths": {"class_widths": {"GND": 0.4}},
    "class_widths_auto": {"class_widths": "auto"},
    "fixed_pour_classes_wires": {"fixed_pour_classes": ["Power"], "fixed_pour_mode": "wires"},
    "fixed_pour_classes_engine": {"fixed_pour_classes": ["Power"], "fixed_pour_mode": "engine"},
    "fixed_pour_classes_auto": {"fixed_pour_classes": "auto"},
    "drop_pour_nets": {"drop_pour_nets": ["GND"]},
    "route_on_plane_layers": {"route_on_plane_layers": ["In2.Cu"]},
}


# --- the default ------------------------------------------------------------------


def test_the_enhanced_profile_reads_the_board_and_legacy_the_dsn():
    assert options.profile_defaults().board_input == "kicad_pcb"
    assert options.profile_defaults("enhanced").board_input == "kicad_pcb"
    assert options.profile_defaults("legacy").board_input == "dsn"
    assert RouteOptions().board_input == "dsn"  # the dataclass default is legacy's
    assert options.BOARD_INPUTS == ("dsn", "kicad_pcb")  # indexes are used


def test_no_saved_file_gives_the_snapshot(tmp_path):
    work = tmp_path / "freerouting-rs"
    assert options.load_for_board(work).board_input == "kicad_pcb"
    broken = tmp_path / "broken.json"
    broken.write_text("{not json", encoding="utf-8")
    for path in (work / "route-options.json", broken):
        assert options.load(path).board_input == "kicad_pcb"


def test_no_saved_file_starts_from_the_default_profile_with_its_sweep(tmp_path):
    # A board with no file gets what a file of only the default profile gets,
    # K09.9's completion sweep included (before, it got the dataclass defaults).
    work = tmp_path / "freerouting-rs"
    broken = tmp_path / "broken.json"
    broken.write_text("{not json", encoding="utf-8")
    profile_only = tmp_path / "profile-only.json"
    profile_only.write_text(json.dumps({"schema": options.SCHEMA, "profile": options.DEFAULT_PROFILE}), encoding="utf-8")
    for path in (work / "route-options.json", broken):
        assert options.load(path) == options.profile_defaults(options.DEFAULT_PROFILE) == options.load(profile_only)
        assert options.load(path).sweep == "free"
    assert options.load_for_board(work) == options.profile_defaults()
    assert options.load_for_board(work, {"fix_rule_areas": False}) == replace(options.profile_defaults(), rule_area_fixup=False)
    # Saved again, nothing beyond the schema and the profile is written.
    options.save(work / "route-options.json", options.load_for_board(work))
    assert json.loads((work / "route-options.json").read_text(encoding="utf-8")) == {"schema": 6, "profile": "enhanced"}


def test_the_snapshot_input_is_not_called_experimental():
    assert "xperimental" not in options.BOARD_INPUT_LABELS["kicad_pcb"]
    assert "xperimental" not in " ".join(options.BOARD_INPUT_LABELS.values())
    source = (Path(options.__file__).parent / "plugin.py").read_text(encoding="utf-8")
    start = source.index("self.board_input = wx.Choice")
    tip = source[start : source.index("self.input_note", start)]
    assert "xperimental" not in tip
    assert "xperimental" not in source.split('"""', 2)[1]  # plugin.py's module docstring


# --- the kept saved choice ----------------------------------------------------------


def test_enhanced_with_the_dsn_writes_the_choice_and_reads_it_back(tmp_path):
    path = tmp_path / "route-options.json"
    chosen = options.profile_defaults("enhanced")
    chosen.board_input = "dsn"
    options.save(path, chosen)
    stored = json.loads(path.read_text(encoding="utf-8"))
    assert stored["board_input"] == "dsn" and stored["schema"] == 6
    assert options.load(path) == chosen


def test_enhanced_with_the_snapshot_writes_nothing(tmp_path):
    path = tmp_path / "route-options.json"
    options.save(path, options.profile_defaults("enhanced"))
    assert "board_input" not in json.loads(path.read_text(encoding="utf-8"))
    assert options.load(path).board_input == "kicad_pcb"


def test_legacy_writes_the_snapshot_choice_and_not_the_dsn(tmp_path):
    path = tmp_path / "route-options.json"
    legacy = options.profile_defaults("legacy")
    options.save(path, legacy)
    assert "board_input" not in json.loads(path.read_text(encoding="utf-8"))
    legacy.board_input = "kicad_pcb"
    options.save(path, legacy)
    assert json.loads(path.read_text(encoding="utf-8"))["board_input"] == "kicad_pcb"
    assert options.load(path) == legacy


def test_a_schema_6_file_saying_dsn_loads_dsn():
    assert RouteOptions.from_dict({"schema": 6, "profile": "enhanced", "board_input": "dsn"}).board_input == "dsn"
    assert RouteOptions.from_dict({"schema": 6, "profile": "enhanced"}).board_input == "kicad_pcb"
    assert RouteOptions.from_dict({"schema": 6, "profile": "legacy"}).board_input == "dsn"
    # a bad value is no saved choice
    assert RouteOptions.from_dict({"schema": 6, "board_input": "svg"}).board_input == "kicad_pcb"


# --- the schema migration -------------------------------------------------------------


def test_schema_6_is_written_and_every_older_schema_still_reads():
    assert options.SCHEMA == 6 and options.READABLE_SCHEMAS == (1, 2, 3, 4, 5, 6)
    for schema in (1, 2, 3, 4, 5):
        loaded = RouteOptions.from_dict({"schema": schema, "profile": "enhanced", "max_passes": 3})
        assert loaded.max_passes == 3 and loaded.board_input == "kicad_pcb", schema


def test_a_schema_5_file_without_a_choice_takes_the_profiles_default():
    assert RouteOptions.from_dict({"schema": 5, "profile": "enhanced"}).board_input == "kicad_pcb"
    assert RouteOptions.from_dict({"schema": 5, "profile": "legacy"}).board_input == "dsn"
    assert RouteOptions.from_dict({"schema": 5, "profile": "enhanced", "board_input": "kicad_pcb"}).board_input == "kicad_pcb"
    assert RouteOptions.from_dict({"schema": 5, "profile": "legacy", "board_input": "kicad_pcb"}).board_input == "kicad_pcb"
    assert RouteOptions.from_dict({"schema": 5, "profile": "enhanced", "board_input": "dsn"}).board_input == "dsn"


def test_a_schema_5_file_with_apply_kicad_rules_loads_and_saves_without_it(tmp_path):
    loaded = RouteOptions.from_dict({"schema": 5, "profile": "enhanced", "apply_kicad_rules": True, "max_passes": 2})
    assert loaded.max_passes == 2
    assert not hasattr(loaded, "apply_kicad_rules")
    assert "apply_kicad_rules" not in {f.name for f in fields(RouteOptions)}
    path = tmp_path / "route-options.json"
    path.write_text(json.dumps({"schema": 5, "apply_kicad_rules": True, "max_passes": 2}), encoding="utf-8")
    options.save(path, options.load(path))
    stored = json.loads(path.read_text(encoding="utf-8"))
    assert stored["schema"] == 6 and "apply_kicad_rules" not in stored and stored["max_passes"] == 2


@pytest.mark.parametrize("name", sorted(FIXUPS))
@pytest.mark.parametrize("schema", [3, 4, 5, 6])
def test_a_saved_fixup_without_a_choice_takes_the_profiles_input(name, schema):
    """K14.5: K13.7's load rule is gone. A file that names no input takes the
    profile's default, with the fix-up on."""
    data = {"schema": schema, "profile": "enhanced", **FIXUPS[name]}
    loaded = RouteOptions.from_dict(data)
    assert loaded.board_input == "kicad_pcb"
    assert all(getattr(loaded, key) == value for key, value in FIXUPS[name].items())
    legacy = RouteOptions.from_dict({**data, "profile": "legacy"})
    assert legacy.board_input == "dsn"  # legacy's own default, not a move


@pytest.mark.parametrize("name", sorted(FIXUPS))
@pytest.mark.parametrize("choice", ["dsn", "kicad_pcb"])
def test_a_saved_fixup_with_a_choice_keeps_the_choice(name, choice):
    loaded = RouteOptions.from_dict({"schema": 6, "profile": "enhanced", "board_input": choice, **FIXUPS[name]})
    assert loaded.board_input == choice


def test_the_k13_7_rules_and_the_refusal_are_gone():
    """K14.5: nothing refuses the snapshot, so there is no list of what does and no rule that
    reads one."""
    for name in ("input_conflicts", "dsn_text_fixups", "DSN_REWRITE_LABELS"):
        assert not hasattr(options, name), name
    assert not hasattr(presets, "_dsn_for_fixups")


def test_the_canary_preset_file_as_a_saved_options_file_takes_the_snapshot():
    from conftest import CANARY_PRESET_FILE

    data = json.loads(CANARY_PRESET_FILE.read_text(encoding="utf-8"))
    assert "board_input" not in data
    loaded = RouteOptions.from_dict(data)
    assert loaded.board_input == "kicad_pcb" and loaded.class_widths and loaded.drop_pour_nets == ["GND"]


# --- the form and the dialog's profile choice ------------------------------------------


def test_the_form_follows_the_profiles_default_when_it_names_no_input():
    assert options.from_form({"profile": "enhanced"})[0].board_input == "kicad_pcb"
    assert options.from_form({"profile": "legacy"})[0].board_input == "dsn"
    assert options.from_form({"profile": "legacy", "board_input": "kicad_pcb"})[0].board_input == "kicad_pcb"
    assert "apply_kicad_rules" not in options.from_form({"apply_kicad_rules": True})[0].to_dict()


# The same fix-ups as the dialog's form holds them.
FORMS = {
    "class_widths": {"class_widths": "GND=0.4"},
    "class_widths_auto": {"class_widths": "auto"},
    "fixed_pour_classes_wires": {"fixed_pour_classes": "Power", "fixed_pour_mode": "wires"},
    "fixed_pour_classes_engine": {"fixed_pour_classes": "Power", "fixed_pour_mode": "engine"},
    "fixed_pour_classes_auto": {"fixed_pour_classes": "auto"},
    "drop_pour_nets": {"drop_pour_nets": "GND"},
    "route_on_plane_layers": {"route_on_plane_layers": "In2.Cu"},
}


@pytest.mark.parametrize("name", sorted(FORMS))
@pytest.mark.parametrize("board_input", ["dsn", "kicad_pcb"])
def test_the_form_accepts_each_fixup_on_either_input(name, board_input):
    """K14.5: no refusal at Route (`options.from_form` is what the dialog's Route calls)."""
    chosen, errors = options.from_form({"board_input": board_input, **FORMS[name]})
    assert errors == [] and chosen.board_input == board_input
    for key, value in FIXUPS[name].items():  # the form parses to what the file stores
        assert getattr(chosen, key) == value, key


def test_the_form_accepts_all_the_fixups_together_on_the_snapshot():
    form = {"board_input": "kicad_pcb", "class_widths": "auto", "fixed_pour_classes": "auto", "drop_pour_nets": "auto"}
    chosen, errors = options.from_form({**form, "route_on_plane_layers": "In2.Cu"})
    assert errors == [] and chosen.board_input == "kicad_pcb"


# --- presets ------------------------------------------------------------------------------


def parsed(**fields_):
    data = {"schema": options.SCHEMA, "name": "p", **fields_}
    found, warning = presets.parse(json.dumps(data), Path("p.json"), "board")
    assert warning is None
    return found


@pytest.mark.parametrize("name", sorted(FIXUPS))
@pytest.mark.parametrize("found", ["dsn", "kicad_pcb"])
def test_a_preset_with_a_fixup_and_no_input_keeps_the_input_it_found_without_a_warning(name, found):
    """K14.5: K13.7's move to the DSN, and its warning, are gone."""
    current = replace_input(options.profile_defaults("enhanced"), found)
    merged, warnings = presets.apply(parsed(**FIXUPS[name]), current)
    assert merged.board_input == found and warnings == []
    assert all(getattr(merged, key) == value for key, value in FIXUPS[name].items())
    assert current.board_input == found  # the caller's options are not changed


def test_a_preset_naming_the_snapshot_keeps_it():
    merged, warnings = presets.apply(parsed(board_input="kicad_pcb", class_widths={"GND": 0.4}), options.profile_defaults())
    assert merged.board_input == "kicad_pcb" and warnings == []


def test_a_preset_without_a_dsn_fixup_leaves_the_input_alone():
    merged, warnings = presets.apply(parsed(max_passes=3), options.profile_defaults())
    assert merged.board_input == "kicad_pcb" and warnings == []
    merged, warnings = presets.apply(parsed(max_passes=3), replace_input(options.profile_defaults(), "dsn"))
    assert merged.board_input == "dsn" and warnings == []


def test_a_preset_naming_the_dsn_keeps_it_without_a_warning():
    merged, warnings = presets.apply(parsed(board_input="dsn", class_widths={"GND": 0.4}), options.profile_defaults())
    assert merged.board_input == "dsn" and warnings == []


def test_a_saved_preset_names_the_input_and_reapplies_it(tmp_path):
    work = tmp_path / "freerouting-rs"
    chosen = options.profile_defaults("enhanced")
    presets.save(work, "snap", chosen)
    stored = json.loads((work / "presets" / "snap.json").read_text(encoding="utf-8"))
    assert stored["board_input"] == "kicad_pcb" and "apply_kicad_rules" not in stored and stored["schema"] == 6
    found, _ = presets.discover(work, None)
    assert presets.apply(found[0], replace_input(chosen, "dsn")) == (chosen, [])


def test_a_preset_from_before_schema_6_with_apply_kicad_rules_applies_quietly():
    merged, warnings = presets.apply(parsed(schema=5, apply_kicad_rules=True, board_input="kicad_pcb"), options.profile_defaults())
    assert warnings == [] and merged.board_input == "kicad_pcb"


def test_the_canary_preset_on_the_new_default_stays_on_the_snapshot():
    from conftest import CANARY_PRESET_FILE

    found, warning = presets.parse(CANARY_PRESET_FILE.read_text(encoding="utf-8"), CANARY_PRESET_FILE, "board")
    assert warning is None
    merged, warnings = presets.apply(found, options.profile_defaults("enhanced"))
    assert merged.board_input == "kicad_pcb" and warnings == []  # K14.5: the reader applies its fix-ups
    assert options.dsn_settings(merged)["class_widths"] == {"Power_High": 0.3, "Switch_Node": 0.3, "Power": 0.3, "GND": 0.4}
    assert options.dsn_settings(merged)["drop_pour_nets"] == ["GND"]


def replace_input(opts, board_input):
    return replace(opts, board_input=board_input)


# --- the dialog's note under the board input (K13.7, the owner's run; K14.5) -----------------------------

CANARY_FIXUPS = {"class_widths": {"GND": 0.4}, "fixed_pour_classes": ["Power_High"], "drop_pour_nets": ["GND"]}


@pytest.mark.parametrize("name", sorted(FIXUPS))
@pytest.mark.parametrize("board_input", ["dsn", "kicad_pcb"])
def test_the_note_has_no_dsn_line_for_any_fixup(name, board_input):
    """K14.5: no fix-up needs the DSN export, so the note holds the ignored-keepouts line
    (snapshot only) and nothing else."""
    chosen = replace(options.profile_defaults(), board_input=board_input, **FIXUPS[name])
    note = options.input_note(chosen)
    assert note == options.input_ignored_note(chosen)
    assert "DSN" not in note and "turn" not in note and "cannot apply" not in note
    assert (note != "") == (board_input == "kicad_pcb")  # the keepouts are on by default


def test_the_note_with_the_canary_fixups_is_the_ignored_note_or_nothing():
    on_snapshot = replace(options.profile_defaults(), **CANARY_FIXUPS)
    assert options.input_note(on_snapshot) == options.input_ignored_note(on_snapshot) != ""
    assert options.input_note(replace(on_snapshot, board_input="dsn")) == ""
    assert options.input_note(options.profile_defaults()) == options.input_ignored_note(options.profile_defaults()) != ""
    assert options.input_note(options.profile_defaults("legacy")) == ""
    one = replace(on_snapshot, edge_and_pad_keepouts=False)
    assert options.input_note(one) == ""
