# K01 — KiCad plugin work that doesn't need the router

Opened 2026-10-15. The plugin (`kicad-plugin/`) exports a DSN, runs the CLI,
and imports the SES. Routing waits for the port, but four things don't:
- tests, because nothing in the plugin is tested today;
- the owner lesson from canary-09: KiCad's DSN export turns a rule area
  that bans only footprints into a full `(keepout …)`, which cost
  `canary-pwr` its 26 × 26 mm fan inlet on B.Cu;
- guards that keep the plugin from freezing or confusing KiCad;
- facts about KiCad's SWIG API, which the plugin relies on.

This checklist is the report's PLG-1, with KRT's UI-thread, installer and
IPC items (KRT report §8) folded in.

**Prerequisites:** none. K01.3's real check needs C08's `--version`; until
then it is tested against a fake executable.

## What exists and what this adds

| Exists (at `70389b7`) | State |
|---|---|
| `kicad-plugin/freerouting_rs/plugin.py` (177 lines) | SWIG `pcbnew.ActionPlugin` and its dialog: Route (`ExportSpecctraDSN`, run `freerouting -de x.dsn -do x.ses`, `ImportSpecctraSES`), Open in viewer, Import SES. Routing runs the subprocess in a thread; pcbnew calls stay on the UI thread |
| `kicad-plugin/freerouting_rs/tools.py` (141 lines) | No pcbnew or wx imports. Executable lookup (`bin_dir`, `FREEROUTING_RS_BIN`, PATH, `~/.cargo/bin`, then `wsl_bin_dir` on Windows), WSL path translation, UTF-16 `wsl.exe` output |
| `kicad-plugin/freerouting_rs/__init__.py` (3 lines) | Imports `plugin` and calls `register()`, so importing the package needs pcbnew and wx |
| `kicad-plugin/install.py` (71 lines) | Copies `freerouting_rs/` into KiCad's user `scripting/plugins` and writes `settings.json` (executable locations) |
| Tests | none |
| Dan's machine | KiCad 10.0.5 on Windows (`host_version` in `canary-pwr.dsn`); plugin at `C:\Users\thebo\Documents\KiCad\10.0\scripting\plugins\freerouting_rs`; the Java plugin (`app.freerouting.kicad-plugin`) installed alongside in `3rdparty\plugins\app_freerouting_kicad-plugin` |
| KiCad's export of rule areas | `canary-pwr.kicad_pcb` zone "Fan inlet keep-out" on B.Cu: `(pads not_allowed) (footprints not_allowed)`, tracks, vias and copper pour allowed, polygon (136.5, 53)–(162.5, 79) mm. KiCad's DSN (`canary-pwr/freerouting-rs/canary-pwr.dsn:64`): `(keepout "" (polygon B.Cu 0 136500 -53000 162500 -53000 …))`, a full keepout. The keepout type is chosen from the tracks and vias flags only (report §0 item 8, from `specctra_export.cpp`, delegated read) |
| KRT | `py_router/ui_thread.py` (issue #688: a worker-thread `SaveBoard` froze KiCad 10.0.5 on Windows 11), `install_plugin.py` (`find_conflicting_pcm_installs`, `disable_pcm_install` into `disabled_pcm_plugins/<version>/`), `py_router/swig_compat.py` |
| Java plugin at `cda540f` | `integrations/KiCad/kicad-freerouting/plugins/{plugin.py,router_dsn.py,process_utils.py}`; it strips Greek letters from the DSN (`router_dsn.py::search_n_strip`) |

What this adds: a pytest suite; `rule_areas.py` and the DSN rewrite; a
version handshake; a worker-thread guard test and a shadow-install guard;
notes on SWIG and the IPC API; Dan's confirmation in KiCad.

## Decisions made here

- **Pure modules, tested with plain Python.** Every module other than
  `plugin.py` and `__init__.py` imports only the standard library. The tests
  (`kicad-plugin/tests/`) put `kicad-plugin/freerouting_rs` on `sys.path`
  in `conftest.py` and import `tools`, `rule_areas` and `version` as
  top-level modules, so the package `__init__` (which registers the plugin)
  never runs. `pytest` is a dev-only `NEW DEP`.
- **The rule-area rewrite works on the DSN text after export**, as the
  report's option (a) chose: no CLI surface, and it works with Java
  Freerouting's DSN files too. The plugin reads each rule area's flags and
  outline from the live board with pcbnew on the UI thread, hands plain
  data to `rule_areas.py`, and writes the rewritten DSN.
- **Mapping** (board-level rule areas, per copper layer):

  | `not_allowed` flags | KiCad's DSN | After the rewrite |
  |---|---|---|
  | tracks and vias (with or without others) | `keepout` | unchanged |
  | vias only | `via_keepout` | unchanged |
  | tracks only | `wire_keepout` (read as `keepout`, blocks vias too) | unchanged, plus a warning (K00 question 1) |
  | footprints, without tracks or vias | `keepout` | `place_keepout` |
  | pads or copper pour only, or no flags | `keepout` | removed |

  The canary fan inlet (pads and footprints) becomes `place_keepout`.
- **Footprint-owned rule areas are not rewritten here.** KiCad writes them
  into the footprint's `(image …)` in image coordinates (for example the
  `(keepout "" (circle F.Cu 3500 2160 4320))` entries in `canary-pwr.dsn`),
  shared by every footprint of that image. The rewrite lists them in a
  warning and leaves them; the direct reader (K06) handles them.
