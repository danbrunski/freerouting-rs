"""What the plugin does to the DSN after KiCad's export, in one place (K09).

`plugin.py` (inside KiCad) and `kicad-plugin/headless_run.py` (pcbnew in a
plain Python, for graded runs) call the same functions, so a headless run
routes exactly what a plugin run routes.

The pcbnew module is passed in, never imported, so this file imports nothing
outside the standard library and the plugin's pure modules. Call it on
KiCad's UI thread only.

`prepare_snapshot` is the K10 form: the board snapshot the router reads
directly, with none of those DSN steps (`snapshot.py`); the K09 fix-ups
`class_widths`, `drop_pour_nets`, `route_on_plane_layers` and
`fixed_pour_classes` go to the router as flags its reader applies to the board
(K14, `snapshot_fixups`, `snapshot_engine_args`).

Order: refill zones, KiCad's export, the planes cut where a higher-priority zone of
another net fills (K15.24, `plane_priority`, always on), the rule-area fix-up (K01.2, `rule_areas`), the
Edge.Cuts, Margin and board mask-aperture keepouts (Q18.5, Q18.7, `edge_graphics`,
always on), the copper graphics and texts (Q22.4, `copper_graphics`, always on), the
thermal-spoke keepouts (Q22.20, `thermal_spokes`, always on), the unlocked
track arcs as polylines (K13.8, `track_arcs`, always on), then the
K09 fix-ups (`dsn_fixups`); the pads' solder-mask apertures go to the router as
a flag (Q18.7, `solder_mask_args`). After `ImportSpecctraSES`, `after_import` sets the
drills KiCad's import takes from the net class instead of the session, and puts the
input copper back (`restore_copper`; track arcs need the pcbnew module). A fix-up that fails is logged with a WARNING and
leaves the DSN as it was before that step; it never blocks routing.
"""

import hashlib
import json
from pathlib import Path

try:  # inside the plugin package
    from . import (
        copper_graphics,
        dsn_fixups,
        edge_graphics,
        kicad_areas,
        kicad_board,
        kicad_rules,
        options,
        plane_priority,
        restore_copper,
        rule_areas,
        ses_vias,
        snapshot,
        thermal_spokes,
        track_arcs,
    )
except ImportError:  # tests and headless scripts put this folder on sys.path
    import copper_graphics
    import dsn_fixups
    import edge_graphics
    import kicad_areas
    import kicad_board
    import kicad_rules
    import options
    import plane_priority
    import restore_copper
    import rule_areas
    import ses_vias
    import snapshot
    import thermal_spokes
    import track_arcs


def _read(path):
    # open(), not Path.read_text(newline=...): that needs Python 3.13 and
    # KiCad 10 ships 3.12 (K03.6).
    with open(path, encoding="utf-8", newline="") as f:
        return f.read()


def _write(path, text):
    with open(path, "w", encoding="utf-8", newline="") as f:
        f.write(text)


def refill(board, pcbnew, phase="before routing export"):
    """Refresh the live copper before either export path, without saving.

    Fixed pour wires must describe the current board, not the fill cached
    before its last edit. A failed fill stops preparation instead of silently
    routing a different board from the command-line flow.
    """
    if not any(not zone.GetIsRuleArea() for zone in board.Zones()):
        return []
    filler = pcbnew.ZONE_FILLER(board)
    if not filler.Fill(board.Zones()):
        raise RuntimeError("KiCad could not refill the copper zones")
    return [f"zones refilled {phase}"]


def router_args(board, opts, extra_args=(), for_gui=False):
    """The current Board Setup minimum and options, shared by both frontends."""
    try:
        nm = board.GetDesignSettings().m_TrackMinWidth
        minimum = nm / 1000.0 if nm and nm > 0 else None
    except Exception:  # older pcbnew without the attribute
        minimum = None
    return (
        opts.to_args(minimum, for_gui=for_gui, extra_args=extra_args)
        + options.board_minimum_args(board_minimums_nm(board))
        + kicad_dru_args(board, opts, for_gui)
    )


