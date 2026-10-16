# S00 — Server and job management (placeholder)

Opened 2026-10-16 by the roadmap review. This series has no checklists. It
exists so the Java classes it will port are listed somewhere, and so the
seams the port leaves for them are named before someone designs around
them. port-completion.md keeps the REST/MCP server and job management
outside "complete"; nothing here starts until Dan asks.

## What Java has

| Java package (at `a773a70ec`) | Lines | What it is |
|---|---:|---|
| `api.*` (`FreeroutingApplication`, filters, `v1/*ControllerV1`, `JobInputResource`, `JobOutputResource`, `JobProgressResource`, `dto/*`, `security/*`) | 9.2k | A Jersey REST API: sessions, jobs, inputs, outputs, progress, system status; API keys; rate limits; OpenAPI and Swagger UI |
| `api.mcp.*` (`McpApplication`, `McpControllerV1`, `McpWebSocketEndpoint`, `OpenApiMcpToolRegistry`) | in the above | An MCP server exposing the same jobs as tools, over HTTP and a WebSocket bridge |
| `management.*` (`HeadlessBoardManager`, `BoardManager`, `BoardLoader`, `CompositeBoardInput`, `jobs/RoutingJobScheduler`, `jobs/RoutingJobSchedulerActionThread`, `sessions/SessionManager`) | 3.3k | Job queue with priorities and one worker thread, sessions, assembling DSN + rules + SES into one job |
| `core.RoutingJob`, `RoutingJobState`, `RoutingJobPriority`, `StopRequestState`, `core.events.*`, `core.results.RoutingResultManifest`, `core.BoardFileDetails`, `core.Session` | 1.5k | The job object: state machine, file details with CRC and size, the result manifest, events |
| `settings.ApiServerSettings`, `McpServerSettings`, `ApiAuthenticationSettings`, `RateLimitSettings`, `NetworkSettings` | 0.2k | Their settings |
| Tests: `api/*` (18), `management/*` (7), `core/RoutingJobFileFormatTest` | — | Contract tests for the endpoints, the scheduler and sessions |

Parts of `management` were ported where the CLI needed them: the load
sequence of `CliRunner.initializeCli`, `RoutingJobScheduler` and
`HeadlessBoardManager` is `freerouting::job` (C03). `BoardFileDetails`'s
CRC and size are not ported; C08's `--result-json` carries what the plugin
needs.

## What the port already leaves for it

- A08: `RouterEvent`, `Progress`, `RunSummary`, the `EventSink` and the
  two-level `CancelToken`. A job's progress endpoint is a sink; stop is the
  token.
- C08: `--progress json` (one JSON object per event) and `--result-json`
  (the manifest). A server returns the same objects.
- C02/C03: one settings field table with Java's names; the API's
  `ApiSettings` source is one more `SettingsSource` in the merge.
- D9 (A10): the stage registry, so an API can list and select algorithms.
- B01: snapshots and `state_hash`, for job outputs and change detection.

## Rules, if it starts

- The server is its own crate (`freerouting-server`, PORT_PLAN's name) and
  depends on `freerouting` only. The engine never learns about HTTP.
- No API keys, rate limiting or analytics are ported until Dan says what
  the server is for; a local single-user server needs none of them.
- `NEW DEP` lines for the HTTP stack, decided by the orchestrator.