- **The rewrite is on by default** (K00 question 3), with a per-board switch
  in the dialog and one log line per changed area in
  `freerouting-rs/plugin.log` next to the board.
- **Version handshake:** Route is refused when the CLI's major.minor differs
  from the plugin's; Open in viewer still works. An executable that prints
  no version is "unknown", allowed with a warning until C08 lands.

## File ownership

| Item | New files | May edit |
|---|---|---|
| K01.1 | `kicad-plugin/tests/{conftest.py,test_tools.py}`, `kicad-plugin/tests/fake_bin/` | `.github/workflows/ci.yml` (append a plugin-test job, after F01.2) |
| K01.2 | `kicad-plugin/freerouting_rs/rule_areas.py`, `kicad-plugin/tests/test_rule_areas.py`, `kicad-plugin/tests/data/*.dsn` | `kicad-plugin/freerouting_rs/plugin.py` |
| K01.3 | `kicad-plugin/freerouting_rs/version.py`, `kicad-plugin/tests/test_version.py` | `tools.py`, `plugin.py` |
| K01.4 | `kicad-plugin/tests/test_threads.py`, `kicad-plugin/tests/test_install.py` | `kicad-plugin/install.py` |
| K01.5 | — | `docs/development/kicad-plugin.md` |
| K01.6 | — | `docs/development/kicad-plugin.md` |

## Checklist

- [ ] **K01.1 — A headless test suite.**
  - `test_tools.py`: lookup order (each source wins over the later ones,
    using temp dirs and a fake executable in `tests/fake_bin/`), WSL path
    translation (`C:\Users\me\b.dsn` → `/mnt/c/Users/me/b.dsn`, a UNC path
    raises), `Tool.command` for native and WSL tools, and UTF-16 decoding
    of `wsl.exe` output in `finish_route`.
  - CI: a job running `python -m pytest kicad-plugin/tests` on Linux and
    Windows, appended to F01.2's workflow once it exists.
  - **Accept:** `python3 -m pytest kicad-plugin/tests` passes here with no
    pcbnew or wx installed; the CI job is green on both OSes (partial with
    "needs F01.2" until the workflow exists).

- [ ] **K01.2 — Rule-area fix-up of the exported DSN (owner lesson).**
  - First, a headless probe in KiCad's Python (Windows 10.0.5:
    `/mnt/c/Program Files/KiCad/10.0/bin/python.exe`; or WSL 10.0.7: system
    `python3`, which imports `pcbnew`), never importing `freerouting_rs`, on
    a *copy* of `canary-pwr.kicad_pcb`: list every zone
    with `GetIsRuleArea()`, its layers, its outline and its flags. Confirm
    the accessor names the report gives (`GetDoNotAllowTracks`, `…Vias`,
    `…Pads`, `…Footprints`; the pour flag is `GetDoNotAllowCopperPour` up
    to 9.0 and `GetDoNotAllowZoneFills` from 10.0) with `dir(zone)`, and
    record the result in the done note.
  - `rule_areas.py`:
    `rewrite(dsn_text, areas) -> (new_text, log_lines)`, where `areas` is a
    list of plain dicts (`layers`, `outline` in DSN µm with Y negated,
    `holes`, the five flags, `name`). It tokenizes the DSN with a small
    quote-aware s-expression scanner (no regular expressions over
    parentheses), finds the `(keepout …)` and `(wire_keepout …)` scopes
    directly inside `(structure …)`, matches each to an area by layer and
    by polygon vertices within 1 µm, and applies the mapping table.
    Unmatched keepouts are left as they are, with a log line. The rest of
    the text is kept byte for byte.
  - `plugin.py` builds `areas` from `board.Zones()` on the UI thread after
    `ExportSpecctraDSN`, calls `rewrite`, writes the DSN, and appends the
    log lines to `freerouting-rs/plugin.log`.
  - **Accept:** `test_rule_areas.py`:
    - a synthetic DSN with five board-level areas (tracks+vias, vias only,
      tracks only, footprints only, pads+pour only) gives exactly: keepout
      kept, via_keepout kept, wire_keepout kept with a warning, one
      `place_keepout`, and the last area removed;
    - `fixtures/Issue754-avionics_hub.dsn` with an area list matching its
      `wire_keepout` comes back byte-identical, with the "blocks vias too"
      warning;
    - a keepout inside an `(image …)` is never changed;
    - on a KiCad 10 export of a copy of `canary-pwr` (made headless; the
      exported DSN stays out of the repo), the fan-inlet polygon
      (`136500 -53000 … 136500 -79000` on B.Cu) is under `place_keepout`
      and no longer under `keepout`; every other line is unchanged (a diff
      of the two texts shows only that scope).

