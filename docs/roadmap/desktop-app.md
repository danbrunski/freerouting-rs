# Desktop app, KiCad plugin and releases

Recorded 2026-10-15. This is the roadmap for everything a user touches
directly: the desktop app (`freerouting-gui`), the KiCad plugin, a direct
`.kicad_pcb` reader, and the release builds. It covers items 5, 6 and 7 of
"What 'complete' means" in [port-completion.md](port-completion.md). The
checklists are the [G series](tasks/G00-desktop-app-README.md), the
[K series](tasks/K00-kicad-README.md) and the
[R series](tasks/R00-release-README.md).

Sources: the GUI, plugin and packaging research report of 2026-10-15
(Java GUI inventory, renderer measurements, plugin contract, KiCad
exporter facts, packaging tools); the KiCadRoutingTools (KRT) report (§6
KiCad I/O, §8 plugin and packaging); the handover notes
[gui.md](../development/gui.md) and [kicad-plugin.md](../development/kicad-plugin.md).

## Where it stands

| Part | State on 2026-10-15 |
|---|---|
| Desktop app | A board viewer: DSN and SES, layers, hover, labels. It drops the engine board after building the scene, draws by transforming every vertex on the CPU each frame, and has no tests beyond scene and tessellation units |
| KiCad plugin | Exports DSN, runs the CLI, imports SES. Route reports "not implemented" until the CLI routes. No tests; not yet confirmed by the owner inside KiCad |
| `.kicad_pcb` reader | None. Java's `io.kicad` reads a lossy JSON format from the Java plugin's experimental mode; it is not ported (decision below) |
| Release builds | None. CI is written but not installed (F01.2) |

## The desktop app (G series)

**The GUI draws and maps input; the engine decides** (decision D7). Every
count, list, overlay and edit comes from an engine call, and every GUI
checklist names the engine work it waits for. That is why most of the
series is gated on other series: the change journal and content hash
(B01), the selection filter (B04), item geometry, item info and the
rules-edit facade (B08), the settings model (C02, C03), the writers
(C04–C06), the router's events and batch loop (A08, A13), and the
interactive engine (I01–I04). G00 lists each dependency and the item that
needs it.

**What can start now.** G01 makes the app keep the board in a `Session`,
gives the crate a library target for egui_kittest, adds frame statistics,
bench mode and in-app screenshots, and switches to Java's mouse and key
model. G02 moves the board onto the GPU. G03 adds the ratsnest, DRC
markers and the Info lists, all from engine calls that exist today. G04
adds the read-only windows and display options.

**Why the renderer is rebuilt now.** `Issue190-processor.Z80` is 350,056
triangles, and today every vertex is transformed on the CPU every frame and
re-tessellated by egui. Real boards are ten times larger. Live redraw while
routing (G07) and dragging (G09) need geometry that stays on the GPU and
changes in small pieces. G02 uploads each chunk of a batch once, culls
chunks outside the viewport, and patches only the chunks a delta touches.
Chunks hold items in `ItemId` order, so a patched chunk is bit-identical to
a fresh one, which makes patching testable by equality. The CPU painter
stays as a fallback.

**How the GUI and the router meet.** The routing job owns its board and runs
on its own thread; the GUI never locks a board the router holds. Events go
over a bounded channel: progress counters, deltas of removed ids and added
geometry drained from the B01 journal, and a final event that hands the
board back. Deltas are emitted at work boundaries, never on a timer the
router reads, so watching a run cannot change it. G07 proves that: a GUI
run and a CLI run with equal settings write byte-identical SES files.

**Java parity in the GUI** means the same features, texts and keys, not the
same Swing structure. Texts come from Java's English bundles; keys follow
Java (A, Z, O, C, F, G, V, U, B, S, R, D, W) with Home and Ctrl+Z/Ctrl+Y as
aliases. Some Java features are proposed for dropping: `.frb` files, the
Fusion and KiCad JSON exports, the tutorial board, telemetry. Several
selection edits in Java are wired to empty methods
(`GuiBoardLegacyEditActions`), so "matching Java" there means deciding what
to add. These are G00's open questions.

**How it is verified.** "Feels fast" and "looks right" are not acceptance:
- egui_kittest tests drive the real `App` through its command enum and
  AccessKit labels, on all three systems;
- snapshots are generated and compared on Linux with lavapipe only, since
  anti-aliasing differs across backends;
- headless screenshots of the real binary under Xvfb, which the implementer
  opens and describes;
- frame targets FT0–FT7 measured with `--bench` in release on a quiet
  machine, on Issue190 and on a synthetic 4×4 tiling of it (174,160 items,
  about 5.6 M triangles), plus the owner's Windows GPU by hand;
- deterministic counters (triangles, draws, chunks drawn, bytes uploaded,
  full rebuilds) that tests assert exactly.

## The KiCad plugin (K01–K03)

**The plugin stays a thin launcher.** It exports a DSN with KiCad's own
exporter, runs the CLI, and imports the SES. KRT does the opposite and
runs its router inside KiCad's Python; its froze-KiCad bug (a worker-thread
`SaveBoard`) is one reason not to. The plugin's only intelligence is the
rule-area fix below, a version handshake, and parsing the CLI's JSON
progress.

**The rule-area lesson.** KiCad picks the DSN keepout type from a rule
area's tracks and vias flags only. An area that bans only footprints, pads
or copper pour becomes a full `keepout`. On `canary-pwr` that is the
26 × 26 mm fan inlet on B.Cu (pads and footprints banned), which blocked
routing that KiCad allows. K01.2 rewrites such areas in the exported DSN:
footprint bans become `place_keepout`, pad-only and pour-only areas are
removed. It works on board-level areas; footprint-owned ones wait for the
direct reader.