def kicad_dru_args(board, opts, for_gui=False):
    """Q22.5: `--kicad-dru <board>.kicad_dru` when the router reads KiCad's
    DSN export and the project has custom rules. The export drops that file;
    the router applies the clearance rules it can express (item types, net
    classes, hole clearance). Always on, in every profile: a fix to the
    input. K13.1: the `.kicad_pcb` input reads the file itself and gets
    `--apply-design-rules`, also always (decided 2026-10-20: the board's own
    rules reach the router on both inputs; the K10.3 option
    `apply_kicad_rules` no longer decides it). The GUI's command line has
    neither flag."""
    if for_gui:
        return []
    if opts.board_input == options.INPUT_KICAD_PCB:
        return snapshot.router_args(opts)
    if opts.board_input != options.INPUT_DSN:
        return []
    try:
        name = board.GetFileName()
    except Exception:  # not a pcbnew board
        return []
    if not name:
        return []
    dru = Path(name).with_suffix(".kicad_dru")
    return ["--kicad-dru", str(dru)] if dru.is_file() else []


def board_minimums_nm(board):
    """Q18: the Board Setup minimums of `options.BOARD_MINIMUM_SETTINGS`, in
    nanometres; an attribute this pcbnew does not have is left out."""
    out = {}
    try:
        settings = board.GetDesignSettings()
    except Exception:  # not a board
        return out
    for attr, _key in options.BOARD_MINIMUM_SETTINGS:
        try:
            out[attr] = getattr(settings, attr)
        except Exception:  # an older pcbnew without the attribute
            continue
    return out


def _dsn_mm(nm):
    """A length in nm as `dsn_fixups.Dsn` reads it back from KiCad's export,
    which prints micrometres with `%.6g`."""
    return float(f"{nm / 1000.0:.6g}") / 1000.0


def board_classes(board):
    """K14.1: the board's net classes as KiCad's DSN export names them, for
    `dsn_fixups.auto_class_widths_of_board` on the board snapshot:
    `{"default_width_mm", "widths_mm": {class: mm}, "net_class": {net:
    class}}`. Each net's class is its effective one
    (`NET_SETTINGS.GetEffectiveNetClass`, an aggregate such as
    `Power,Default` included); `Default` is `kicad_default`, as in the export.
    The widths go through the export's `%.6g` micrometres, so they are the
    floats the DSN path reads."""
    settings = board.GetDesignSettings().m_NetSettings
    widths, net_class = {}, {}
    for name in board.GetNetsByName().keys():
        name = str(name)
        if not name:
            continue
        nc = settings.GetEffectiveNetClass(name)
        cls = str(nc.GetName())
        cls = "kicad_default" if cls == "Default" else cls
        net_class[name] = cls
        widths[cls] = _dsn_mm(nc.GetTrackWidth())
    return {
        "default_width_mm": _dsn_mm(settings.GetDefaultNetclass().GetTrackWidth()),
        "widths_mm": widths,
        "net_class": net_class,
    }


def save_arguments(board, pcbnew, design, opts, extra_args=(), lines=()):
    """Record the exact prepared input and CLI arguments for reproduction.

    This is also headless_run's input: scripts consume the same arguments
    as Route instead of maintaining a second set of router flags.
    """
    design = Path(design)
    data = {
        "args": router_args(board, opts, extra_args),
        "log": list(lines),
        "kicad": pcbnew.GetBuildVersion(),
        "input_sha256": hashlib.sha256(design.read_bytes()).hexdigest(),
    }
    path = design.with_suffix(".args.json")
    _write(path, json.dumps(data, indent=2) + "\n")
    return path


def fix_plane_priorities(board, pcbnew, dsn):
    """K15.24: each plane where a higher-priority zone of another net fills
    is cut as KiCad's fill cuts it (`kicad_board.plane_cuts`,
    `plane_priority`). Always on: the router counts no pad or via on a plane
    where KiCad fills another net. Runs first, while the DSN's planes are
    still the export's. Returns the log lines (one per cut plane; none with
    nothing to cut, and then the file is not written)."""
    try:
        planes = kicad_board.plane_cuts(board, pcbnew)
        if not any(p["cut"] is not None for p in planes):
            return []
        text = _read(dsn)
        new_text, lines = plane_priority.apply(text, planes)
        if new_text != text:
            _write(dsn, new_text)
    except Exception as e:  # never block routing on the fix-up
        lines = [f"WARNING: planes not cut by zone priority: {e!r}"]
    return lines


