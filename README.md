# freerouting-rs

A Rust port of [Freerouting](https://github.com/freerouting/freerouting), the
Specctra DSN/SES PCB autorouter.

Work in progress: nothing routes yet. See [docs/PORT_PLAN.md](docs/PORT_PLAN.md)
for the module map, crate layout and porting order.

## Layout

| Crate | Purpose |
| --- | --- |
| `freerouting-geometry` | Planar geometry with exact integer/rational math |
| `freerouting-dsn` | Specctra DSN/SES/rules parsing and writing |
| `freerouting` | Routing engine library: board, rules, DRC, autorouter, optimizer |
| `freerouting-cli` | The `freerouting` command-line tool |

## License

GPL-3.0, the same as upstream Freerouting, since this is a derivative work.
