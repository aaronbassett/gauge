# gauge-telemetry wire contract (v1)

`gauge-telemetry` emits the **Gauge OTLP profile** (see `gauge-events`'s
`SPEC.md`). On top of that profile this crate guarantees:

- Event names are app-namespaced: the bare `Event::name()` is prefixed with
  `<service.name>.` (e.g. `command_invoked` → `tome.command_invoked`).
- Attribute values are scalars only (string / bool / int / double); `None`
  fields are omitted; nested values and `null` (including non-finite floats) are
  rejected at emit.
- Quantities (`duration_ms`, `latency_ms`, `cpu_cores`, `ram_gb`, counts, rank)
  are sent as **raw integers** — bucketing happens at read time on the server.
- Environment attributes ride only on the low-frequency `install` / `heartbeat`
  events.

## Worked example

A `CommandInvoked { command: "search", duration_ms: 142, outcome: Ok,
surface: Cli }` for app `tome`, install `00000000-0000-4000-8000-000000000001`,
session `00000000-0000-4000-8000-000000000002`, at `timeUnixNano`
`1781430705123000000`, on `darwin`/`arm64`, encodes to the OTLP body pinned by
`tests/spec_pin.rs`.
