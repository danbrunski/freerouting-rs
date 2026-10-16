# Batch-pipeline probe

`BatchProbe.java` is the Java side of `crates/freerouting/tests/autoroute_batch_control.rs`
(A10): the item selection and pass reordering of `BatchAutorouter`/`AutoroutePassRunner`
(`items`), the component and pin order of `BatchFanout` (`fanout-order`) and
`AutorouteUnroutedReport.build` (`unrouted-report`). It lives in package
`app.freerouting.autoroute.pipeline` to call package-private methods and reads private
state by reflection. It carries a verbatim copy of the board probes' `Boards` class.

Its output is committed under `crates/freerouting/tests/data/batch_probe/<kind>/<board>.jsonl`
(not `fixtures/reference/probe/`, which the re-pin owns). To regenerate, from a detached
worktree of the pin (`a773a70ec`):

```sh
P=tools/java-reference/probes/batch/BatchProbe.java
D=crates/freerouting/tests/data/batch_probe
for b in Issue026-J2_reference Issue508-DAC2020_bm01 Issue269-z10_module; do
  tools/java-reference/probe.sh PIN $P items fixtures/$b.dsn $D/items/$b.jsonl
done
tools/java-reference/probe.sh PIN $P fanout-order fixtures/Issue558-dev-board.dsn \
  $D/fanout-order/Issue558-dev-board.jsonl
tools/java-reference/probe.sh PIN $P unrouted-report \
  fixtures/PCBench-retroreflectors-CONGFLOCK.dsn \
  $D/unrouted-report/PCBench-retroreflectors-CONGFLOCK.jsonl
tools/java-reference/probe.sh PIN $P shuffle 2000046 40   # the shuffle test's values
```

When A01's `AutorouteProbe.java` lands, these kinds can move into it.
