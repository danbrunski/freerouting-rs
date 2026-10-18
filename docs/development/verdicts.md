# Verdicts: why each incomplete connection is incomplete (Q06)

`freerouting-verdicts/1` is the report of the `verdicts` diagnostic
(`crates/freerouting/src/quality/verdict.rs`, attribution in
`quality/attribution.rs`). It gives every incomplete connection of the final
board exactly one verdict, with the evidence for it. It is report-only: the
SES is byte-identical with the diagnostic on and off.

## Turning it on

- CLI: `--router.special.diagnostics=verdicts`, or `"special": {"diagnostics":
  ["verdicts"]}` in a settings file. The report goes under the top-level key
  `verdicts` of the result manifest (`--result-json PATH`, C08.4), after
  Java's keys. Without the diagnostic the key is absent.
- Engine: `RunSummary::verdicts` (`Option<VerdictReport>`) of
  `run_pipeline`. The CLI manifest and the GUI's incompletes list read this
  one value; nothing else computes verdicts.

## The verdicts

One per incomplete connection, by this precedence:

| Verdict | When |
|---|---|
| `infeasible` | Q03 (`quality::feasibility`) says an end pin cannot be reached at its class width. Q03 runs once per job on the board before the fanout, for the end pins of incomplete connections that are Q03 candidates (class width above the pin's minimum width). The first such pin by item id is the evidence. |
| `work_limit` | The connection's last attempt failed with its maze search cut off by the work limit. |
| `blocked_by` | The analysis search exhausted and at least one blocking item is on a routed net. |
| `unrouted` | Anything else: only fixed copper, keepouts or the outline in the way ("boxed in"), or the analysis found a path (`search: "found"`: the connection is routable on the final board, typically because a later connection ripped it and the run ended before it was retried). |

**The analysis.** After the optimizer, each incomplete connection is searched
once more with the Freerouting maze search, on a copy of the final board,
from the `from` item's connected set to the rest of its net, with ripup off
and the via and trace costs and budget of the connection's last attempt
(pass 1's when it has none). With ripup off every routed trace and via is an
obstacle, so the search's failure record names the routed nets in the way;
with Java's ripup on, routed copper is entered by ripping and never blocks.

**The last attempt** is the latest connection-router call that searched
(state `ROUTED`, `FAILED` or `INSERT_ERROR`; a skip such as
`NO_UNCONNECTED_NETS` is not an attempt) for a task of the connection's net
whose item is in the connection's `from` or `to` connected set on the final
board. For a net with many pins this is the latest attempt anywhere in the
connected sets, not necessarily one between these two ends.

**Item kinds.** An item is *routed* if it is a trace or via whose fixed state
is unfixed; everything else is *fixed*: pins, keepouts, the outline,
conduction areas, and fixed or shove-fixed wiring.

## Attribution

The failure record has one entry per block the search met: the blocking
item, its net, layer and reason, and the expansion step (the popped maze
element) that met it, located at the middle of that element's entry line.
Duplicates are removed and the entries sorted by `(net, item id, layer,
reason, step)`; at most 65,536 are kept (`coverage.dropped` counts the rest).

Per routed net (net number above 0):
- `blocked`: entries whose item is on that net;
- `unique`: entries where it is the only routed net met at that step;
- `near_start`, `near_dest`: entries within 3 mm of a pin of the search's
  start set (the `from` side) or destination set (the rest of the net),
  measured to the pin's bounding box.

Ranking (KiCadRoutingTools' `count`): nets with `unique == blocked` first,
by `unique`, then `near_start + near_dest`; then the others by `unique +
min(near, unique) + 0.5 · (blocked − unique)`, then `near`; ties by net
number ascending. At most 10 nets are listed.

## Schema `freerouting-verdicts/1`

A member whose value is null is left out (the writer is Gson-style, like the
rest of the manifest). Lengths are mm with four decimals, coordinates DSN mm.

Top level:

| Field | Type | Meaning |
|---|---|---|
| `schema` | string | `"freerouting-verdicts/1"` |
| `count` | int | incomplete connections |
| `counts` | object | `infeasible`, `work_limit`, `blocked_by`, `unrouted`: connections with each verdict |
| `connections` | array | one object per incomplete connection, by ascending `(from.item, to.item)` |

A connection:

| Field | Type | Meaning |
|---|---|---|
| `net` | int | net number |
| `net_name` | string | net name |
| `net_class` | string | net class name |
| `trace_width_mm` | number | class trace width on the `from` item's first layer |
| `from`, `to` | object | the airline's ends (below) |
| `length_mm` | number | airline length |
| `verdict` | string | `infeasible`, `work_limit`, `blocked_by` or `unrouted` |
| `evidence` | object | depends on `verdict` (below) |
| `attempt` | object | the last attempt; absent if the router never tried the connection |
| `search` | string | how the analysis ended: `exhausted`, `found`, `cut` (its own work limit) or `not_run` (no control for the net, or nothing to connect) |
| `coverage` | object | the analysis record: `entries`, `routed`, `fixed`, `dropped` |
| `blockers` | array | the ranked routed nets of the analysis (below), also for verdicts other than `blocked_by` |

An end (`from`, `to`):

| Field | Type | Meaning |
|---|---|---|
| `item` | int | item id |
| `kind` | string | Java's item class name: `pin`, `via`, `polylinetrace`, `conductionarea`, ... |
| `name` | string | `Component-Pin` for a pin, else Java's item string |
| `layers` | array | names of the first and last layer |
| `x_mm`, `y_mm` | number | the airline's end |
| `pad_min_width_mm` | number | smallest pad width of a pin or via; absent for other items |

`attempt`:

| Field | Type | Meaning |
|---|---|---|
| `pass` | int | the pass, from 1 |
| `state` | string | Java's `AutorouteAttemptState` name (`FAILED`, `ROUTED`, `INSERT_ERROR`, ...) |
| `work_units` | int | work units of the attempt (search-tree nodes visited; 64,000 per nominal ms) |
| `work_limit_hit` | bool | the attempt stopped at its work budget |

A blocker:

| Field | Type | Meaning |
|---|---|---|
| `net`, `name` | int, string | the net |
| `blocked`, `unique`, `near_start`, `near_dest` | int | the counts above |

`evidence` by verdict:
- `infeasible`: Q03's pin object (`freerouting-feasibility/1` `pins[]`: `id`,
  `net`, `component`, `pin`, `width_mm`, `corridor_mm`, `throat`, `near`,
  `neck`, ...);
- `work_limit`: `pass`, `work_units`;
- `blocked_by`: `top_net`, `top_net_name` (the first blocker);
- `unrouted`: the coverage object.

## The Java fork's analysis tools

The tools in the Java fork (`docs/development/tools/` at `main`) answer
these questions from a DSN and an SES; with the verdicts they come from the
result JSON:
- `Incompletes.java` (the incomplete connections, by net and component, with
  their lengths): `connections[]` with `net_name`, `from.name`, `to.name`,
  `length_mm`.
- `PowerFailures.java` (which unrouted connections are of the wide classes,
  with the pad width and what is in the way): `trace_width_mm`,
  `from.pad_min_width_mm`, `to.pad_min_width_mm`, the `infeasible` evidence
  (Q03's `near` items and corridor) and `blockers`.
- `GndAirlines.java` (the airlines of one net, by end kind): filter
  `connections[]` on `net_name`, group by `from.kind`/`to.kind` and
  `layers`.
- `ViolDiff.java` (the clearance violations a session adds): not a verdict
  question. The manifest's `board_statistics.clearance_violations` gives
  `pre_existing_count` and `router_introduced_count`; the list itself is the
  DRC report's (the `-drc` mode), not the verdicts'.