def fix_rule_areas(board, pcbnew, dsn):
    """K01.2. Returns the log lines."""
    try:
        areas = kicad_areas.collect(board, pcbnew)
        text = _read(dsn)
        new_text, lines = rule_areas.rewrite(text, areas)
        if new_text != text:
            _write(dsn, new_text)
    except Exception as e:  # never block routing on the fix-up
        lines = [f"WARNING: rule-area fix-up skipped: {e!r}"]
    return lines or ["no rule areas on the board"]


def fix_edge_graphics(board, pcbnew, dsn):
    """Q18.5: the Edge.Cuts and Margin shapes the export leaves out, as copper
    keepouts, and the board's own F.Mask/B.Mask apertures (Q18.7)
    (`edge_graphics`). Always on: routing never breaks the board's edge
    clearance or bridges a mask aperture. Returns the log lines (none with nothing to keep out)."""
    try:
        facts = kicad_board.edge_graphics(board, pcbnew)
        text = _read(dsn)
        new_text, lines = edge_graphics.apply(text, facts)
        if new_text != text:
            _write(dsn, new_text)
    except Exception as e:  # never block routing on the fix-up
        lines = [f"WARNING: edge-graphic keepouts skipped: {e!r}"]
    return lines


def fix_copper_graphics(board, pcbnew, dsn):
    """Q22.4: the copper shapes and texts the export leaves out, as keepouts
    (`copper_graphics`; shapes of a net the DSN lists are left out and
    counted). Always on: routing never crosses the board's own copper.
    Returns the log lines (none with nothing on copper)."""
    try:
        facts = kicad_board.copper_graphics(board, pcbnew)
        text = _read(dsn)
        new_text, lines = copper_graphics.apply(text, facts)
        if new_text != text:
            _write(dsn, new_text)
    except Exception as e:  # never block routing on the fix-up
        lines = [f"WARNING: copper-graphic obstacles skipped: {e!r}"]
    return lines


def fix_thermal_spokes(board, pcbnew, dsn):
    """Q22.20: room for the thermal spokes of every pad that a zone of its net
    connects by thermal relief, as keepouts that carry the pad's net
    (`thermal_spokes`). Always on: routing never starves a thermal relief
    below Board Setup's `min_resolved_spokes`. Returns the log lines (none with
    nothing to keep clear)."""
    try:
        facts = kicad_board.thermal_facts(board, pcbnew)
        text = _read(dsn)
        new_text, lines = thermal_spokes.apply(text, facts)
        if new_text != text:
            _write(dsn, new_text)
    except Exception as e:  # never block routing on the fix-up
        lines = [f"WARNING: thermal-spoke keepouts skipped: {e!r}"]
    return lines


def fix_track_arcs(board, dsn):
    """K13.8: each unlocked track arc as the polyline the `.kicad_pcb` reader
    lays, in place of the one chord KiCad's export writes (`track_arcs`).
    Always on: routed copper keeps its clearance from the arc, and
    `restore_copper` puts the arc back after the import. Returns the log lines
    (none without an unlocked arc)."""
    try:
        arcs = track_arcs.collect(board)
        if not arcs:
            return []
        text = _read(dsn)
        new_text, lines = track_arcs.apply(text, arcs)
        if new_text != text:
            _write(dsn, new_text)
    except Exception as e:  # never block routing on the fix-up
        lines = [f"WARNING: track arcs left as chords: {e!r}"]
    return lines


def solder_mask_args(board, pcbnew, dsn):
    """Q18.7: the router flag that keeps other nets out of the pads' mask
    apertures (`kicad_board.mask_expansions`), for the pads whose aperture
    reaches past the smallest clearance of the exported DSN, grown by the
    board's solder-mask-to-copper clearance (Q22.12). Returns
    `(args, log_lines)`; never raises."""
    try:
        floor = dsn_fixups.Dsn(_read(dsn)).min_clearance_mm()
        pads = kicad_board.mask_expansions(board, pcbnew, floor, kicad_board.mask_to_copper_nm(board))
    except Exception as e:  # never block routing on it
        return [], [f"WARNING: solder-mask apertures skipped: {e!r}"]
    if not pads:
        return [], []
    return options.solder_mask_args(pads), [
        f"solder mask: {len(pads)} pad(s) whose aperture reaches past their clearance kept clear of other nets"
    ]


def dru_path(board):
    """The `<board>.kicad_dru` beside the board file, or None for a board with no file name."""
    try:
        name = board.GetFileName()
    except Exception:  # not a pcbnew board
        return None
    return Path(name).with_suffix(".kicad_dru") if name else None


