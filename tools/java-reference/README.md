# Java reference harness

Runs the Java Freerouting engine on every board in `fixtures/` and stores what it
produced in `fixtures/reference/`, so the Rust port has something concrete to compare
against.

```sh
tools/java-reference/run.sh ../freerouting load
tools/java-reference/run.sh ../freerouting route 100 00:00:45
```

The first argument is a checkout of `danbrunski/freerouting` (the port is pinned to
`fc4bd20`). The script needs `python3`, `pip`, `curl` and the Gradle wrapper in that
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