- [ ] **K01.3 — Version handshake.**
  - `version.py`: `__version__` equal to the workspace version in
    `Cargo.toml` (R01.1's gate keeps them equal).
  - `tools.query_version(tool, timeout=10)` runs `freerouting --version`
    and parses `freerouting <x.y.z> (<sha>)` (C08's format). The dialog
    shows the router's and the plugin's versions. A major.minor mismatch
    disables Route with a message naming both and the fix (rebuild, or
    reinstall the plugin); an unparsable answer shows "unknown" and a
    warning.
  - **Accept:** `test_version.py` with fake executables printing `freerouting
    0.1.0 (abc1234)` (accepted), `freerouting 0.2.0 (abc1234)` (Route
    refused, message names both versions), garbage and a non-zero exit
    ("unknown", allowed with a warning), and one that sleeps past the
    timeout ("unknown"). The real check against C08's binary is added to the
    done note when C08 lands.

- [ ] **K01.4 — UI-thread and shadow-install guards.**
  - `test_threads.py`: parses `plugin.py` with `ast`, finds every function
    passed as `threading.Thread(target=…)` (including lambdas), follows the
    calls it makes inside the package, and fails if any reaches
    `pcbnew.*` or `wx.*` other than `wx.CallAfter`. Today's routing worker
    calls only `tools.finish_route`, so it passes.
  - `install.py`: before copying, look for another plugin folder under the
    same KiCad version (`scripting/plugins` and `3rdparty/plugins`) whose
    `metadata.json` identifier or folder name is ours (K03 sets the
    identifier); move it to `<KiCad user dir>/disabled_pcm_plugins/<version>/`
    with a printed path, or refuse with `--keep-pcm`. Never touch a folder
    whose identifier is `app.freerouting.kicad-plugin` (the Java plugin).
  - **Accept:**
    - `test_threads.py` passes, and fails on a planted test copy of
      `plugin.py` whose worker calls `pcbnew.Refresh()` (negative control).
    - `test_install.py` on a fake KiCad tree in a temp dir: a shadow copy
      with our identifier is moved and reported; the Java plugin's folder
      is left untouched (checksums before and after).

- [ ] **K01.5 — SWIG, IPC and KiCad version notes.**
  - Add a section to `kicad-plugin.md`:
    - whether KiCad master removed SWIG and the Python bindings (KRT's
      `py_router/swig_compat.py` docstring says 2026-03, commit
      `65a442b1`); check it on KiCad's GitLab and give the link;
    - which IPC-API (`kipy`) calls, if any, export Specctra DSN or import a
      session, and what replaces `ExportSpecctraDSN` and `ImportSpecctraSES`
      otherwise (K08's direct writer, K04–K07's reader);
    - the pour-flag accessor rename between 9.0 and 10.0, and whether the
      plugin needs a fallback for KiCad 9 (`hasattr` on the zone).
  - **Accept:** the section exists; each claim is marked verified (with a
    URL or a probe result) or unverified.

- [ ] **K01.6 — Dan confirms the plugin in KiCad.** *Manual, Dan.*
  - After the orchestrator reinstalls the plugin on Windows
    (`install.py --bin-dir target\release`, then Tools > External Plugins >
    Refresh Plugins; never restarting KiCad), Dan checks: the toolbar
    button, the dialog with both versions, Open in viewer, Import SES, and
    the rule-area log on `canary-pwr` naming the fan inlet.
  - **Accept:** Dan's confirmation, with the date and what he saw, in
    `kicad-plugin.md` "Verified". Until then the item is partial with
    "needs Dan".