def filler_clearance_nm(board, zones_nm):
    """K15.32: the smallest clearance KiCad's zone filler can apply between the signal-layer
    zones and other nets' copper, for `router.cleanup.pour_clearance_um`: the smallest of the
    zones' own clearances (`zones_nm`) and every `clearance` constraint of the board's `.kicad_dru`
    (conditional rules included; a custom rule replaces the zone's own value in KiCad 10.0.7's
    filler, K15.29, K15.32). The net classes' clearances are left out: measured on 10.0.7, a class
    clearance below the zone's changes nothing and one above it raises the gap. Returns
    `(nanometres or None, log_lines)`; None when the rules file exists and cannot be read, so the value is unknown and the router
    keeps its own clearances. Never raises."""
    values, lines = [zones_nm], []
    path = dru_path(board)
    if path is not None and path.is_file():
        try:
            rules = kicad_rules.clearance_minimums_nm(_read(path))
        except (OSError, ValueError) as e:  # RulesError is a ValueError; a bad encoding too
            return None, [f"WARNING: {path.name} unreadable ({e}); no pour clearance passed, the router keeps its own"]
        if rules:
            values += [nm for _, nm in rules]
            lines.append(
                f"{path.name}: {len(rules)} clearance rule(s), smallest {min(nm for _, nm in rules) / 1000:g} um "
                "(KiCad's filler lets them win over the zones' own clearance)"
            )
    return min(values), lines


def pour_rule_args(board, pcbnew, dsn):
    """K15.27: the router flags `--router.cleanup.pour_min_thickness_um` and
    `--router.cleanup.pour_clearance_um` for a DSN input (K15.29: and
    `pour_island_removal`, `pour_islands_kept_nets`): the smallest
    `min_thickness` and zone clearance of the board's copper zones on the
    exported DSN's signal layers (`kicad_board.signal_pour_rules`), which
    KiCad's export drops. A signal layer is one the router takes as one: a
    layer of the DSN (after the fix-ups, so a power layer opened to routing
    counts) typed `signal` or `jumper`, or untyped. A `power` layer is a
    plane, and a `mixed` layer is one the router does not load, so a zone on
    either counts as no signal-layer pour. No such zone: no flag. Returns
    `(args, log_lines)`; never raises. Not called for the `.kicad_pcb` input,
    whose reader has each zone's own values. The flags come before the stage
    choices, and the arguments after `--` of `headless_run.py` come after
    them, so a flag given there wins."""
    try:
        layers = dsn_fixups.Dsn(_read(dsn)).layers
        signal = [name for name, kind, _ in layers if kind is None or kind in ("signal", "jumper")]
        if not signal:
            return [], []
        rules = kicad_board.signal_pour_rules(board, pcbnew, signal)
    except Exception as e:  # never block routing on it
        return [], [f"WARNING: zone thickness and clearance not passed: {e!r}"]
    if rules is None:
        return [], []
    clearance, clearance_lines = filler_clearance_nm(board, rules["clearance_nm"])
    args = options.pour_rule_args(
        rules["min_thickness_nm"],
        clearance,
        rules.get("island_removal", False),
        rules.get("islands_kept_nets", ()),
        rules.get("net_min_thickness_nm"),
    )
    if not args:
        return [], []
    removal = any(a.startswith("--router.cleanup.pour_island_removal=") for a in args)
    kept = rules.get("islands_kept_nets") or []
    islands = f", islands no pad reaches dropped (kept on {len(kept)} net(s))" if removal else ""
    own = next((a for a in args if a.startswith("--router.cleanup.pour_min_thickness_nets=")), None)
    if own:
        islands = f", {own.count(',') + 1} net(s) by their own thicker zones" + islands
    shown = f"clearance {clearance / 1000:g} um" if clearance and clearance > 0 else "no clearance"
    return args, clearance_lines + [
        f"signal-layer zones: {rules['zones']} on {', '.join(rules['layers'])}; the cleanup judges their pours "
        f"by min thickness {rules['min_thickness_nm'] / 1000:g} um and {shown}"
        f"{islands} ({' '.join(args)})"
    ]


