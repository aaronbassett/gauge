# Deploying gauge-server to Fly.io

One-time setup:

```bash
fly apps create gauge-telemetry
# Managed Postgres (verify current syntax with `fly mpg --help`):
fly mpg create --name gauge-pg --region lhr
fly mpg attach gauge-pg --app gauge-telemetry   # sets DATABASE_URL

# Secrets
fly secrets set --app gauge-telemetry \
  GAUGE_JWT_SECRET="$(openssl rand -base64 48)" \
  GAUGE_APP_ALLOWLIST="tome,midnight-manual" \
  GAUGE_USER_STORE="$(cat users.toml)"
```

`users.toml` (never committed; lives in your password manager):

```toml
schema_version = 1

[[users]]
user_id = "aaron"
role = "admin"
public_key = "ed25519:<output of `gauge keys generate --user-id aaron`>"
created_at = "2026-06-12"
```

Deploy + verify:

```bash
fly deploy
curl https://gauge-telemetry.fly.dev/healthz   # -> ok
curl https://gauge-telemetry.fly.dev/readyz    # -> ok
```

Rotating GAUGE_JWT_SECRET invalidates all issued tokens (1h TTL anyway).
Adding a reader = add a [[users]] row, re-run `fly secrets set GAUGE_USER_STORE=...`.
