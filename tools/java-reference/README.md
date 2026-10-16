# Java reference harness

Runs the Java Freerouting engine on every board in `fixtures/` and stores what it
produced in `fixtures/reference/`, so the Rust port has something concrete to compare
against.

```sh
tools/java-reference/run.sh ../freerouting load
tools/java-reference/run.sh ../freerouting route 100 00:00:45
```

The first argument is a checkout of `danbrunski/freerouting` (the port is pinned to
`cda540f`). The script needs `python3`, `pip`, `curl` and the Gradle wrapper in that
checkout, which it only uses to resolve dependencies. You don't need a JDK 25 install:
the script takes a Java 25 runtime from the `jdk4py` wheel and compiles with the Eclipse
compiler (ECJ). Set `JAVA=/path/to/java` to use your own Java 25+ instead. Build
products go to `target/java-reference/`.

## Outputs

| Path | Contents |
| --- | --- |
| `fixtures/reference/load/<board>.json` | Result of `DsnReader.readBoard` (`success`, `outline_missing`, `parse_error`, or `exception`) and the board's `BoardStatistics`: layers, item counts by type, components, pads, nets, incomplete connections, clearance violations, bounds. |
| `fixtures/reference/route/<board>.json` | Final job state, wall-clock routing time, router score and `BoardStatistics` after the batch autorouter and optimizer ran with the given pass and time limits. |
| `fixtures/reference/route/<board>.ses` | The SES session file the Java router wrote. |

`load` output is deterministic and should be byte-identical on every run. `route`
output isn't: Java's maze search uses an unseeded `java.util.Random` and the router
runs against a wall-clock timeout, so treat routed results as a baseline for completion
rate, via count, length, score and speed, not as an exact target.

`route` skips boards that already have a `route/<board>.json`, so an interrupted run
picks up where it stopped. Set `REFERENCE_REDO=1` to rerun everything.

## Known Java failures

- `Issue006-LPC18XX_43XX_SCH.dsn` is not a PCB file (`parse_error`).
- `Issue179-Autorouter_PCB1_2023-3-24.dsn` throws a `NullPointerException` in Java
  (`exception`).

## Probes

`probe.sh` compiles and runs one Java probe against the Java classes of a pin checkout, for
differential tests whose expected values must come from Java:

```sh
git -C ../freerouting worktree add --detach <scratchpad>/java-pin <pin>
TMPDIR=<scratchpad> tools/java-reference/probe.sh <scratchpad>/java-pin tools/java-reference/probes/JavaRandomProbe.java
# 30 63 48 84 70
# [8, 1, 4, 3, 9, 6, 0, 2, 5, 7]
git -C ../freerouting worktree remove --force <scratchpad>/java-pin
```

- The probe is any `.java` file with a `main`; its class is the file name, prefixed by its
  `package` if it has one. Extra arguments go to `main`. Stdout and the exit status are the
  probe's. A compile error prints ECJ's message on stderr and exits 1.
- When `target/java-reference/classes` (or the checkout's `build/reference-classpath.txt`) is
  missing, the script builds it through `run.sh` with a fixture filter that matches no board,
  so no reference file is written.
- The cached classes must come from the checkout given: `run.sh` records the commit in the
  generated `Constants.java`, and `probe.sh` exits 2 when that differs from the checkout's
  HEAD. Delete `target/java-reference/classes` to rebuild for another pin.
- Never point it at `../freerouting` itself, which another session edits; use a detached
  worktree. `REFERENCE_WORK_DIR` and `JAVA` work as for `run.sh`; the probe is compiled into a
  temporary directory under `TMPDIR`.
- `probes/` holds probes worth keeping. `JavaRandomProbe.java` prints the `java.util.Random`
  values `freerouting_geometry::jmath` is tested against.
