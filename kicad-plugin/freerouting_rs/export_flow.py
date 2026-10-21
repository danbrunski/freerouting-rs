"""What the plugin does to the DSN after KiCad's export, in one place (K09).

`plugin.py` (inside KiCad) and `kicad-plugin/headless_run.py` (pcbnew in a
plain Python, for graded runs) call the same functions, so a headless run
routes exactly what a plugin run routes.

The pcbnew module is passed in, never imported, so this file imports nothing
outside the standard library and the plugin's pure modules. Call it on
KiCad's UI thread only.

`prepare_snapshot` is the K10 form: the board snapshot the router reads
directly, with none of those DSN steps (`snapshot.py`).

Order: refill zones, KiCad's export, the rule-area fix-up (K01.2, `rule_areas`), the
Edge.Cuts, Margin and board mask-aperture keepouts (Q18.5, Q18.7, `edge_graphics`,
always on), the copper graphics and texts (Q22.4, `copper_graphics`, always on), then the
K09 fix-ups (`dsn_fixups`); the pads' solder-mask apertures go to the router as
a flag (Q18.7, `solder_mask_args`). After `ImportSpecctraSES`, `after_import` sets the
drills KiCad's import takes from the net class instead of the session. A fix-up that fails is logged with a WARNING and
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
        options,
        restore_copper,
        rule_areas,
        ses_vias,
        snapshot,
    )
except ImportError:  # tests and headless scripts put this folder on sys.path
    import copper_graphics
    import dsn_fixups
    import edge_graphics
    import kicad_areas
    import kicad_board
    import options
    import restore_copper
    import rule_areas
    import ses_vias
    import snapshot


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
    if opts.rule_area_fixup:
        lines += fix_rule_areas(board, pcbnew, dsn)
    lines += fix_edge_graphics(board, pcbnew, dsn)
    lines += fix_copper_graphics(board, pcbnew, dsn)
    mask_args, mask_lines = solder_mask_args(board, pcbnew, dsn)
    lines += mask_lines
    args, more = fix_dsn(board, pcbnew, dsn, opts)
    args = mask_args + args
    if locked:
        more.append(f"fix existing: {len(locked)} track(s) and via(s) exported as fixed")
    return True, args, lines + more, locked


def prepare_snapshot(board, pcbnew, work_dir, stem, board_dir, opts):
    """K10: the `board_input = kicad_pcb` counterpart of `prepare`. Takes the
    snapshot (`snapshot.take`), after locking the tracks first for
    `fix_existing` as `prepare` does (the reader writes a locked track as
    fixed copper). No DSN fix-up applies: `options.input_conflicts` keeps the
    options that need one off. `fixed_pour_mode = engine` with named classes
    is the one K09 option that works here, because it is an engine flag.
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
    args = []
    if opts.fixed_pour_mode == "engine" and opts.fixed_pour_classes and opts.fixed_pour_classes != options.AUTO:
        args = dsn_fixups.engine_plane_obstacle_args(opts.fixed_pour_classes)
        lines.append(f"pours of {', '.join(opts.fixed_pour_classes)} kept clear by the router ({' '.join(args)})")
    lines += snapshot.notes(opts)
    if locked:
        lines.append(f"fix existing: {len(locked)} track(s) and via(s) in the snapshot as fixed")
    return True, args, lines, locked


def after_import(board, ses, pcbnew=None, originals=(), board_input=options.INPUT_DSN):
    """Corrects what KiCad's `ImportSpecctraSES` gets wrong: the drill of
    routed vias (`ses_vias`), then the coordinates of the input copper
    (`restore_copper`, Q22.3: `originals` are `restore_copper.originals` taken
    before the import). Returns the log lines; never raises."""
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
            counts = restore_copper.restore(board.GetTracks(), originals, restore_copper.bound_nm(board_input))
            lines.append(restore_copper.log_line(counts))
        except Exception as e:  # keep the import as it is
            lines.append(f"WARNING: input copper not restored: {e!r}")
    if pcbnew is not None:
        try:
            lines += refill(board, pcbnew, "after session import")
        except Exception as e:
            lines.append(f"WARNING: zones could not be refilled after session import: {e}")
    return lines