def fix_dsn(board, pcbnew, dsn, opts):
    """K09: the fix-ups `opts` asks for. Returns `(router_args, log_lines)`;
    `router_args` carries the engine flags of `fixed_pour_mode = engine`."""
    settings = options.dsn_settings(opts)
    args, lines = [], []
    engine = opts.fixed_pour_mode == "engine" and opts.fixed_pour_classes
    if not dsn_fixups.wanted(settings) and not engine:
        return args, lines
    try:
        info = kicad_board.collect(board, pcbnew)
        text = _read(dsn)
        if engine:
            classes = opts.fixed_pour_classes
            if classes == options.AUTO:
                classes = dsn_fixups.auto_fixed_pour_classes(info, dsn_fixups.Dsn(text))
            args = dsn_fixups.engine_plane_obstacle_args(classes)
            lines.append(f"pours of {', '.join(classes) or 'no class'} kept clear by the router ({' '.join(args)})")
        if dsn_fixups.wanted(settings):
            new_text, more = dsn_fixups.apply(text, info, settings)
            lines += more
            if new_text != text:
                _write(dsn, new_text)
    except Exception as e:  # never block routing on the fix-up
        lines.append(f"WARNING: DSN fix-ups skipped: {e!r}")
    return args, lines


def prepare(board, pcbnew, dsn, opts, export):
    """Exports with `export(board, dsn)` (True on success), then applies the
    fix-ups. With `fix_existing` every track and via is locked first, so KiCad
    writes it as fixed copper; the caller keeps the returned tracks locked
    through the import and then calls `kicad_board.restore_locks`.
    Returns `(ok, router_args, log_lines, locked_tracks)`."""
    locked = []
    try:
        lines = refill(board, pcbnew)
        locked = kicad_board.lock_tracks(board) if opts.fix_existing else []
        if not export(board, dsn):
            raise RuntimeError("KiCad's DSN export failed")
    except Exception as e:
        kicad_board.restore_locks(locked)
        return False, [], [f"ERROR: routing preparation failed: {e}"], []
    lines += fix_plane_priorities(board, pcbnew, dsn)
    if opts.rule_area_fixup:
        lines += fix_rule_areas(board, pcbnew, dsn)
    lines += fix_edge_graphics(board, pcbnew, dsn)
    lines += fix_copper_graphics(board, pcbnew, dsn)
    lines += fix_thermal_spokes(board, pcbnew, dsn)
    lines += fix_track_arcs(board, dsn)
    mask_args, mask_lines = solder_mask_args(board, pcbnew, dsn)
    lines += mask_lines
    args, more = fix_dsn(board, pcbnew, dsn, opts)
    pour_args, pour_lines = pour_rule_args(board, pcbnew, dsn)
    args = mask_args + args + pour_args
    more += pour_lines
    if locked:
        more.append(f"fix existing: {len(locked)} track(s) and via(s) exported as fixed")
    return True, args, lines + more, locked


def snapshot_fixups(board, pcbnew, opts):
    """K14.1-K14.4: `class_widths`, `drop_pour_nets`,
    `route_on_plane_layers` and `fixed_pour_classes` in `wires` mode for the
    board snapshot, as router flags (`dsn_fixups.input_fixup_args`); the
    `.kicad_pcb` reader applies them to its tree as `dsn_fixups.apply` changes
    the DSN. `auto` is resolved from the board (`kicad_board.collect`,
    `board_classes`), as `fix_dsn` resolves it from the board and the DSN.
    Returns `(router_args, log_lines)`; a failure is a WARNING line and no
    flags, as a failed DSN fix-up is."""
    settings = {
        "class_widths": opts.class_widths,
        "drop_pour_nets": opts.drop_pour_nets,
        "route_on_plane_layers": opts.route_on_plane_layers,
        "fixed_pour_classes": options.dsn_settings(opts)["fixed_pour_classes"],
    }
    if not any(settings.values()):
        return [], []
    try:
        info = {}
        if options.AUTO in (settings["class_widths"], settings["drop_pour_nets"], settings["fixed_pour_classes"]):
            info = board_facts(board, pcbnew)
        return dsn_fixups.input_fixup_args(dsn_fixups.resolve_on_board(settings, info))
    except Exception as e:  # never block routing on the fix-up
        return [], [f"WARNING: board snapshot fix-ups skipped: {e!r}"]


def board_facts(board, pcbnew):
    """The board facts `auto` reads on the snapshot: `kicad_board.collect`
    with the board's own net classes (`board_classes`) as `classes`."""
    info = kicad_board.collect(board, pcbnew)
    info["classes"] = board_classes(board)
    return info


