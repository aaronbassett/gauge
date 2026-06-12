# The Gauge OTLP Profile (v1)

Gauge ingests telemetry as standard **OTLP/HTTP, logs signal, JSON encoding** at
`POST /v1/logs`. Any OTLP-conformant exporter can ship to it. On top of plain
OTLP, Gauge requires the profile below. Batches violating §1 are rejected whole
(HTTP 400); individual records violating §2–§3 are dropped via OTLP partial
success.

## 1. Required resource attributes (one resourceLogs block per request)

| Attribute | Meaning | Constraint |
|---|---|---|
| `service.name` | App id | Must be on the server allowlist |
| `service.version` | App version | Non-empty |
| `service.instance.id` | Anonymous install UUID | RFC-4122 v4, random per install, user-resettable |
| `session.id` | Per-process session UUID | RFC-4122 v4 |
| `os.type` | Platform | `darwin` \| `linux` \| `windows` |
| `host.arch` | CPU arch | `amd64` \| `arm64` |

## 2. Events

- One event per LogRecord. Event name in the `eventName` field (OTLP >= 1.4)
  or an `event.name` attribute; write **both** for compatibility.
- Names are app-namespaced: `<service.name>.<event>` (e.g. `tome.search`).
- `timeUnixNano` is required and non-zero.
- Attribute values are scalars only (string, bool, int, double).

## 3. Hygiene limits

≤ 30 attributes/record; string values ≤ 128 bytes; ≤ 1,000 records/batch;
body ≤ 1 MiB.

## 4. Privacy obligations (sender-side, mandatory)

Only bucketed counts and closed enum strings. Never: query text, file paths,
hostnames, usernames, emails, IPs, locale, raw error messages, or any
free-form string. Pin your event constructors to this spec with tests.
The server never stores IP/User-Agent and never echoes attribute values in
errors or logs.

## 5. Worked example

```json
{
  "resourceLogs": [
    {
      "resource": {
        "attributes": [
          { "key": "service.name", "value": { "stringValue": "tome" } },
          { "key": "service.version", "value": { "stringValue": "0.6.0" } },
          { "key": "service.instance.id", "value": { "stringValue": "0b9c1f2e-3a4d-4b6c-8e1f-2a3b4c5d6e7f" } },
          { "key": "session.id", "value": { "stringValue": "7f6e5d4c-3b2a-4f1e-9c8b-1a2b3c4d5e6f" } },
          { "key": "os.type", "value": { "stringValue": "darwin" } },
          { "key": "host.arch", "value": { "stringValue": "arm64" } }
        ]
      },
      "scopeLogs": [
        {
          "logRecords": [
            {
              "timeUnixNano": "1781430705123000000",
              "eventName": "tome.search",
              "attributes": [
                { "key": "event.name", "value": { "stringValue": "tome.search" } },
                { "key": "surface", "value": { "stringValue": "cli" } },
                { "key": "latency_bucket", "value": { "stringValue": "50-200ms" } },
                { "key": "reranker_used", "value": { "boolValue": true } },
                { "key": "candidates_returned", "value": { "intValue": "12" } }
              ]
            }
          ]
        }
      ]
    }
  ]
}
```