**Tracks-only areas keep Java's meaning.** KiCad writes them as
`wire_keepout`, which Java and the Rust parser read as a full keepout, so
they block vias too (`Issue754-avionics_hub` has one over its whole Ground
layer). Making them trace-only would change routing results against Java.
It is proposed as a measured Q-series divergence after board parity, not a
default (K00 question 1).

**The CLI contract** the plugin relies on is C08's: JSON progress lines,
a result manifest, `stop` on stdin to finish and keep the best board, exit
codes 0/1/2/130, `--version`. K02 builds options, progress, Stop and
Abort, a summary and "Route in GUI" on it, and proves the whole path on
KiCad's demo boards and `canary-pwr`, graded by KiCad's own DRC (F02.6)
against Java Freerouting on the same exports.

**Packaging** follows KRT: one PCM zip per version carrying every
platform's executables, a resolver that picks the right one, and a version
gate in CI. The plugin's identifier differs from the Java plugin's, which
the owner runs alongside.

**Hard rules** for anyone touching KiCad (from `kicad-plugin.md`): never
close or kill KiCad (Windows or the Linux one in WSL), never touch the Java
plugin, never import the plugin package in a standalone KiCad Python,
rebuild the Windows exes natively (mingw on PATH) because KiCad launches
the Windows checkout's `target\release`.

## The direct `.kicad_pcb` reader (K04–K08)

**Why a reader at all.** KiCad's DSN export loses rule-area kinds, turns
track arcs into single chords, rounds shape coordinates to six significant
digits (about 1 µm above 100 mm), drops copper zones owned by footprints,
and depends on SWIG calls
(`ExportSpecctraDSN`, `ImportSpecctraSES`) that KiCad master may already
have removed (KRT's claim, to be checked in K01.5). Java's `io.kicad` is no
help: it reads a JSON format that has no keepout kinds, no holes, no arcs
and no pad rotation, and it sorts outline corners by angle, which scrambles
concave outlines.

**Design.** A new crate `freerouting-kicad` parses `.kicad_pcb` (KiCad 6 to
10, with KRT's list of pitfalls: escaped quotes, one-line boards, the
KiCad 10 net dialect, absolute pad angles, footprint-owned areas in board
coordinates) and produces a DSN syntax tree. `build_board` stays the only
board builder, and the tree can be written as DSN for debugging. The tree
uses KiCad's own DSN frame (µm, resolution 10 per µm, Y negated) but keeps
KiCad's full nm precision, so the reader can be compared with KiCad's
export of the same board within half a rounding step of the export. Rule
areas map from all five flags; tracks-only areas keep Java's meaning, as
above, but the tree keeps them distinct so a later change is one line.

**How it is verified.** On a corpus of KiCad 6–10 boards (the Java repo's
fixtures, KiCad's demos, the owner's boards out of the repo), `build_board` of
the reader's output is compared with `build_board` of KiCad 10's DSN export:
item counts by kind, nets, board statistics and DRC, with every difference
listed and explained (the rule-area fixes, the arcs, net-class order,
footprint copper zones). KiCad's own DRC grades routed results through
F02.6, and K07.4 routes a board and imports the session into KiCad to check
the coordinate frame end to end. Open: whether a board resolution finer
than 0.1 µm is ever worth it (K00 question 2), and whether a direct writer
(K08) is needed before KiCad 11.

port-completion.md puts the importer outside "complete". It has no router
dependency, so it can run in parallel with anything, but it is scheduled
after the plugin routes unless the owner wants it sooner.

## Release builds (R series)

One tag-triggered workflow builds Linux (on Ubuntu 22.04, glibc 2.35 floor,
tar and AppImage, no bundled system libraries), Windows (MSVC with a static
C runtime, zip and MSI) and macOS (both architectures joined with `lipo`,
an ad-hoc signed app and DMG). cargo-packager is the one tool that makes
all three installers from one config; it is pinned, with native fallbacks,
because its last release is old. A version gate runs first and fails a tag
in seconds when the workspace, the plugin and its metadata disagree. A
build stamp gives `--version` and About the same commit id. Releases are
drafts until the owner publishes them. Signing (R02) waits for the owner's decision.

## Risks

- **The router's event stream is not yet designed** (A08). If the router
  lands without deltas, the GUI falls back to full rebuilds during routing
  and misses FT4 and FT5. G00 states what G07 needs so A08 can include it.
- **GPU and driver variety.** The paint callback must behave on DX12,
  Metal, Vulkan, GL and lavapipe. The CPU fallback and the snapshot
  comparison between the two paths contain that risk; the owner's Windows run is
  the real check.
- **Most of the G series waits on engine work** (B, C, A, I). G01–G04 can
  proceed now; the rest is sequenced behind the port's critical path.
- **KiCad's exporter and API move.** SWIG removal, KiCad 11 exporting pours
  as wire polygons, and the pour-flag accessor rename between 9.0 and 10.0
  all affect the plugin. K01.5 records the facts; the direct reader is the
  long-term answer.
- **The KiCad coordinate frame** must line up for a `.kicad_pcb` input to
  produce a session KiCad places correctly (K07).
- **Young tooling.** cargo-packager's last release is old, WiX v3 is out of
  support upstream, and an egui upgrade means upgrading egui_kittest in the
  same commit and regenerating snapshots.
- **Two KiCads on one machine.** The owner's plugin runs in Windows KiCad 10.0.5;
  WSL has KiCad 10.0.7. Headless results should name the version used.
