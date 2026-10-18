"""What the plugin does to the DSN after KiCad's export, in one place (K09).

`plugin.py` (inside KiCad) and `kicad-plugin/headless_run.py` (pcbnew in a
plain Python, for graded runs) call the same functions, so a headless run
routes exactly what a plugin run routes.

The pcbnew module is passed in, never imported, so this file imports nothing
outside the standard library and the plugin's pure modules. Call it on
KiCad's UI thread only.

`prepare_snapshot` is the K10 form: the board snapshot the router reads
directly, with none of those DSN steps (`snapshot.py`).

Order: KiCad's export, the rule-area fix-up (K01.2, `rule_areas`), then the
K09 fix-ups (`dsn_fixups`). After `ImportSpecctraSES`, `after_import` sets the
drills KiCad's import takes from the net class instead of the session. A fix-up that fails is logged with a WARNING and
leaves the DSN as it was before that step; it never blocks routing.
"""

try:  # inside the plugin package
    from . import dsn_fixups, kicad_areas, kicad_board, options, rule_areas, ses_vias, snapshot
except ImportError:  # tests and headless scripts put this folder on sys.path
    import dsn_fixups
    import kicad_areas
    import kicad_board
    import options
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
    locked = kicad_board.lock_tracks(board) if opts.fix_existing else []
    if not export(board, dsn):
        kicad_board.restore_locks(locked)
        return False, [], [], []
    lines = fix_rule_areas(board, pcbnew, dsn) if opts.rule_area_fixup else []
    args, more = fix_dsn(board, pcbnew, dsn, opts)
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
    locked = kicad_board.lock_tracks(board) if opts.fix_existing else []
    ok, lines = snapshot.take(board, pcbnew, work_dir, stem, board_dir)
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


def after_import(board, ses):
    """Corrects what KiCad's `ImportSpecctraSES` gets wrong (`ses_vias`):
    the drill of routed vias. Returns the log lines; never raises."""
    try:
        changed = kicad_board.fix_via_drills(board, ses_vias.vias(_read(ses)))
    except Exception as e:  # the import itself succeeded; keep it
        return [f"WARNING: via drill check skipped: {e!r}"]
    return [f"via drills: {changed} via(s) set to their session padstack's drill"] if changed else []