def snapshot_engine_args(board, pcbnew, opts):
    """K09.2 arm (a) on the board snapshot: `fixed_pour_mode = engine` is the
    router flag `--router.plane_obstacle_net_classes`, with `auto` resolved
    from the board (K14.4, `dsn_fixups.auto_fixed_pour_classes_of_board`) as
    `fix_dsn` resolves it from the board and the DSN. Returns
    `(router_args, log_lines)`; a failed board read is a WARNING line and no
    flags."""
    if opts.fixed_pour_mode != "engine" or not opts.fixed_pour_classes:
        return [], []
    classes = opts.fixed_pour_classes
    if classes == options.AUTO:
        try:
            classes = dsn_fixups.auto_fixed_pour_classes_of_board(board_facts(board, pcbnew))
        except Exception as e:  # never block routing on the fix-up
            return [], [f"WARNING: board snapshot fix-ups skipped: {e!r}"]
    args = dsn_fixups.engine_plane_obstacle_args(classes)
    return args, [f"pours of {', '.join(classes) or 'no class'} kept clear by the router ({' '.join(args)})"]


def prepare_snapshot(board, pcbnew, work_dir, stem, board_dir, opts):
    """K10: the `board_input = kicad_pcb` counterpart of `prepare`. Takes the
    snapshot (`snapshot.take`), after locking the tracks first for
    `fix_existing` as `prepare` does (the reader writes a locked track as
    fixed copper). No DSN text is fixed up: `class_widths`, `drop_pour_nets`,
    `route_on_plane_layers` and the fixed pour wires go to the router as flags
    its reader applies (K14, `snapshot_fixups`); `edge_and_pad_keepouts` is
    ignored, because the reader writes those keepouts itself
    (`options.input_ignored`, K13.7), and `snapshot.notes` says so in the log.
    `fixed_pour_mode = engine` works here too, `auto` included, because it is
    an engine flag (`snapshot_engine_args`).
    Returns `(ok, router_args, log_lines, locked_tracks)`."""
    try:
        filled = refill(board, pcbnew)
    except Exception as e:
        return False, [], [f"ERROR: routing preparation failed: {e}"], []
    locked = kicad_board.lock_tracks(board) if opts.fix_existing else []
    ok, lines = snapshot.take(board, pcbnew, work_dir, stem, board_dir)
    lines = filled + lines
    if not ok:
        kicad_board.restore_locks(locked)
        return False, [], lines, []
    args, engine_lines = snapshot_engine_args(board, pcbnew, opts)
    lines += engine_lines
    fix_args, fix_lines = snapshot_fixups(board, pcbnew, opts)
    args += fix_args
    lines += fix_lines
    lines += snapshot.notes(opts)
    if locked:
        lines.append(f"fix existing: {len(locked)} track(s) and via(s) in the snapshot as fixed")
    return True, args, lines, locked


def after_import(board, ses, pcbnew=None, originals=(), board_input=options.INPUT_DSN):
    """Corrects what KiCad's `ImportSpecctraSES` gets wrong: the drill of
    routed vias (`ses_vias`), then the coordinates of the input copper
    (`restore_copper`, Q22.3: `originals` are `restore_copper.originals` taken
    before the import; K13.8: the original track arcs, with `pcbnew` given).
    Returns the log lines; never raises."""
    lines = []
    try:
        changed = kicad_board.fix_via_drills(board, ses_vias.vias(_read(ses)))
    except Exception as e:  # the import itself succeeded; keep it
        lines.append(f"WARNING: via drill check skipped: {e!r}")
    else:
        if changed:
            lines.append(f"via drills: {changed} via(s) set to their session padstack's drill")
    if originals:
        try:
            editor = restore_copper.PcbnewEditor(board, pcbnew) if hasattr(pcbnew, "PCB_ARC") else None
            counts = restore_copper.restore(board.GetTracks(), originals, restore_copper.bound_nm(board_input), editor)
            lines.append(restore_copper.log_line(counts))
        except Exception as e:  # keep the import as it is
            lines.append(f"WARNING: input copper not restored: {e!r}")
    if pcbnew is not None:
        try:
            lines += refill(board, pcbnew, "after session import")
        except Exception as e:
            lines.append(f"WARNING: zones could not be refilled after session import: {e}")
    return lines
