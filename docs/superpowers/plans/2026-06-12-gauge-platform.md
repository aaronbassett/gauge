# Gauge Telemetry Platform Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the gauge telemetry platform: an OTLP-ingesting axum server with Ed25519/JWT-authenticated query API on Fly.io + Postgres, a client binary that is both a ratatui dashboard and an MCP server, and a sender batching client for future app migrations.

**Architecture:** Single Cargo workspace, five crates. Shared crates (`gauge-auth`, `gauge-events`, `gauge-query`) define the auth protocol, the Gauge OTLP profile, and the query DSL; `gauge-server` persists events in one JSONB-attributed Postgres table and translates DSL queries into parameterized SQL; the `gauge` client consumes the same shared types for TUI, MCP, and CLI surfaces.

**Tech Stack:** Rust (edition 2024), axum 0.8, sqlx 0.8 (Postgres, runtime queries), tokio, ed25519-dalek 2, jsonwebtoken 9, serde/serde_json, schemars, ratatui + crossterm, rmcp (official MCP SDK), reqwest (rustls), insta + wiremock + sqlx::test for testing.

**Spec:** `docs/superpowers/specs/2026-06-12-gauge-telemetry-platform-design.md` (approved 2026-06-12).

---

## Phases and phase gates

| Phase | Delivers | Tasks |
|---|---|---|
| **1 — Server + foundations** | Workspace, `gauge-auth`, `gauge-events` (types/validation/SPEC.md), `gauge-query`, full `gauge-server`, Fly deployment | 1–21 |
| **2 — Client** | `gauge` binary: keys/login CLI, API layer, MCP server, TUI dashboard | 22–30 |
| **3 — Sender batching client** | `gauge_events::sender`: disk queue + OTLP encoder + crash-safe drain (used by future Tome/MNM migrations) | 31–34 |

**PHASE GATE PROTOCOL (mandatory):** At the end of each phase there is a "Phase Gate" section. Do not start the next phase until its steps are complete. Each gate requires: (a) full workspace test suite green; (b) re-read the remaining phases of THIS plan against what you learned (crate API drift, version changes, design friction) and **edit this plan document** to correct any task that no longer matches reality; (c) commit the plan revision with a message describing what changed and why. If nothing needs changing, commit a note in the plan's changelog below stating the gate passed unmodified.

### Plan changelog

- 2026-06-12: Initial plan written.
- 2026-06-12 (execution): Began autonomous subagent-driven execution. Drift log accumulates here; consolidated at each phase gate.
  - Task 1: pinned `time = "=0.3.47"` (workspace dep). `time 0.3.48` (edition 2024) adds a blanket impl that conflicts (E0119) with `sqlx-core 0.8.6`'s `impl<T> From<T> for Json<T>`. Pin keeps `sqlx 0.8` + `rust-version 1.93`. Revisit if sqlx 0.8.x patches or we move to sqlx 0.9 (needs Rust ≥1.94).

### Implementation progress (durable recovery ledger)

> One box per task/gate. Ticked when the task's PR is merged to `main`. This is the source of truth for resuming the loop after a crash.

- [x] Task 1 — Workspace scaffolding + CI (PR #1, merged)
- [ ] Task 2 — gauge-auth: error + wire format
- [ ] Task 3 — gauge-auth: keypair
- [ ] Task 4 — gauge-auth: user store
- [ ] Task 5 — gauge-auth: challenge store
- [ ] Task 6 — gauge-auth: JWT + protocol + client
- [ ] Task 7 — gauge-events: OTLP wire types
- [ ] Task 8 — gauge-events: Gauge profile validation
- [ ] Task 9 — gauge-events: SPEC.md + pin test
- [ ] Task 10 — gauge-query: query DSL types
- [ ] Task 11 — gauge-server: scaffold
- [ ] Task 12 — gauge-server: events migration + batch insert
- [ ] Task 13 — gauge-server: OTLP ingest endpoint
- [ ] Task 14 — gauge-server: auth endpoints
- [ ] Task 15 — gauge-server: bearer middleware
- [ ] Task 16 — gauge-server: query SQL builder
- [ ] Task 17 — gauge-server: POST /v1/query
- [ ] Task 18 — gauge-server: GET /v1/meta
- [ ] Task 19 — gauge-server: rate limiting
- [ ] Task 20 — gauge-server: privacy canary tests
- [ ] Task 21 — Deployment: Dockerfile, fly.toml, runbook
- [ ] PHASE GATE 1 → 2
- [ ] Task 22 — gauge: CLI scaffold, paths, config, error
- [ ] Task 23 — gauge: keys generate
- [ ] Task 24 — gauge: ApiClient (login, token cache, 401 retry)
- [ ] Task 25 — gauge: query one-shot command
- [ ] Task 26 — gauge: MCP tool query builders (pure)
- [ ] Task 27 — gauge: MCP server (rmcp, stdio)
- [ ] Task 28 — gauge: TUI data layer
- [ ] Task 29 — gauge: TUI app state + rendering
- [ ] Task 30 — gauge: TUI event loop + wiring
- [ ] PHASE GATE 2 → 3
- [ ] Task 31 — gauge-events: sender feature + disk queue
- [ ] Task 32 — gauge-events: sender config, enqueue, encoder
- [ ] Task 33 — gauge-events: sender transport + crash-safe drain
- [ ] PHASE GATE 3 — completion

---

## Environment prerequisites (read before Task 1)

- Rust toolchain ≥ 1.93 via rustup (`rust-toolchain.toml` pins it).
- A local Postgres 16 for tests. `sqlx::test` creates a throwaway database per test and needs `DATABASE_URL`:

```bash
docker run -d --name gauge-pg -e POSTGRES_PASSWORD=postgres -p 5432:5432 postgres:16
export DATABASE_URL=postgres://postgres:postgres@localhost:5432/postgres
```

All `cargo test` invocations for `gauge-server` assume `DATABASE_URL` is exported.

- **Version note:** dependency versions below are majors believed current at plan time. When `cargo check` reveals a newer major or an API drift (most likely: `rmcp`, `ratatui`), adapt the code at the call site, then record the drift at the next phase gate.

## File structure (final state)

```
gauge/
├── Cargo.toml                          # workspace root
├── rust-toolchain.toml
├── deny.toml
├── .gitignore
├── .github/workflows/ci.yml
├── migrations/0001_events.sql
├── Dockerfile.server
├── fly.toml
├── crates/
│   ├── gauge-auth/src/{lib,error,wire,keypair,user,challenge,jwt,protocol,client}.rs
│   ├── gauge-events/
│   │   ├── SPEC.md
│   │   ├── src/{lib,otlp,profile}.rs            # phase 1
│   │   ├── src/sender/{mod,queue,encode,transport,drain}.rs   # phase 3
│   │   └── tests/fixtures/{valid_batch.json,spec_anonymous_example.json}
│   ├── gauge-query/src/{lib,field,request,response,meta,validate}.rs
│   ├── gauge-server/src/{main,config,error,state,app,db,sqlbuild}.rs
│   ├── gauge-server/src/routes/{mod,health,ingest,auth,query,meta}.rs
│   ├── gauge-server/src/middleware/{mod,bearer,rate_limit}.rs
│   └── gauge/src/{main,paths,config,keys,login,api,query_cmd}.rs
│       └── src/mcp/{mod,tools}.rs  src/tui/{mod,app,data,ui}.rs
└── docs/...
```

---

# PHASE 1 — Server + foundations

### Task 1: Workspace scaffolding + CI

**Files:**
- Create: `Cargo.toml`, `rust-toolchain.toml`, `.gitignore`, `deny.toml`, `.github/workflows/ci.yml`
- Create: `crates/gauge-auth/Cargo.toml`, `crates/gauge-auth/src/lib.rs` (and the same stub pair for `gauge-events`, `gauge-query`, `gauge-server`, `gauge`)

- [ ] **Step 1: Root files**

`Cargo.toml`:

```toml
[workspace]
resolver = "3"
members = [
    "crates/gauge-auth",
    "crates/gauge-events",
    "crates/gauge-query",
    "crates/gauge-server",
    "crates/gauge",
]

[workspace.package]
version = "0.1.0"
edition = "2024"
license = "MIT OR Apache-2.0"
rust-version = "1.93"

[workspace.dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"
time = { version = "0.3", features = ["serde-human-readable", "formatting", "parsing", "macros"] }
uuid = { version = "1", features = ["v4", "serde"] }
base64 = "0.22"
schemars = "1"
ed25519-dalek = { version = "2", features = ["rand_core"] }
rand_core = { version = "0.6", features = ["getrandom"] }
jsonwebtoken = "9"
toml = "0.8"
axum = "0.8"
tokio = { version = "1", features = ["full"] }
tower = "0.5"
tower-http = { version = "0.6", features = ["trace", "request-id"] }
futures = "0.3"
sqlx = { version = "0.8", default-features = false, features = ["runtime-tokio", "tls-rustls", "postgres", "uuid", "time", "json", "migrate", "macros"] }
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "json"] }
reqwest = { version = "0.12", default-features = false, features = ["json", "rustls-tls"] }
clap = { version = "4", features = ["derive"] }
ratatui = "0.29"
crossterm = "0.28"
rmcp = { version = "0.6", features = ["server", "transport-io"] }
insta = { version = "1", features = ["json"] }
wiremock = "0.6"
tempfile = "3"
```

`rust-toolchain.toml`:

```toml
[toolchain]
channel = "1.93"
components = ["rustfmt", "clippy"]
```

`.gitignore`:

```
/target
*.private
.env
```

`deny.toml`:

```toml
[licenses]
allow = ["MIT", "Apache-2.0", "Apache-2.0 WITH LLVM-exception", "BSD-2-Clause", "BSD-3-Clause", "ISC", "Unicode-3.0", "Zlib", "MPL-2.0", "CDLA-Permissive-2.0", "OpenSSL"]

[advisories]
yanked = "deny"

[bans]
multiple-versions = "allow"
```

- [ ] **Step 2: Crate stubs**

For each of the five crates, create `crates/<name>/Cargo.toml` and an empty `src/lib.rs` (for `gauge-server` and `gauge`, `src/main.rs` with `fn main() {}` instead). Library crate manifests:

```toml
# crates/gauge-auth/Cargo.toml
[package]
name = "gauge-auth"
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true

[dependencies]
serde.workspace = true
thiserror.workspace = true
time.workspace = true
uuid.workspace = true
base64.workspace = true
ed25519-dalek.workspace = true
rand_core.workspace = true
jsonwebtoken.workspace = true
toml.workspace = true
```

```toml
# crates/gauge-events/Cargo.toml
[package]
name = "gauge-events"
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true

[dependencies]
serde.workspace = true
serde_json.workspace = true
thiserror.workspace = true
time.workspace = true
uuid.workspace = true
```

```toml
# crates/gauge-query/Cargo.toml
[package]
name = "gauge-query"
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true

[dependencies]
serde.workspace = true
serde_json.workspace = true
thiserror.workspace = true
time.workspace = true
schemars.workspace = true
```

```toml
# crates/gauge-server/Cargo.toml
[package]
name = "gauge-server"
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true

[dependencies]
gauge-auth = { path = "../gauge-auth" }
gauge-events = { path = "../gauge-events" }
gauge-query = { path = "../gauge-query" }
axum.workspace = true
tokio.workspace = true
tower.workspace = true
tower-http.workspace = true
sqlx.workspace = true
serde.workspace = true
serde_json.workspace = true
thiserror.workspace = true
time.workspace = true
uuid.workspace = true
base64.workspace = true
tracing.workspace = true
tracing-subscriber.workspace = true

[dev-dependencies]
insta.workspace = true
```

```toml
# crates/gauge/Cargo.toml
[package]
name = "gauge"
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true

[dependencies]
gauge-auth = { path = "../gauge-auth" }
gauge-query = { path = "../gauge-query" }
clap.workspace = true
tokio.workspace = true
reqwest.workspace = true
serde.workspace = true
serde_json.workspace = true
thiserror.workspace = true
time.workspace = true
base64.workspace = true
toml.workspace = true
schemars.workspace = true

[dev-dependencies]
wiremock.workspace = true
tempfile.workspace = true
```

(ratatui/crossterm/rmcp are added to `gauge` in Phase 2 when first used; `gauge-server`'s `main.rs` stays `fn main() {}` until Task 11.)

- [ ] **Step 3: CI workflow**

`.github/workflows/ci.yml`:

```yaml
name: CI
on:
  push:
    branches: [main]
  pull_request:

jobs:
  lint:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - run: rustup show
      - run: cargo fmt --all --check
      - run: cargo clippy --workspace --all-targets -- -D warnings

  test:
    runs-on: ubuntu-latest
    services:
      postgres:
        image: postgres:16
        env:
          POSTGRES_PASSWORD: postgres
        ports: ["5432:5432"]
        options: >-
          --health-cmd pg_isready --health-interval 5s
          --health-timeout 5s --health-retries 10
    env:
      DATABASE_URL: postgres://postgres:postgres@localhost:5432/postgres
    steps:
      - uses: actions/checkout@v4
      - run: rustup show
      - run: cargo test --workspace

  deny:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: EmbarkStudios/cargo-deny-action@v2
```

- [ ] **Step 4: Verify**

Run: `cargo check --workspace`
Expected: compiles with no errors (warnings about empty crates are fine).

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "chore: scaffold gauge workspace, crates, and CI"
```

---

### Task 2: gauge-auth — error type + public key wire format

**Files:**
- Create: `crates/gauge-auth/src/error.rs`, `crates/gauge-auth/src/wire.rs`
- Modify: `crates/gauge-auth/src/lib.rs`

- [ ] **Step 1: Write the failing tests**

Append to `crates/gauge-auth/src/wire.rs` (tests module included with implementation file; write tests first):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    // A valid 32-byte ed25519 public key, base64 (this is 32 zero bytes — valid curve point not required for parse-length tests, but VerifyingKey::from_bytes rejects invalid points, so use a real key in round-trip tests below).
    #[test]
    fn parse_rejects_missing_prefix() {
        assert!(matches!(parse_public_key_wire("AAAA"), Err(AuthError::InvalidWireFormat)));
    }

    #[test]
    fn parse_rejects_wrong_length() {
        assert!(matches!(parse_public_key_wire("ed25519:AAAA"), Err(AuthError::InvalidLength)));
    }

    #[test]
    fn parse_rejects_bad_base64() {
        assert!(matches!(parse_public_key_wire("ed25519:!!!not-base64!!!"), Err(AuthError::Base64)));
    }

    #[test]
    fn flexible_b64_accepts_padded_and_unpadded() {
        assert_eq!(b64_decode_flexible("aGk=").unwrap(), b"hi");
        assert_eq!(b64_decode_flexible("aGk").unwrap(), b"hi");
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p gauge-auth`
Expected: FAIL — `parse_public_key_wire` / `b64_decode_flexible` not found.

- [ ] **Step 3: Implement**

`crates/gauge-auth/src/error.rs`:

```rust
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AuthError {
    #[error("invalid public key wire format (expected `ed25519:<base64>`)")]
    InvalidWireFormat,
    #[error("invalid key, nonce, or signature length")]
    InvalidLength,
    #[error("signature verification failed")]
    InvalidSignature,
    #[error("challenge not found or already used")]
    ChallengeNotFound,
    #[error("challenge expired")]
    ChallengeExpired,
    #[error("JWT secret must be at least 32 bytes")]
    SecretTooShort,
    #[error("jwt error: {0}")]
    Jwt(String),
    #[error("user store error: {0}")]
    UserStore(String),
    #[error("base64 decode error")]
    Base64,
}
```

`crates/gauge-auth/src/wire.rs` (above the tests module):

```rust
use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD};
use ed25519_dalek::VerifyingKey;

use crate::error::AuthError;

pub const ED25519_WIRE_PREFIX: &str = "ed25519:";
pub const PUBLIC_KEY_LEN: usize = 32;
pub const SIGNATURE_LEN: usize = 64;
pub const NONCE_LEN: usize = 32;

/// Accepts both padded and unpadded standard base64 (mn-auth compatibility).
pub fn b64_decode_flexible(s: &str) -> Result<Vec<u8>, AuthError> {
    STANDARD
        .decode(s)
        .or_else(|_| STANDARD_NO_PAD.decode(s))
        .map_err(|_| AuthError::Base64)
}

pub fn parse_public_key_wire(wire: &str) -> Result<VerifyingKey, AuthError> {
    let b64 = wire
        .strip_prefix(ED25519_WIRE_PREFIX)
        .ok_or(AuthError::InvalidWireFormat)?;
    let bytes = b64_decode_flexible(b64)?;
    let arr: [u8; PUBLIC_KEY_LEN] = bytes.try_into().map_err(|_| AuthError::InvalidLength)?;
    VerifyingKey::from_bytes(&arr).map_err(|_| AuthError::InvalidWireFormat)
}

pub fn format_public_key_wire(key: &VerifyingKey) -> String {
    format!("{ED25519_WIRE_PREFIX}{}", STANDARD_NO_PAD.encode(key.as_bytes()))
}
```

`crates/gauge-auth/src/lib.rs`:

```rust
pub mod error;
pub mod wire;

pub use error::AuthError;
```

- [ ] **Step 4: Run to verify pass**

Run: `cargo test -p gauge-auth`
Expected: PASS (4 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/gauge-auth
git commit -m "feat(auth): error type and ed25519 public key wire format"
```

---

### Task 3: gauge-auth — keypair generate/sign/verify

**Files:**
- Create: `crates/gauge-auth/src/keypair.rs`
- Modify: `crates/gauge-auth/src/lib.rs`

- [ ] **Step 1: Write the failing tests**

`crates/gauge-auth/src/keypair.rs` tests module:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::parse_public_key_wire;

    #[test]
    fn sign_verify_round_trip() {
        let kp = Keypair::generate();
        let sig = kp.sign(b"nonce-bytes");
        assert!(verify_signature(&kp.verifying_key(), b"nonce-bytes", &sig).is_ok());
    }

    #[test]
    fn tampered_message_fails() {
        let kp = Keypair::generate();
        let sig = kp.sign(b"nonce-bytes");
        assert!(matches!(
            verify_signature(&kp.verifying_key(), b"other-bytes", &sig),
            Err(AuthError::InvalidSignature)
        ));
    }

    #[test]
    fn wrong_key_fails() {
        let kp = Keypair::generate();
        let other = Keypair::generate();
        let sig = kp.sign(b"nonce-bytes");
        assert!(verify_signature(&other.verifying_key(), b"nonce-bytes", &sig).is_err());
    }

    #[test]
    fn public_wire_round_trips_through_parser() {
        let kp = Keypair::generate();
        let parsed = parse_public_key_wire(&kp.public_wire()).unwrap();
        assert_eq!(parsed.as_bytes(), kp.verifying_key().as_bytes());
    }

    #[test]
    fn seed_round_trip() {
        let kp = Keypair::generate();
        let restored = Keypair::from_seed(&kp.seed());
        assert_eq!(restored.public_wire(), kp.public_wire());
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p gauge-auth keypair`
Expected: FAIL — `Keypair` not found.

- [ ] **Step 3: Implement**

`crates/gauge-auth/src/keypair.rs` (above tests):

```rust
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand_core::OsRng;

use crate::error::AuthError;
use crate::wire::{SIGNATURE_LEN, format_public_key_wire};

/// Holds the private signing key in memory. Never logged: no Debug impl.
pub struct Keypair(SigningKey);

impl Keypair {
    pub fn generate() -> Self {
        Self(SigningKey::generate(&mut OsRng))
    }

    pub fn from_seed(seed: &[u8; 32]) -> Self {
        Self(SigningKey::from_bytes(seed))
    }

    pub fn seed(&self) -> [u8; 32] {
        self.0.to_bytes()
    }

    pub fn verifying_key(&self) -> VerifyingKey {
        self.0.verifying_key()
    }

    pub fn public_wire(&self) -> String {
        format_public_key_wire(&self.0.verifying_key())
    }

    pub fn sign(&self, msg: &[u8]) -> [u8; SIGNATURE_LEN] {
        self.0.sign(msg).to_bytes()
    }
}

pub fn verify_signature(key: &VerifyingKey, msg: &[u8], sig: &[u8]) -> Result<(), AuthError> {
    let sig: [u8; SIGNATURE_LEN] = sig.try_into().map_err(|_| AuthError::InvalidLength)?;
    let sig = Signature::from_bytes(&sig);
    key.verify(msg, &sig).map_err(|_| AuthError::InvalidSignature)
}
```

Add `pub mod keypair;` and `pub use keypair::{Keypair, verify_signature};` to `lib.rs`.

- [ ] **Step 4: Run to verify pass**

Run: `cargo test -p gauge-auth`
Expected: PASS (9 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/gauge-auth
git commit -m "feat(auth): ed25519 keypair generation, signing, verification"
```

---

### Task 4: gauge-auth — user store (users.toml)

**Files:**
- Create: `crates/gauge-auth/src/user.rs`
- Modify: `crates/gauge-auth/src/lib.rs`

- [ ] **Step 1: Write the failing tests**

`crates/gauge-auth/src/user.rs` tests module:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::keypair::Keypair;

    fn toml_for(user_id: &str, key_wire: &str) -> String {
        format!(
            r#"
schema_version = 1

[[users]]
user_id = "{user_id}"
role = "admin"
public_key = "{key_wire}"
created_at = "2026-06-12"
note = "test user"
"#
        )
    }

    #[test]
    fn loads_valid_store() {
        let kp = Keypair::generate();
        let store = UserStore::from_toml_str(&toml_for("alice", &kp.public_wire())).unwrap();
        let user = store.get("alice").unwrap();
        assert_eq!(user.role, Role::Admin);
        assert_eq!(user.public_key, kp.public_wire());
        assert!(store.get("bob").is_none());
    }

    #[test]
    fn rejects_wrong_schema_version() {
        let err = UserStore::from_toml_str("schema_version = 2").unwrap_err();
        assert!(matches!(err, AuthError::UserStore(_)));
    }

    #[test]
    fn rejects_duplicate_user_id() {
        let kp = Keypair::generate();
        let one = toml_for("alice", &kp.public_wire());
        let dup = format!("{one}\n[[users]]\nuser_id = \"alice\"\nrole = \"viewer\"\npublic_key = \"{}\"\n", kp.public_wire());
        assert!(UserStore::from_toml_str(&dup).is_err());
    }

    #[test]
    fn rejects_unparseable_public_key() {
        let bad = toml_for("alice", "ed25519:not!!base64");
        assert!(UserStore::from_toml_str(&bad).is_err());
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p gauge-auth user`
Expected: FAIL — `UserStore` not found.

- [ ] **Step 3: Implement**

`crates/gauge-auth/src/user.rs` (above tests):

```rust
use std::collections::HashMap;

use crate::error::AuthError;
use crate::wire::parse_public_key_wire;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Admin,
    Viewer,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct User {
    pub user_id: String,
    pub role: Role,
    pub public_key: String,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct UserFile {
    schema_version: u32,
    #[serde(default)]
    users: Vec<User>,
}

pub struct UserStore {
    users: HashMap<String, User>,
}

impl UserStore {
    pub fn from_toml_str(s: &str) -> Result<Self, AuthError> {
        let file: UserFile =
            toml::from_str(s).map_err(|e| AuthError::UserStore(e.to_string()))?;
        if file.schema_version != 1 {
            return Err(AuthError::UserStore(format!(
                "unsupported schema_version {}",
                file.schema_version
            )));
        }
        let mut users = HashMap::new();
        for u in file.users {
            parse_public_key_wire(&u.public_key)
                .map_err(|e| AuthError::UserStore(format!("user `{}`: {e}", u.user_id)))?;
            if users.insert(u.user_id.clone(), u).is_some() {
                return Err(AuthError::UserStore("duplicate user_id".into()));
            }
        }
        Ok(Self { users })
    }

    pub fn get(&self, user_id: &str) -> Option<&User> {
        self.users.get(user_id)
    }

    pub fn len(&self) -> usize {
        self.users.len()
    }

    pub fn is_empty(&self) -> bool {
        self.users.is_empty()
    }
}
```

Add `pub mod user;` and `pub use user::{Role, User, UserStore};` to `lib.rs`.

- [ ] **Step 4: Run to verify pass**

Run: `cargo test -p gauge-auth`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/gauge-auth
git commit -m "feat(auth): TOML user store with key validation at load"
```

---

### Task 5: gauge-auth — single-use challenge store

**Files:**
- Create: `crates/gauge-auth/src/challenge.rs`
- Modify: `crates/gauge-auth/src/lib.rs`

- [ ] **Step 1: Write the failing tests**

`crates/gauge-auth/src/challenge.rs` tests module:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;

    const T0: OffsetDateTime = datetime!(2026-06-12 10:00:00 UTC);

    #[test]
    fn mint_then_consume_within_ttl() {
        let store = ChallengeStore::new();
        let c = store.mint("alice", T0);
        assert_eq!(c.user_id, "alice");
        assert_eq!(c.expires_at, T0 + CHALLENGE_TTL);
        let consumed = store.consume(&c.challenge_id, T0 + time::Duration::seconds(30)).unwrap();
        assert_eq!(consumed.nonce, c.nonce);
    }

    #[test]
    fn consume_is_single_use() {
        let store = ChallengeStore::new();
        let c = store.mint("alice", T0);
        store.consume(&c.challenge_id, T0).unwrap();
        assert!(matches!(
            store.consume(&c.challenge_id, T0),
            Err(AuthError::ChallengeNotFound)
        ));
    }

    #[test]
    fn consume_after_expiry_fails_and_removes() {
        let store = ChallengeStore::new();
        let c = store.mint("alice", T0);
        let late = T0 + CHALLENGE_TTL + time::Duration::seconds(1);
        assert!(matches!(store.consume(&c.challenge_id, late), Err(AuthError::ChallengeExpired)));
        assert!(matches!(store.consume(&c.challenge_id, T0), Err(AuthError::ChallengeNotFound)));
    }

    #[test]
    fn purge_removes_only_expired() {
        let store = ChallengeStore::new();
        let old = store.mint("alice", T0 - time::Duration::minutes(5));
        let fresh = store.mint("bob", T0);
        store.purge_expired(T0);
        assert!(store.consume(&old.challenge_id, T0).is_err());
        assert!(store.consume(&fresh.challenge_id, T0).is_ok());
    }

    #[test]
    fn nonces_are_unique() {
        let store = ChallengeStore::new();
        assert_ne!(store.mint("a", T0).nonce, store.mint("a", T0).nonce);
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p gauge-auth challenge`
Expected: FAIL — `ChallengeStore` not found.

- [ ] **Step 3: Implement**

`crates/gauge-auth/src/challenge.rs` (above tests):

```rust
use std::collections::HashMap;
use std::sync::Mutex;

use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use crate::error::AuthError;
use crate::wire::NONCE_LEN;

/// FR from spec: challenge TTL is clamped to 60 seconds.
pub const CHALLENGE_TTL: Duration = Duration::seconds(60);

#[derive(Debug, Clone)]
pub struct Challenge {
    pub challenge_id: Uuid,
    pub user_id: String,
    pub nonce: [u8; NONCE_LEN],
    pub expires_at: OffsetDateTime,
}

#[derive(Default)]
pub struct ChallengeStore {
    inner: Mutex<HashMap<Uuid, Challenge>>,
}

impl ChallengeStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn mint(&self, user_id: &str, now: OffsetDateTime) -> Challenge {
        let mut nonce = [0u8; NONCE_LEN];
        rand_core::RngCore::fill_bytes(&mut rand_core::OsRng, &mut nonce);
        let c = Challenge {
            challenge_id: Uuid::new_v4(),
            user_id: user_id.to_string(),
            nonce,
            expires_at: now + CHALLENGE_TTL,
        };
        self.inner.lock().unwrap().insert(c.challenge_id, c.clone());
        c
    }

    /// Removes the challenge regardless of outcome: expired consume attempts
    /// also burn the challenge (single-use either way).
    pub fn consume(&self, id: &Uuid, now: OffsetDateTime) -> Result<Challenge, AuthError> {
        let c = self
            .inner
            .lock()
            .unwrap()
            .remove(id)
            .ok_or(AuthError::ChallengeNotFound)?;
        if now > c.expires_at {
            return Err(AuthError::ChallengeExpired);
        }
        Ok(c)
    }

    pub fn purge_expired(&self, now: OffsetDateTime) {
        self.inner.lock().unwrap().retain(|_, c| c.expires_at >= now);
    }
}
```

Add `pub mod challenge;` and `pub use challenge::{CHALLENGE_TTL, Challenge, ChallengeStore};` to `lib.rs`.

- [ ] **Step 4: Run to verify pass**

Run: `cargo test -p gauge-auth`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/gauge-auth
git commit -m "feat(auth): in-memory single-use challenge store with 60s TTL"
```

---

### Task 6: gauge-auth — JWT mint/verify + protocol DTOs + client helper

**Files:**
- Create: `crates/gauge-auth/src/jwt.rs`, `crates/gauge-auth/src/protocol.rs`, `crates/gauge-auth/src/client.rs`
- Modify: `crates/gauge-auth/src/lib.rs`

- [ ] **Step 1: Write the failing tests**

`crates/gauge-auth/src/jwt.rs` tests module:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::user::Role;
    use time::macros::datetime;

    fn secret() -> SigningSecret {
        SigningSecret::new(vec![7u8; 32]).unwrap()
    }

    #[test]
    fn rejects_short_secret() {
        assert!(matches!(SigningSecret::new(vec![7u8; 31]), Err(AuthError::SecretTooShort)));
    }

    #[test]
    fn mint_verify_round_trip() {
        let now = OffsetDateTime::now_utc();
        let (token, exp) = mint_token(&secret(), "alice", Role::Admin, now).unwrap();
        assert_eq!(exp, now.unix_timestamp() + TOKEN_TTL_SECS);
        let claims = verify_token(&secret(), &token).unwrap();
        assert_eq!(claims.sub, "alice");
        assert_eq!(claims.role, Role::Admin);
        assert!(!claims.jti.is_empty());
    }

    #[test]
    fn tampered_token_fails() {
        let (token, _) = mint_token(&secret(), "alice", Role::Admin, OffsetDateTime::now_utc()).unwrap();
        let mut tampered = token.clone();
        tampered.push('x');
        assert!(verify_token(&secret(), &tampered).is_err());
    }

    #[test]
    fn wrong_secret_fails() {
        let (token, _) = mint_token(&secret(), "alice", Role::Admin, OffsetDateTime::now_utc()).unwrap();
        let other = SigningSecret::new(vec![9u8; 32]).unwrap();
        assert!(verify_token(&other, &token).is_err());
    }

    #[test]
    fn expired_token_fails() {
        let past = datetime!(2020-01-01 00:00:00 UTC);
        let (token, _) = mint_token(&secret(), "alice", Role::Viewer, past).unwrap();
        assert!(verify_token(&secret(), &token).is_err());
    }

    #[test]
    fn secret_debug_is_redacted() {
        assert_eq!(format!("{:?}", secret()), "SigningSecret(redacted)");
    }
}
```

`crates/gauge-auth/src/client.rs` tests module:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::keypair::{Keypair, verify_signature};
    use base64::Engine as _;

    #[test]
    fn sign_challenge_produces_verifiable_signature() {
        let kp = Keypair::generate();
        let nonce = [42u8; 32];
        let nonce_b64 = base64::engine::general_purpose::STANDARD_NO_PAD.encode(nonce);
        let sig_b64 = sign_challenge(&kp, &nonce_b64).unwrap();
        let sig = crate::wire::b64_decode_flexible(&sig_b64).unwrap();
        assert!(verify_signature(&kp.verifying_key(), &nonce, &sig).is_ok());
    }

    #[test]
    fn sign_challenge_rejects_wrong_nonce_length() {
        let kp = Keypair::generate();
        let short = base64::engine::general_purpose::STANDARD_NO_PAD.encode([1u8; 8]);
        assert!(matches!(sign_challenge(&kp, &short), Err(AuthError::InvalidLength)));
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p gauge-auth jwt client`
Expected: FAIL — modules not found.

- [ ] **Step 3: Implement**

`crates/gauge-auth/src/jwt.rs` (above tests):

```rust
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, encode};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::error::AuthError;
use crate::user::Role;

pub const TOKEN_TTL_SECS: i64 = 3600;

/// Opaque wrapper: prevents accidental Debug/log leak of the HS256 secret.
pub struct SigningSecret(Vec<u8>);

impl SigningSecret {
    pub fn new(bytes: impl Into<Vec<u8>>) -> Result<Self, AuthError> {
        let b = bytes.into();
        if b.len() < 32 {
            return Err(AuthError::SecretTooShort);
        }
        Ok(Self(b))
    }
}

impl std::fmt::Debug for SigningSecret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SigningSecret(redacted)")
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Claims {
    pub sub: String,
    pub iat: i64,
    pub exp: i64,
    pub role: Role,
    pub jti: String,
}

/// Returns (token, exp_unix_seconds).
pub fn mint_token(
    secret: &SigningSecret,
    user_id: &str,
    role: Role,
    now: OffsetDateTime,
) -> Result<(String, i64), AuthError> {
    let exp = now.unix_timestamp() + TOKEN_TTL_SECS;
    let claims = Claims {
        sub: user_id.to_string(),
        iat: now.unix_timestamp(),
        exp,
        role,
        jti: Uuid::new_v4().to_string(),
    };
    let token = encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(&secret.0),
    )
    .map_err(|e| AuthError::Jwt(e.to_string()))?;
    Ok((token, exp))
}

pub fn verify_token(secret: &SigningSecret, token: &str) -> Result<Claims, AuthError> {
    let mut validation = Validation::new(Algorithm::HS256);
    validation.leeway = 0;
    decode::<Claims>(token, &DecodingKey::from_secret(&secret.0), &validation)
        .map(|d| d.claims)
        .map_err(|e| AuthError::Jwt(e.to_string()))
}
```

`crates/gauge-auth/src/protocol.rs` (shared request/response DTOs for the auth endpoints — used by server routes and client login flow):

```rust
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ChallengeRequest {
    pub user_id: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ChallengeResponse {
    pub challenge_id: uuid::Uuid,
    pub nonce_b64: String,
    pub expires_in_s: u32,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct VerifyRequest {
    pub challenge_id: uuid::Uuid,
    pub signature_b64: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct VerifyResponse {
    pub token: String,
    pub user_id: String,
    /// Unix seconds.
    pub expires_at: i64,
}
```

`crates/gauge-auth/src/client.rs` (above tests):

```rust
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD_NO_PAD;

use crate::error::AuthError;
use crate::keypair::Keypair;
use crate::wire::{NONCE_LEN, b64_decode_flexible};

/// Client half of the challenge/response: decode the server's nonce, sign it,
/// return the base64 signature for POST /v1/auth/verify.
pub fn sign_challenge(keypair: &Keypair, nonce_b64: &str) -> Result<String, AuthError> {
    let nonce = b64_decode_flexible(nonce_b64)?;
    if nonce.len() != NONCE_LEN {
        return Err(AuthError::InvalidLength);
    }
    Ok(STANDARD_NO_PAD.encode(keypair.sign(&nonce)))
}
```

`lib.rs` final form for phase 1:

```rust
pub mod challenge;
pub mod client;
pub mod error;
pub mod jwt;
pub mod keypair;
pub mod protocol;
pub mod user;
pub mod wire;

pub use challenge::{CHALLENGE_TTL, Challenge, ChallengeStore};
pub use client::sign_challenge;
pub use error::AuthError;
pub use jwt::{Claims, SigningSecret, TOKEN_TTL_SECS, mint_token, verify_token};
pub use keypair::{Keypair, verify_signature};
pub use user::{Role, User, UserStore};
```

- [ ] **Step 4: Run to verify pass**

Run: `cargo test -p gauge-auth`
Expected: PASS (all gauge-auth tests).

- [ ] **Step 5: Commit**

```bash
git add crates/gauge-auth
git commit -m "feat(auth): HS256 JWT mint/verify, protocol DTOs, client sign helper"
```

---

### Task 7: gauge-events — OTLP wire types

**Files:**
- Create: `crates/gauge-events/src/otlp.rs`, `crates/gauge-events/tests/fixtures/valid_batch.json`, `crates/gauge-events/tests/otlp_roundtrip.rs`
- Modify: `crates/gauge-events/src/lib.rs`

- [ ] **Step 1: Write the fixture and failing test**

`crates/gauge-events/tests/fixtures/valid_batch.json` (a canonical Gauge-profile OTLP request — also reused by server tests; note 64-bit ints are JSON strings per the protobuf JSON mapping):

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

`crates/gauge-events/tests/otlp_roundtrip.rs`:

```rust
use gauge_events::otlp::ExportLogsServiceRequest;

const FIXTURE: &str = include_str!("fixtures/valid_batch.json");

#[test]
fn fixture_parses() {
    let req: ExportLogsServiceRequest = serde_json::from_str(FIXTURE).unwrap();
    let rl = &req.resource_logs[0];
    assert_eq!(rl.resource.as_ref().unwrap().attributes.len(), 6);
    let rec = &rl.scope_logs[0].log_records[0];
    assert_eq!(rec.event_name.as_deref(), Some("tome.search"));
    assert_eq!(rec.time_unix_nano, Some(1_781_430_705_123_000_000));
    assert_eq!(rec.attributes.len(), 5);
}

#[test]
fn serialization_round_trips() {
    let req: ExportLogsServiceRequest = serde_json::from_str(FIXTURE).unwrap();
    let json = serde_json::to_string(&req).unwrap();
    let back: ExportLogsServiceRequest = serde_json::from_str(&json).unwrap();
    // timeUnixNano must serialize back to a string (protobuf JSON int64 rule)
    assert!(json.contains("\"timeUnixNano\":\"1781430705123000000\""));
    assert_eq!(back.resource_logs[0].scope_logs[0].log_records[0].time_unix_nano,
               Some(1_781_430_705_123_000_000));
}

#[test]
fn time_unix_nano_accepts_json_number_too() {
    let req: ExportLogsServiceRequest =
        serde_json::from_str(r#"{"resourceLogs":[{"scopeLogs":[{"logRecords":[{"timeUnixNano":123}]}]}]}"#).unwrap();
    assert_eq!(req.resource_logs[0].scope_logs[0].log_records[0].time_unix_nano, Some(123));
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p gauge-events`
Expected: FAIL — module `otlp` not found.

- [ ] **Step 3: Implement**

`crates/gauge-events/src/otlp.rs`:

```rust
//! Serde types for the subset of the OTLP/HTTP logs-signal JSON encoding that
//! the Gauge profile uses. Field names follow the protobuf JSON mapping
//! (camelCase; 64-bit integers encoded as decimal strings).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportLogsServiceRequest {
    #[serde(default)]
    pub resource_logs: Vec<ResourceLogs>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceLogs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource: Option<Resource>,
    #[serde(default)]
    pub scope_logs: Vec<ScopeLogs>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Resource {
    #[serde(default)]
    pub attributes: Vec<KeyValue>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScopeLogs {
    #[serde(default)]
    pub log_records: Vec<LogRecord>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogRecord {
    #[serde(default, skip_serializing_if = "Option::is_none", with = "u64_string_opt")]
    pub time_unix_nano: Option<u64>,
    /// OTLP >= 1.4 LogRecord event name field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_name: Option<String>,
    #[serde(default)]
    pub attributes: Vec<KeyValue>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct KeyValue {
    pub key: String,
    #[serde(default)]
    pub value: AnyValue,
}

/// Protobuf JSON encodes AnyValue as a single-variant object,
/// e.g. {"stringValue": "x"} or {"intValue": "42"}.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnyValue {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub string_value: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bool_value: Option<bool>,
    /// int64 as decimal string per the protobuf JSON mapping.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub int_value: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub double_value: Option<f64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportLogsServiceResponse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub partial_success: Option<ExportLogsPartialSuccess>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportLogsPartialSuccess {
    pub rejected_log_records: i64,
    pub error_message: String,
}

/// u64 that serializes as a string but tolerantly deserializes from string or number.
mod u64_string_opt {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(v: &Option<u64>, s: S) -> Result<S::Ok, S::Error> {
        match v {
            Some(n) => s.serialize_str(&n.to_string()),
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<u64>, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            N(u64),
            S(String),
        }
        Ok(match Option::<Raw>::deserialize(d)? {
            None => None,
            Some(Raw::N(n)) => Some(n),
            Some(Raw::S(s)) => Some(s.parse().map_err(serde::de::Error::custom)?),
        })
    }
}
```

`crates/gauge-events/src/lib.rs`:

```rust
pub mod otlp;
```

- [ ] **Step 4: Run to verify pass**

Run: `cargo test -p gauge-events`
Expected: PASS (3 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/gauge-events
git commit -m "feat(events): OTLP logs-signal wire types with protobuf JSON semantics"
```

---

### Task 8: gauge-events — Gauge profile validation

**Files:**
- Create: `crates/gauge-events/src/profile.rs`
- Modify: `crates/gauge-events/src/lib.rs`

- [ ] **Step 1: Write the failing tests**

`crates/gauge-events/tests/profile_validation.rs`:

```rust
use gauge_events::otlp::ExportLogsServiceRequest;
use gauge_events::profile::{BatchError, validate_batch};

const FIXTURE: &str = include_str!("fixtures/valid_batch.json");

fn fixture() -> ExportLogsServiceRequest {
    serde_json::from_str(FIXTURE).unwrap()
}

fn allow() -> Vec<String> {
    vec!["tome".to_string(), "midnight-manual".to_string()]
}

#[test]
fn valid_batch_passes() {
    let batch = validate_batch(&fixture(), &allow()).unwrap();
    assert_eq!(batch.resource.app, "tome");
    assert_eq!(batch.resource.os, "darwin");
    assert_eq!(batch.events.len(), 1);
    assert!(batch.rejections.is_empty());
    let ev = &batch.events[0];
    assert_eq!(ev.event_name, "tome.search");
    // event.name attribute is stripped from stored attributes
    assert!(!ev.attributes.contains_key("event.name"));
    assert_eq!(ev.attributes["surface"], serde_json::json!("cli"));
    assert_eq!(ev.attributes["reranker_used"], serde_json::json!(true));
    assert_eq!(ev.attributes["candidates_returned"], serde_json::json!(12));
}

#[test]
fn unknown_app_is_batch_error() {
    let err = validate_batch(&fixture(), &["other".to_string()]).unwrap_err();
    assert!(matches!(err, BatchError::UnknownApp(a) if a == "tome"));
}

#[test]
fn missing_resource_attr_is_batch_error() {
    let mut req = fixture();
    req.resource_logs[0]
        .resource
        .as_mut()
        .unwrap()
        .attributes
        .retain(|kv| kv.key != "service.instance.id");
    let err = validate_batch(&req, &allow()).unwrap_err();
    assert!(matches!(err, BatchError::BadResourceAttr("service.instance.id")));
}

#[test]
fn bad_os_type_is_batch_error() {
    let mut req = fixture();
    for kv in &mut req.resource_logs[0].resource.as_mut().unwrap().attributes {
        if kv.key == "os.type" {
            kv.value.string_value = Some("macos".into()); // profile requires "darwin"
        }
    }
    assert!(matches!(validate_batch(&req, &allow()), Err(BatchError::BadResourceAttr("os.type"))));
}

#[test]
fn multiple_resource_blocks_rejected() {
    let mut req = fixture();
    let dup = req.resource_logs[0].clone();
    req.resource_logs.push(dup);
    assert!(matches!(validate_batch(&req, &allow()), Err(BatchError::ExpectedSingleResource)));
}

#[test]
fn record_missing_event_name_is_rejected_not_fatal() {
    let mut req = fixture();
    let mut bad = req.resource_logs[0].scope_logs[0].log_records[0].clone();
    bad.event_name = None;
    bad.attributes.retain(|kv| kv.key != "event.name");
    req.resource_logs[0].scope_logs[0].log_records.push(bad);
    let batch = validate_batch(&req, &allow()).unwrap();
    assert_eq!(batch.events.len(), 1);
    assert_eq!(batch.rejections.len(), 1);
    assert_eq!(batch.rejections[0].index, 1);
    assert!(batch.rejections[0].reason.contains("event name"));
}

#[test]
fn event_name_falls_back_to_attribute() {
    let mut req = fixture();
    req.resource_logs[0].scope_logs[0].log_records[0].event_name = None;
    let batch = validate_batch(&req, &allow()).unwrap();
    assert_eq!(batch.events[0].event_name, "tome.search");
}

#[test]
fn wrong_prefix_rejected() {
    let mut req = fixture();
    req.resource_logs[0].scope_logs[0].log_records[0].event_name = Some("other.search".into());
    let batch = validate_batch(&req, &allow()).unwrap();
    assert_eq!(batch.rejections.len(), 1);
    assert!(batch.rejections[0].reason.contains("prefixed"));
}

#[test]
fn oversized_attribute_string_rejected() {
    let mut req = fixture();
    req.resource_logs[0].scope_logs[0].log_records[0]
        .attributes
        .push(gauge_events::otlp::KeyValue {
            key: "big".into(),
            value: gauge_events::otlp::AnyValue {
                string_value: Some("x".repeat(129)),
                ..Default::default()
            },
        });
    let batch = validate_batch(&req, &allow()).unwrap();
    assert_eq!(batch.rejections.len(), 1);
    assert!(batch.rejections[0].reason.contains("128"));
}

#[test]
fn too_many_records_is_batch_error() {
    let mut req = fixture();
    let rec = req.resource_logs[0].scope_logs[0].log_records[0].clone();
    req.resource_logs[0].scope_logs[0].log_records = vec![rec; 1001];
    assert!(matches!(validate_batch(&req, &allow()), Err(BatchError::TooManyRecords)));
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p gauge-events profile`
Expected: FAIL — module `profile` not found.

- [ ] **Step 3: Implement**

`crates/gauge-events/src/profile.rs`:

```rust
//! Gauge OTLP profile validation: required resource attributes, event naming,
//! and hygiene limits. Shared by the server (ingest) and senders (pre-flight).

use serde_json::{Map, Value};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::otlp::{ExportLogsServiceRequest, KeyValue, LogRecord};

pub const MAX_ATTRIBUTES_PER_RECORD: usize = 30;
pub const MAX_ATTR_STRING_BYTES: usize = 128;
pub const MAX_RECORDS_PER_BATCH: usize = 1000;
pub const MAX_BODY_BYTES: usize = 1_048_576;
pub const OS_TYPES: &[&str] = &["darwin", "linux", "windows"];
pub const HOST_ARCHS: &[&str] = &["amd64", "arm64"];

#[derive(Debug, Clone, PartialEq)]
pub struct ResourceInfo {
    pub app: String,
    pub app_version: String,
    pub install_id: Uuid,
    pub session_id: Uuid,
    pub os: String,
    pub arch: String,
}

#[derive(Debug, Clone)]
pub struct ParsedEvent {
    pub event_name: String,
    pub time: OffsetDateTime,
    pub attributes: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Rejection {
    pub index: usize,
    pub reason: String,
}

#[derive(Debug)]
pub struct ValidatedBatch {
    pub resource: ResourceInfo,
    pub events: Vec<ParsedEvent>,
    pub rejections: Vec<Rejection>,
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum BatchError {
    #[error("request must contain exactly one resourceLogs block")]
    ExpectedSingleResource,
    #[error("missing or invalid required resource attribute `{0}`")]
    BadResourceAttr(&'static str),
    #[error("service.name `{0}` is not in the app allowlist")]
    UnknownApp(String),
    #[error("batch exceeds {MAX_RECORDS_PER_BATCH} records")]
    TooManyRecords,
}

fn attr_str<'a>(attrs: &'a [KeyValue], key: &str) -> Option<&'a str> {
    attrs
        .iter()
        .find(|kv| kv.key == key)
        .and_then(|kv| kv.value.string_value.as_deref())
}

pub fn validate_batch(
    req: &ExportLogsServiceRequest,
    allowlist: &[String],
) -> Result<ValidatedBatch, BatchError> {
    let [rl] = req.resource_logs.as_slice() else {
        return Err(BatchError::ExpectedSingleResource);
    };
    let res_attrs = rl
        .resource
        .as_ref()
        .map(|r| r.attributes.as_slice())
        .unwrap_or(&[]);

    let app = attr_str(res_attrs, "service.name")
        .filter(|s| !s.is_empty())
        .ok_or(BatchError::BadResourceAttr("service.name"))?
        .to_string();
    if !allowlist.iter().any(|a| a == &app) {
        return Err(BatchError::UnknownApp(app));
    }
    let app_version = attr_str(res_attrs, "service.version")
        .filter(|s| !s.is_empty())
        .ok_or(BatchError::BadResourceAttr("service.version"))?
        .to_string();
    let install_id = attr_str(res_attrs, "service.instance.id")
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or(BatchError::BadResourceAttr("service.instance.id"))?;
    let session_id = attr_str(res_attrs, "session.id")
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or(BatchError::BadResourceAttr("session.id"))?;
    let os = attr_str(res_attrs, "os.type")
        .filter(|s| OS_TYPES.contains(s))
        .ok_or(BatchError::BadResourceAttr("os.type"))?
        .to_string();
    let arch = attr_str(res_attrs, "host.arch")
        .filter(|s| HOST_ARCHS.contains(s))
        .ok_or(BatchError::BadResourceAttr("host.arch"))?
        .to_string();

    let records: Vec<&LogRecord> = rl.scope_logs.iter().flat_map(|s| &s.log_records).collect();
    if records.len() > MAX_RECORDS_PER_BATCH {
        return Err(BatchError::TooManyRecords);
    }

    let resource = ResourceInfo { app, app_version, install_id, session_id, os, arch };
    let mut events = Vec::new();
    let mut rejections = Vec::new();
    for (index, rec) in records.iter().enumerate() {
        match parse_record(rec, &resource.app) {
            Ok(ev) => events.push(ev),
            Err(reason) => rejections.push(Rejection { index, reason }),
        }
    }
    Ok(ValidatedBatch { resource, events, rejections })
}

fn parse_record(rec: &LogRecord, app: &str) -> Result<ParsedEvent, String> {
    let event_name = rec
        .event_name
        .clone()
        .or_else(|| {
            rec.attributes
                .iter()
                .find(|kv| kv.key == "event.name")
                .and_then(|kv| kv.value.string_value.clone())
        })
        .ok_or_else(|| "missing event name (eventName field or event.name attribute)".to_string())?;
    if !event_name.starts_with(&format!("{app}.")) {
        return Err(format!("event name must be prefixed with `{app}.`"));
    }

    let nanos = rec
        .time_unix_nano
        .filter(|n| *n > 0)
        .ok_or_else(|| "missing or zero timeUnixNano".to_string())?;
    let time = OffsetDateTime::from_unix_timestamp_nanos(nanos as i128)
        .map_err(|_| "timeUnixNano out of range".to_string())?;

    let attrs: Vec<&KeyValue> = rec.attributes.iter().filter(|kv| kv.key != "event.name").collect();
    if attrs.len() > MAX_ATTRIBUTES_PER_RECORD {
        return Err(format!("more than {MAX_ATTRIBUTES_PER_RECORD} attributes"));
    }

    let mut attributes = Map::new();
    for kv in attrs {
        let v = &kv.value;
        let value = if let Some(s) = &v.string_value {
            if s.len() > MAX_ATTR_STRING_BYTES {
                return Err(format!("attribute `{}` exceeds {MAX_ATTR_STRING_BYTES} bytes", kv.key));
            }
            Value::String(s.clone())
        } else if let Some(b) = v.bool_value {
            Value::Bool(b)
        } else if let Some(i) = &v.int_value {
            Value::Number(
                i.parse::<i64>()
                    .map_err(|_| format!("attribute `{}` has invalid intValue", kv.key))?
                    .into(),
            )
        } else if let Some(d) = v.double_value {
            serde_json::Number::from_f64(d)
                .map(Value::Number)
                .ok_or_else(|| format!("attribute `{}` has non-finite doubleValue", kv.key))?
        } else {
            return Err(format!("attribute `{}` has unsupported value type", kv.key));
        };
        attributes.insert(kv.key.clone(), value);
    }
    Ok(ParsedEvent { event_name, time, attributes })
}
```

Add `pub mod profile;` to `crates/gauge-events/src/lib.rs`.

- [ ] **Step 4: Run to verify pass**

Run: `cargo test -p gauge-events`
Expected: PASS (13 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/gauge-events
git commit -m "feat(events): Gauge OTLP profile validation with partial rejection"
```

---

### Task 9: gauge-events — SPEC.md with pinned worked example

**Files:**
- Create: `crates/gauge-events/SPEC.md`, `crates/gauge-events/tests/spec_pin.rs`

- [ ] **Step 1: Write SPEC.md**

`crates/gauge-events/SPEC.md` — the sender-facing standard. Content (write exactly; the fenced JSON block must be byte-identical to `tests/fixtures/valid_batch.json`):

````markdown
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
<PASTE THE EXACT BYTES OF tests/fixtures/valid_batch.json HERE>
```
````

When writing the file, replace the `<PASTE ...>` placeholder line with the literal content of `tests/fixtures/valid_batch.json`.

- [ ] **Step 2: Write the pin test**

`crates/gauge-events/tests/spec_pin.rs`:

```rust
//! Tome-style doc pinning: the worked example in SPEC.md must be byte-for-byte
//! identical to the fixture that the validation tests exercise.

const SPEC: &str = include_str!("../SPEC.md");
const FIXTURE: &str = include_str!("fixtures/valid_batch.json");

#[test]
fn spec_worked_example_matches_fixture_exactly() {
    assert!(
        SPEC.contains(FIXTURE.trim()),
        "SPEC.md worked example has drifted from tests/fixtures/valid_batch.json"
    );
}

#[test]
fn spec_example_is_a_valid_gauge_batch() {
    let req: gauge_events::otlp::ExportLogsServiceRequest =
        serde_json::from_str(FIXTURE).unwrap();
    let batch = gauge_events::profile::validate_batch(&req, &["tome".to_string()]).unwrap();
    assert!(batch.rejections.is_empty());
}
```

- [ ] **Step 3: Run to verify pass**

Run: `cargo test -p gauge-events spec_pin`
Expected: PASS (2 tests). If the containment assert fails, the SPEC.md paste step was not byte-exact — fix SPEC.md, not the test.

- [ ] **Step 4: Commit**

```bash
git add crates/gauge-events
git commit -m "docs(events): SPEC.md profile standard with pinned worked example"
```

---

### Task 10: gauge-query — query DSL types

**Files:**
- Create: `crates/gauge-query/src/{field,request,response,meta,validate}.rs`
- Modify: `crates/gauge-query/src/lib.rs`

- [ ] **Step 1: Write the failing tests**

`crates/gauge-query/tests/dsl.rs`:

```rust
use gauge_query::*;

#[test]
fn parses_spec_example_request() {
    let json = r#"{
      "measures": ["unique_installs"],
      "dimensions": ["app", "event_name"],
      "filters": [{"field": "app", "op": "eq", "value": "tome"},
                  {"field": "attr.surface", "op": "eq", "value": "mcp"}],
      "time_range": {"last": "7d"},
      "granularity": "day",
      "order": [{"field": "unique_installs", "dir": "desc"}],
      "limit": 100
    }"#;
    let req: QueryRequest = serde_json::from_str(json).unwrap();
    assert_eq!(req.measures, vec![Measure::UniqueInstalls]);
    assert_eq!(req.dimensions, vec![Field::App, Field::EventName]);
    assert_eq!(req.filters[1].field, Field::Attr("surface".into()));
    assert_eq!(req.granularity, Some(Granularity::Day));
    assert_eq!(req.limit, Some(100));
    validate(&req).unwrap();
}

#[test]
fn field_round_trips_attr_grammar() {
    assert_eq!(Field::parse("attr.latency_bucket").unwrap(), Field::Attr("latency_bucket".into()));
    assert_eq!(Field::Attr("x".into()).to_string(), "attr.x");
    assert!(Field::parse("attr.").is_err());
    assert!(Field::parse("attr.bad key").is_err());
    assert!(Field::parse("install_id").is_err()); // never queryable as a dimension
}

#[test]
fn unknown_top_level_field_rejected() {
    let json = r#"{"measures":["count"],"time_range":{"last":"1d"},"nope":1}"#;
    assert!(serde_json::from_str::<QueryRequest>(json).is_err());
}

#[test]
fn validate_rejects_empty_measures() {
    let req = QueryRequest { measures: vec![], ..minimal() };
    assert!(matches!(validate(&req), Err(QueryError::EmptyMeasures)));
}

#[test]
fn validate_rejects_limit_over_cap() {
    let req = QueryRequest { limit: Some(10_001), ..minimal() };
    assert!(matches!(validate(&req), Err(QueryError::LimitTooLarge(10_001))));
}

#[test]
fn validate_rejects_bad_time_range() {
    for bad in ["7", "7w", "0d", "400d", "-1h"] {
        let req = QueryRequest { time_range: TimeRange::Last { last: bad.into() }, ..minimal() };
        assert!(validate(&req).is_err(), "{bad} should be invalid");
    }
}

#[test]
fn validate_filter_op_value_rules() {
    // exists takes no value and only applies to attr fields
    let ok = Filter { field: Field::Attr("k".into()), op: FilterOp::Exists, value: None };
    let req = QueryRequest { filters: vec![ok], ..minimal() };
    validate(&req).unwrap();

    let bad = Filter { field: Field::App, op: FilterOp::Exists, value: None };
    let req = QueryRequest { filters: vec![bad], ..minimal() };
    assert!(validate(&req).is_err());

    let bad = Filter { field: Field::App, op: FilterOp::Eq, value: None };
    let req = QueryRequest { filters: vec![bad], ..minimal() };
    assert!(validate(&req).is_err());

    let bad = Filter { field: Field::App, op: FilterOp::In, value: Some(FilterValue::One("x".into())) };
    let req = QueryRequest { filters: vec![bad], ..minimal() };
    assert!(validate(&req).is_err());
}

fn minimal() -> QueryRequest {
    QueryRequest {
        measures: vec![Measure::Count],
        dimensions: vec![],
        filters: vec![],
        time_range: TimeRange::Last { last: "1d".into() },
        granularity: None,
        order: vec![],
        limit: None,
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p gauge-query`
Expected: FAIL — types not found.

- [ ] **Step 3: Implement**

`crates/gauge-query/src/field.rs`:

```rust
use crate::validate::QueryError;

/// Queryable fields: typed envelope columns or `attr.<key>` JSONB extractions.
/// install_id/session_id are deliberately NOT addressable (anonymity: no
/// per-install drill-down through the API).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Field {
    App,
    EventName,
    AppVersion,
    Os,
    Arch,
    Attr(String),
}

impl Field {
    pub fn parse(s: &str) -> Result<Self, QueryError> {
        Ok(match s {
            "app" => Self::App,
            "event_name" => Self::EventName,
            "app_version" => Self::AppVersion,
            "os" => Self::Os,
            "arch" => Self::Arch,
            other => {
                let key = other
                    .strip_prefix("attr.")
                    .filter(|k| {
                        !k.is_empty()
                            && k.len() <= 64
                            && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
                    })
                    .ok_or_else(|| QueryError::UnknownField(other.to_string()))?;
                Self::Attr(key.to_string())
            }
        })
    }
}

impl std::fmt::Display for Field {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::App => f.write_str("app"),
            Self::EventName => f.write_str("event_name"),
            Self::AppVersion => f.write_str("app_version"),
            Self::Os => f.write_str("os"),
            Self::Arch => f.write_str("arch"),
            Self::Attr(k) => write!(f, "attr.{k}"),
        }
    }
}

impl serde::Serialize for Field {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

impl<'de> serde::Deserialize<'de> for Field {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Self::parse(&s).map_err(serde::de::Error::custom)
    }
}

impl schemars::JsonSchema for Field {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Field".into()
    }
    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "string",
            "description": "One of: app, event_name, app_version, os, arch, or attr.<key> for an event attribute"
        })
    }
}
```

`crates/gauge-query/src/request.rs`:

```rust
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::field::Field;

pub const DEFAULT_LIMIT: u32 = 1_000;
pub const MAX_LIMIT: u32 = 10_000;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct QueryRequest {
    pub measures: Vec<Measure>,
    #[serde(default)]
    pub dimensions: Vec<Field>,
    #[serde(default)]
    pub filters: Vec<Filter>,
    pub time_range: TimeRange,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub granularity: Option<Granularity>,
    #[serde(default)]
    pub order: Vec<Order>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Measure {
    Count,
    UniqueInstalls,
    UniqueSessions,
}

impl Measure {
    pub fn alias(&self) -> &'static str {
        match self {
            Self::Count => "count",
            Self::UniqueInstalls => "unique_installs",
            Self::UniqueSessions => "unique_sessions",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Filter {
    pub field: Field,
    pub op: FilterOp,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<FilterValue>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum FilterOp {
    Eq,
    Neq,
    In,
    Exists,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum FilterValue {
    One(String),
    Many(Vec<String>),
}

/// Relative ranges use "<N>h" or "<N>d" (max 365d). Absolute uses RFC3339.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum TimeRange {
    Last { last: String },
    Absolute { from: String, to: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Granularity {
    Hour,
    Day,
    Week,
}

impl Granularity {
    pub fn date_trunc_unit(&self) -> &'static str {
        match self {
            Self::Hour => "hour",
            Self::Day => "day",
            Self::Week => "week",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Order {
    /// References an output alias: a measure name, a dimension string, or "time_bucket".
    pub field: String,
    pub dir: Dir,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Dir {
    Asc,
    Desc,
}
```

`crates/gauge-query/src/validate.rs`:

```rust
use thiserror::Error;
use time::{Duration, OffsetDateTime, format_description::well_known::Rfc3339};

use crate::request::{FilterOp, FilterValue, MAX_LIMIT, QueryRequest, TimeRange};
use crate::field::Field;

#[derive(Debug, Error, PartialEq)]
pub enum QueryError {
    #[error("unknown field `{0}`")]
    UnknownField(String),
    #[error("measures must not be empty")]
    EmptyMeasures,
    #[error("limit {0} exceeds the maximum of {MAX_LIMIT}")]
    LimitTooLarge(u32),
    #[error("invalid time range `{0}` (expected <N>h, <N>d up to 365d, or RFC3339 from/to)")]
    BadTimeRange(String),
    #[error("filter on `{0}`: op `{1}` requires {2}")]
    BadFilter(String, String, &'static str),
    #[error("order field `{0}` is not in the selected output")]
    BadOrderField(String),
}

pub fn parse_last(s: &str) -> Result<Duration, QueryError> {
    let bad = || QueryError::BadTimeRange(s.to_string());
    if s.len() < 2 {
        return Err(bad());
    }
    let (num, unit) = s.split_at(s.len() - 1);
    let n: i64 = num.parse().map_err(|_| bad())?;
    if n < 1 {
        return Err(bad());
    }
    let d = match unit {
        "h" => Duration::hours(n),
        "d" => Duration::days(n),
        _ => return Err(bad()),
    };
    if d > Duration::days(365) {
        return Err(bad());
    }
    Ok(d)
}

/// Resolve a TimeRange to concrete [from, to) bounds.
pub fn resolve_time_range(tr: &TimeRange, now: OffsetDateTime) -> Result<(OffsetDateTime, OffsetDateTime), QueryError> {
    match tr {
        TimeRange::Last { last } => Ok((now - parse_last(last)?, now)),
        TimeRange::Absolute { from, to } => {
            let f = OffsetDateTime::parse(from, &Rfc3339)
                .map_err(|_| QueryError::BadTimeRange(from.clone()))?;
            let t = OffsetDateTime::parse(to, &Rfc3339)
                .map_err(|_| QueryError::BadTimeRange(to.clone()))?;
            if f >= t {
                return Err(QueryError::BadTimeRange(format!("{from}..{to}")));
            }
            Ok((f, t))
        }
    }
}

pub fn validate(req: &QueryRequest) -> Result<(), QueryError> {
    if req.measures.is_empty() {
        return Err(QueryError::EmptyMeasures);
    }
    if let Some(l) = req.limit
        && l > MAX_LIMIT
    {
        return Err(QueryError::LimitTooLarge(l));
    }
    resolve_time_range(&req.time_range, OffsetDateTime::now_utc())?;
    for f in &req.filters {
        let fname = f.field.to_string();
        let opname = format!("{:?}", f.op).to_lowercase();
        match (f.op, &f.value) {
            (FilterOp::Eq | FilterOp::Neq, Some(FilterValue::One(_))) => {}
            (FilterOp::In, Some(FilterValue::Many(v))) if !v.is_empty() => {}
            (FilterOp::Exists, None) => {
                if !matches!(f.field, Field::Attr(_)) {
                    return Err(QueryError::BadFilter(fname, opname, "an attr.<key> field"));
                }
            }
            (FilterOp::Eq | FilterOp::Neq, _) => {
                return Err(QueryError::BadFilter(fname, opname, "a single string value"));
            }
            (FilterOp::In, _) => {
                return Err(QueryError::BadFilter(fname, opname, "a non-empty string array"));
            }
            (FilterOp::Exists, Some(_)) => {
                return Err(QueryError::BadFilter(fname, opname, "no value"));
            }
        }
    }
    Ok(())
}
```

`crates/gauge-query/src/response.rs`:

```rust
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct QueryResponse {
    /// One JSON object per row, keyed by output aliases
    /// (measure names, dimension strings, "time_bucket").
    pub rows: Vec<serde_json::Value>,
    pub truncated: bool,
    pub elapsed_ms: u64,
}
```

`crates/gauge-query/src/meta.rs`:

```rust
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MetaResponse {
    pub apps: Vec<AppMeta>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AppMeta {
    pub app: String,
    pub event_names: Vec<String>,
    pub attribute_keys: Vec<String>,
    /// RFC3339, None when the app has no events.
    pub first_event: Option<String>,
    pub last_event: Option<String>,
    pub total_events: i64,
}
```

`crates/gauge-query/src/lib.rs`:

```rust
pub mod field;
pub mod meta;
pub mod request;
pub mod response;
pub mod validate;

pub use field::Field;
pub use meta::{AppMeta, MetaResponse};
pub use request::{
    DEFAULT_LIMIT, Dir, Filter, FilterOp, FilterValue, Granularity, MAX_LIMIT, Measure, Order,
    QueryRequest, TimeRange,
};
pub use response::QueryResponse;
pub use validate::{QueryError, parse_last, resolve_time_range, validate};
```

- [ ] **Step 4: Run to verify pass**

Run: `cargo test -p gauge-query`
Expected: PASS (7 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/gauge-query
git commit -m "feat(query): typed query DSL with validation, meta and response types"
```

---

### Task 11: gauge-server — scaffold (config, error envelope, state, health)

**Files:**
- Create: `crates/gauge-server/src/lib.rs`, `src/config.rs`, `src/error.rs`, `src/state.rs`, `src/app.rs`, `src/routes/mod.rs`, `src/routes/health.rs`
- Create: `crates/gauge-server/tests/common/mod.rs`, `crates/gauge-server/tests/health.rs`
- Modify: `crates/gauge-server/src/main.rs`

Note: gauge-server is structured as lib + thin binary so integration tests can build routers directly.

- [ ] **Step 1: Write the failing test**

`crates/gauge-server/tests/common/mod.rs` (shared by all server integration tests):

```rust
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use gauge_auth::{ChallengeStore, Keypair, SigningSecret, UserStore};
use gauge_server::state::AppState;
use sqlx::PgPool;
use tower::ServiceExt as _;

pub const TEST_SECRET: [u8; 32] = [7u8; 32];

/// AppState with one registered admin user ("alice") and apps tome + midnight-manual allowlisted.
pub fn test_state(pool: PgPool) -> (AppState, Keypair) {
    let kp = Keypair::generate();
    let toml = format!(
        "schema_version = 1\n\n[[users]]\nuser_id = \"alice\"\nrole = \"admin\"\npublic_key = \"{}\"\n",
        kp.public_wire()
    );
    let state = AppState {
        pool,
        allowlist: Arc::new(vec!["tome".into(), "midnight-manual".into()]),
        users: Arc::new(UserStore::from_toml_str(&toml).unwrap()),
        challenges: Arc::new(ChallengeStore::new()),
        secret: Arc::new(SigningSecret::new(TEST_SECRET.to_vec()).unwrap()),
    };
    (state, kp)
}

pub async fn send_json(
    app: &Router,
    method: &str,
    uri: &str,
    body: Option<serde_json::Value>,
    bearer: Option<&str>,
) -> (StatusCode, serde_json::Value) {
    let mut req = Request::builder().method(method).uri(uri);
    if let Some(b) = bearer {
        req = req.header(header::AUTHORIZATION, format!("Bearer {b}"));
    }
    let req = match body {
        Some(v) => req
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(v.to_string()))
            .unwrap(),
        None => req.body(Body::empty()).unwrap(),
    };
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let json = if bytes.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::String(
            String::from_utf8_lossy(&bytes).into_owned(),
        ))
    };
    (status, json)
}
```

`crates/gauge-server/tests/health.rs`:

```rust
mod common;

use axum::http::StatusCode;
use gauge_server::app::build_router;
use sqlx::PgPool;

#[sqlx::test]
async fn healthz_and_readyz_ok(pool: PgPool) {
    let (state, _kp) = common::test_state(pool);
    let app = build_router(state);
    let (status, _) = common::send_json(&app, "GET", "/healthz", None, None).await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = common::send_json(&app, "GET", "/readyz", None, None).await;
    assert_eq!(status, StatusCode::OK);
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p gauge-server`
Expected: FAIL — `gauge_server::state` not found.

- [ ] **Step 3: Implement**

`crates/gauge-server/src/lib.rs`:

```rust
pub mod app;
pub mod config;
pub mod error;
pub mod routes;
pub mod state;
```

`crates/gauge-server/src/config.rs`:

```rust
use std::net::SocketAddr;

use gauge_auth::SigningSecret;

pub struct Config {
    pub listen_addr: SocketAddr,
    pub database_url: String,
    pub jwt_secret: SigningSecret,
    pub user_store_toml: String,
    pub app_allowlist: Vec<String>,
    pub rate_logs_per_min: u32,
    pub rate_auth_per_min: u32,
    pub rate_user_per_min: u32,
}

impl Config {
    pub fn from_env() -> Result<Self, String> {
        fn req(k: &str) -> Result<String, String> {
            std::env::var(k).map_err(|_| format!("missing required env var {k}"))
        }
        fn opt_u32(k: &str, default: u32) -> Result<u32, String> {
            match std::env::var(k) {
                Ok(v) => v.parse().map_err(|_| format!("{k} must be an integer")),
                Err(_) => Ok(default),
            }
        }
        let listen_addr = std::env::var("GAUGE_LISTEN_ADDR")
            .unwrap_or_else(|_| "0.0.0.0:8080".into())
            .parse()
            .map_err(|e| format!("GAUGE_LISTEN_ADDR: {e}"))?;
        Ok(Self {
            listen_addr,
            database_url: req("DATABASE_URL")?,
            jwt_secret: SigningSecret::new(req("GAUGE_JWT_SECRET")?.into_bytes())
                .map_err(|e| e.to_string())?,
            user_store_toml: req("GAUGE_USER_STORE")?,
            app_allowlist: req("GAUGE_APP_ALLOWLIST")?
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect(),
            rate_logs_per_min: opt_u32("GAUGE_RATE_LOGS_PER_MIN", 60)?,
            rate_auth_per_min: opt_u32("GAUGE_RATE_AUTH_PER_MIN", 10)?,
            rate_user_per_min: opt_u32("GAUGE_RATE_USER_PER_MIN", 120)?,
        })
    }
}
```

`crates/gauge-server/src/error.rs`:

```rust
use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

/// The one error envelope: {code, message, remediation}.
/// Never put attribute values or request bodies in `message`.
#[derive(Debug)]
pub struct ApiError {
    pub status: StatusCode,
    pub code: &'static str,
    pub message: String,
    pub remediation: Option<String>,
}

impl ApiError {
    pub fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self { status, code, message: message.into(), remediation: None }
    }
    pub fn with_remediation(mut self, r: impl Into<String>) -> Self {
        self.remediation = Some(r.into());
        self
    }
    pub fn bad_request(code: &'static str, m: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, code, m)
    }
    pub fn unauthorized(code: &'static str, m: impl Into<String>) -> Self {
        Self::new(StatusCode::UNAUTHORIZED, code, m)
    }
    pub fn forbidden(code: &'static str, m: impl Into<String>) -> Self {
        Self::new(StatusCode::FORBIDDEN, code, m)
    }
    pub fn not_found(code: &'static str, m: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, code, m)
    }
    pub fn unprocessable(code: &'static str, m: impl Into<String>) -> Self {
        Self::new(StatusCode::UNPROCESSABLE_ENTITY, code, m)
    }
    pub fn service_unavailable(code: &'static str, m: impl Into<String>) -> Self {
        Self::new(StatusCode::SERVICE_UNAVAILABLE, code, m)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = serde_json::json!({
            "code": self.code,
            "message": self.message,
            "remediation": self.remediation,
        });
        (self.status, Json(body)).into_response()
    }
}
```

`crates/gauge-server/src/state.rs`:

```rust
use std::sync::Arc;

use gauge_auth::{ChallengeStore, SigningSecret, UserStore};
use sqlx::PgPool;

use crate::config::Config;

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub allowlist: Arc<Vec<String>>,
    pub users: Arc<UserStore>,
    pub challenges: Arc<ChallengeStore>,
    pub secret: Arc<SigningSecret>,
}

impl AppState {
    pub fn from_config(cfg: Config, pool: PgPool) -> Result<Self, String> {
        Ok(Self {
            pool,
            allowlist: Arc::new(cfg.app_allowlist),
            users: Arc::new(
                UserStore::from_toml_str(&cfg.user_store_toml).map_err(|e| e.to_string())?,
            ),
            challenges: Arc::new(ChallengeStore::new()),
            secret: Arc::new(cfg.jwt_secret),
        })
    }
}
```

`crates/gauge-server/src/routes/mod.rs`:

```rust
pub mod health;
```

`crates/gauge-server/src/routes/health.rs`:

```rust
use axum::extract::State;

use crate::error::ApiError;
use crate::state::AppState;

pub async fn healthz() -> &'static str {
    "ok"
}

pub async fn readyz(State(st): State<AppState>) -> Result<&'static str, ApiError> {
    sqlx::query("SELECT 1")
        .execute(&st.pool)
        .await
        .map_err(|_| ApiError::service_unavailable("db_unavailable", "database not reachable"))?;
    Ok("ok")
}
```

`crates/gauge-server/src/app.rs`:

```rust
use axum::Router;
use axum::routing::get;

use crate::routes;
use crate::state::AppState;

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(routes::health::healthz))
        .route("/readyz", get(routes::health::readyz))
        .layer(tower_http::request_id::PropagateRequestIdLayer::x_request_id())
        .layer(tower_http::trace::TraceLayer::new_for_http())
        .layer(tower_http::request_id::SetRequestIdLayer::x_request_id(
            tower_http::request_id::MakeRequestUuid,
        ))
        .with_state(state)
}
```

(The three tower-http layers give every request an `x-request-id` and span-per-request JSON logs — the spec's "request IDs, never payloads" logging. When later tasks restructure `build_router` into merged sub-routers, keep these three layers as the outermost layers on the final merged router.)

`crates/gauge-server/src/main.rs`:

```rust
use gauge_server::app::build_router;
use gauge_server::config::Config;
use gauge_server::state::AppState;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();
    let cfg = Config::from_env()?;
    let addr = cfg.listen_addr;
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(5)
        .connect(&cfg.database_url)
        .await?;
    let state = AppState::from_config(cfg, pool)?;
    let app = build_router(state);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(%addr, "gauge-server listening");
    axum::serve(listener, app).await?;
    Ok(())
}
```

Also add `tower.workspace = true` usage: add to `crates/gauge-server/Cargo.toml` `[dev-dependencies]`: `tower = { workspace = true, features = ["util"] }`.

- [ ] **Step 4: Run to verify pass**

Run: `cargo test -p gauge-server` (requires `DATABASE_URL` exported)
Expected: PASS (1 test).

- [ ] **Step 5: Commit**

```bash
git add crates/gauge-server
git commit -m "feat(server): axum scaffold with config, error envelope, health routes"
```

---

### Task 12: gauge-server — events migration + batch insert

**Files:**
- Create: `migrations/0001_events.sql`, `crates/gauge-server/src/db.rs`, `crates/gauge-server/tests/db.rs`
- Modify: `crates/gauge-server/src/lib.rs` (add `pub mod db;`), `crates/gauge-server/src/main.rs` (add migration run)

- [ ] **Step 1: Write the migration**

`migrations/0001_events.sql` (exact DDL from the spec):

```sql
CREATE TABLE events (
    id           BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    app          TEXT        NOT NULL,
    app_version  TEXT        NOT NULL,
    install_id   UUID        NOT NULL,
    session_id   UUID        NOT NULL,
    os           TEXT        NOT NULL,
    arch         TEXT        NOT NULL,
    event_name   TEXT        NOT NULL,
    time         TIMESTAMPTZ NOT NULL,
    received_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    attributes   JSONB       NOT NULL DEFAULT '{}'::jsonb
);

CREATE INDEX events_app_name_time_idx    ON events (app, event_name, time);
CREATE INDEX events_app_install_time_idx ON events (app, install_id, time);
```

- [ ] **Step 2: Write the failing test**

`crates/gauge-server/tests/db.rs`:

```rust
mod common;

use gauge_events::profile::{ParsedEvent, ResourceInfo};
use gauge_server::db;
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

pub fn test_resource(app: &str) -> ResourceInfo {
    ResourceInfo {
        app: app.into(),
        app_version: "0.1.0".into(),
        install_id: Uuid::new_v4(),
        session_id: Uuid::new_v4(),
        os: "darwin".into(),
        arch: "arm64".into(),
    }
}

pub fn test_event(app: &str, name: &str, time: OffsetDateTime) -> ParsedEvent {
    let mut attributes = serde_json::Map::new();
    attributes.insert("surface".into(), serde_json::json!("cli"));
    ParsedEvent { event_name: format!("{app}.{name}"), time, attributes }
}

#[sqlx::test(migrations = "../../migrations")]
async fn insert_events_persists_rows(pool: PgPool) {
    let res = test_resource("tome");
    let now = OffsetDateTime::now_utc();
    let events = vec![test_event("tome", "search", now), test_event("tome", "install", now)];
    db::insert_events(&pool, &res, &events).await.unwrap();

    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM events").fetch_one(&pool).await.unwrap();
    assert_eq!(n, 2);
    let (app, name, attrs): (String, String, serde_json::Value) = sqlx::query_as(
        "SELECT app, event_name, attributes FROM events ORDER BY event_name LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(app, "tome");
    assert_eq!(name, "tome.install");
    assert_eq!(attrs["surface"], serde_json::json!("cli"));
}

#[sqlx::test(migrations = "../../migrations")]
async fn insert_empty_slice_is_noop(pool: PgPool) {
    db::insert_events(&pool, &test_resource("tome"), &[]).await.unwrap();
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM events").fetch_one(&pool).await.unwrap();
    assert_eq!(n, 0);
}
```

- [ ] **Step 3: Run to verify failure**

Run: `cargo test -p gauge-server db`
Expected: FAIL — module `db` not found.

- [ ] **Step 4: Implement**

`crates/gauge-server/src/db.rs`:

```rust
use gauge_events::profile::{ParsedEvent, ResourceInfo};
use sqlx::PgPool;
use time::OffsetDateTime;

/// Batch insert via UNNEST: one statement regardless of batch size,
/// fully parameterized.
pub async fn insert_events(
    pool: &PgPool,
    res: &ResourceInfo,
    events: &[ParsedEvent],
) -> Result<(), sqlx::Error> {
    if events.is_empty() {
        return Ok(());
    }
    let mut names: Vec<String> = Vec::with_capacity(events.len());
    let mut times: Vec<OffsetDateTime> = Vec::with_capacity(events.len());
    let mut attrs: Vec<serde_json::Value> = Vec::with_capacity(events.len());
    for e in events {
        names.push(e.event_name.clone());
        times.push(e.time);
        attrs.push(serde_json::Value::Object(e.attributes.clone()));
    }
    sqlx::query(
        r#"INSERT INTO events (app, app_version, install_id, session_id, os, arch, event_name, time, attributes)
           SELECT $1, $2, $3, $4, $5, $6, n, t, a
           FROM UNNEST($7::text[], $8::timestamptz[], $9::jsonb[]) AS u(n, t, a)"#,
    )
    .bind(&res.app)
    .bind(&res.app_version)
    .bind(res.install_id)
    .bind(res.session_id)
    .bind(&res.os)
    .bind(&res.arch)
    .bind(&names)
    .bind(&times)
    .bind(&attrs)
    .execute(pool)
    .await?;
    Ok(())
}
```

Add `pub mod db;` to `lib.rs`. In `main.rs`, after the pool is created add:

```rust
    sqlx::migrate!("../../migrations").run(&pool).await?;
```

- [ ] **Step 5: Run to verify pass**

Run: `cargo test -p gauge-server`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add migrations crates/gauge-server
git commit -m "feat(server): events table migration and UNNEST batch insert"
```

---

### Task 13: gauge-server — OTLP ingest endpoint

**Files:**
- Create: `crates/gauge-server/src/routes/ingest.rs`, `crates/gauge-server/tests/ingest.rs`
- Modify: `crates/gauge-server/src/routes/mod.rs`, `crates/gauge-server/src/app.rs`

- [ ] **Step 1: Write the failing tests**

`crates/gauge-server/tests/ingest.rs`:

```rust
mod common;

use axum::http::StatusCode;
use gauge_server::app::build_router;
use sqlx::PgPool;

const FIXTURE: &str = include_str!("../../gauge-events/tests/fixtures/valid_batch.json");

#[sqlx::test(migrations = "../../migrations")]
async fn valid_batch_is_stored(pool: PgPool) {
    let (state, _kp) = common::test_state(pool.clone());
    let app = build_router(state);
    let body: serde_json::Value = serde_json::from_str(FIXTURE).unwrap();
    let (status, resp) = common::send_json(&app, "POST", "/v1/logs", Some(body), None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(resp.get("partialSuccess").is_none());
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM events").fetch_one(&pool).await.unwrap();
    assert_eq!(n, 1);
    let name: String = sqlx::query_scalar("SELECT event_name FROM events").fetch_one(&pool).await.unwrap();
    assert_eq!(name, "tome.search");
}

#[sqlx::test(migrations = "../../migrations")]
async fn unknown_app_is_rejected_whole(pool: PgPool) {
    let (state, _kp) = common::test_state(pool.clone());
    let app = build_router(state);
    let mut body: serde_json::Value = serde_json::from_str(FIXTURE).unwrap();
    body["resourceLogs"][0]["resource"]["attributes"][0]["value"]["stringValue"] = "evil-app".into();
    let (status, resp) = common::send_json(&app, "POST", "/v1/logs", Some(body), None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(resp["code"], "invalid_batch");
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM events").fetch_one(&pool).await.unwrap();
    assert_eq!(n, 0);
}

#[sqlx::test(migrations = "../../migrations")]
async fn bad_record_yields_partial_success(pool: PgPool) {
    let (state, _kp) = common::test_state(pool.clone());
    let app = build_router(state);
    let mut body: serde_json::Value = serde_json::from_str(FIXTURE).unwrap();
    // append a record with a wrong-prefix event name
    let mut bad = body["resourceLogs"][0]["scopeLogs"][0]["logRecords"][0].clone();
    bad["eventName"] = "wrong.prefix".into();
    bad["attributes"] = serde_json::json!([]);
    body["resourceLogs"][0]["scopeLogs"][0]["logRecords"].as_array_mut().unwrap().push(bad);
    let (status, resp) = common::send_json(&app, "POST", "/v1/logs", Some(body), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(resp["partialSuccess"]["rejectedLogRecords"], 1);
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM events").fetch_one(&pool).await.unwrap();
    assert_eq!(n, 1);
}

#[sqlx::test(migrations = "../../migrations")]
async fn malformed_json_is_400(pool: PgPool) {
    let (state, _kp) = common::test_state(pool);
    let app = build_router(state);
    let (status, resp) =
        common::send_json(&app, "POST", "/v1/logs", Some(serde_json::json!("not otlp")), None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(resp["code"], "invalid_otlp");
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p gauge-server ingest`
Expected: FAIL — 404s (route not mounted).

- [ ] **Step 3: Implement**

`crates/gauge-server/src/routes/ingest.rs`:

```rust
use axum::Json;
use axum::body::Bytes;
use axum::extract::State;
use gauge_events::otlp::{ExportLogsPartialSuccess, ExportLogsServiceRequest, ExportLogsServiceResponse};
use gauge_events::profile::validate_batch;

use crate::db;
use crate::error::ApiError;
use crate::state::AppState;

/// Anonymous OTLP/HTTP logs ingest. Privacy rules for this handler:
/// never log request bodies, attribute values, or client IPs.
pub async fn ingest(
    State(st): State<AppState>,
    body: Bytes,
) -> Result<Json<ExportLogsServiceResponse>, ApiError> {
    let req: ExportLogsServiceRequest = serde_json::from_slice(&body)
        .map_err(|_| ApiError::bad_request("invalid_otlp", "request body is not valid OTLP/HTTP JSON (logs signal)"))?;

    let batch = validate_batch(&req, &st.allowlist)
        .map_err(|e| ApiError::bad_request("invalid_batch", e.to_string()))?;

    if !batch.events.is_empty() {
        db::insert_events(&st.pool, &batch.resource, &batch.events)
            .await
            .map_err(|e| {
                tracing::error!(kind = %e.to_string(), "ingest insert failed");
                ApiError::service_unavailable("db_unavailable", "could not persist events; retry later")
            })?;
    }

    tracing::info!(
        app = %batch.resource.app,
        accepted = batch.events.len(),
        rejected = batch.rejections.len(),
        "ingest"
    );

    let partial_success = (!batch.rejections.is_empty()).then(|| ExportLogsPartialSuccess {
        rejected_log_records: batch.rejections.len() as i64,
        error_message: batch
            .rejections
            .iter()
            .take(5)
            .map(|r| format!("record {}: {}", r.index, r.reason))
            .collect::<Vec<_>>()
            .join("; "),
    });
    Ok(Json(ExportLogsServiceResponse { partial_success }))
}
```

Update `routes/mod.rs` (`pub mod ingest;`) and `app.rs`:

```rust
use axum::extract::DefaultBodyLimit;
use axum::routing::{get, post};

pub fn build_router(state: AppState) -> Router {
    let public = Router::new()
        .route("/healthz", get(routes::health::healthz))
        .route("/readyz", get(routes::health::readyz));
    let ingest = Router::new()
        .route("/v1/logs", post(routes::ingest::ingest))
        .layer(DefaultBodyLimit::max(gauge_events::profile::MAX_BODY_BYTES));
    public.merge(ingest).with_state(state)
}
```

- [ ] **Step 4: Run to verify pass**

Run: `cargo test -p gauge-server`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/gauge-server
git commit -m "feat(server): anonymous OTLP ingest with partial success semantics"
```

---

### Task 14: gauge-server — auth endpoints (challenge/verify)

**Files:**
- Create: `crates/gauge-server/src/routes/auth.rs`, `crates/gauge-server/tests/auth.rs`
- Modify: `crates/gauge-server/src/routes/mod.rs`, `crates/gauge-server/src/app.rs`

- [ ] **Step 1: Write the failing tests**

`crates/gauge-server/tests/auth.rs`:

```rust
mod common;

use axum::http::StatusCode;
use base64::Engine as _;
use gauge_auth::{SigningSecret, sign_challenge, verify_token};
use gauge_server::app::build_router;
use sqlx::PgPool;

#[sqlx::test]
async fn full_handshake_issues_valid_jwt(pool: PgPool) {
    let (state, kp) = common::test_state(pool);
    let app = build_router(state);

    let (status, ch) = common::send_json(
        &app, "POST", "/v1/auth/challenge",
        Some(serde_json::json!({"user_id": "alice"})), None,
    ).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(ch["expires_in_s"], 60);

    let sig = sign_challenge(&kp, ch["nonce_b64"].as_str().unwrap()).unwrap();
    let (status, v) = common::send_json(
        &app, "POST", "/v1/auth/verify",
        Some(serde_json::json!({"challenge_id": ch["challenge_id"], "signature_b64": sig})), None,
    ).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(v["user_id"], "alice");

    let secret = SigningSecret::new(common::TEST_SECRET.to_vec()).unwrap();
    let claims = verify_token(&secret, v["token"].as_str().unwrap()).unwrap();
    assert_eq!(claims.sub, "alice");
    assert_eq!(claims.exp, v["expires_at"].as_i64().unwrap());
}

#[sqlx::test]
async fn unknown_user_is_404(pool: PgPool) {
    let (state, _kp) = common::test_state(pool);
    let app = build_router(state);
    let (status, _) = common::send_json(
        &app, "POST", "/v1/auth/challenge",
        Some(serde_json::json!({"user_id": "mallory"})), None,
    ).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[sqlx::test]
async fn wrong_signature_is_403(pool: PgPool) {
    let (state, _kp) = common::test_state(pool);
    let app = build_router(state);
    let (_, ch) = common::send_json(
        &app, "POST", "/v1/auth/challenge",
        Some(serde_json::json!({"user_id": "alice"})), None,
    ).await;
    let bogus = base64::engine::general_purpose::STANDARD_NO_PAD.encode([0u8; 64]);
    let (status, resp) = common::send_json(
        &app, "POST", "/v1/auth/verify",
        Some(serde_json::json!({"challenge_id": ch["challenge_id"], "signature_b64": bogus})), None,
    ).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(resp["code"], "invalid_signature");
}

#[sqlx::test]
async fn challenge_is_single_use(pool: PgPool) {
    let (state, kp) = common::test_state(pool);
    let app = build_router(state);
    let (_, ch) = common::send_json(
        &app, "POST", "/v1/auth/challenge",
        Some(serde_json::json!({"user_id": "alice"})), None,
    ).await;
    let sig = sign_challenge(&kp, ch["nonce_b64"].as_str().unwrap()).unwrap();
    let body = serde_json::json!({"challenge_id": ch["challenge_id"], "signature_b64": sig});
    let (first, _) = common::send_json(&app, "POST", "/v1/auth/verify", Some(body.clone()), None).await;
    assert_eq!(first, StatusCode::OK);
    let (second, _) = common::send_json(&app, "POST", "/v1/auth/verify", Some(body), None).await;
    assert_eq!(second, StatusCode::NOT_FOUND);
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p gauge-server auth`
Expected: FAIL — 404s (routes not mounted).

- [ ] **Step 3: Implement**

`crates/gauge-server/src/routes/auth.rs`:

```rust
use axum::Json;
use axum::extract::State;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD_NO_PAD;
use gauge_auth::protocol::{ChallengeRequest, ChallengeResponse, VerifyRequest, VerifyResponse};
use gauge_auth::wire::{b64_decode_flexible, parse_public_key_wire};
use gauge_auth::{AuthError, mint_token, verify_signature};
use time::OffsetDateTime;

use crate::error::ApiError;
use crate::state::AppState;

pub async fn challenge(
    State(st): State<AppState>,
    Json(req): Json<ChallengeRequest>,
) -> Result<Json<ChallengeResponse>, ApiError> {
    if req.user_id.trim().is_empty() {
        return Err(ApiError::bad_request("invalid_request", "user_id must not be empty"));
    }
    let now = OffsetDateTime::now_utc();
    st.challenges.purge_expired(now);
    if st.users.get(&req.user_id).is_none() {
        // same body as a consumed challenge: prevents user enumeration
        return Err(ApiError::not_found("not_found", "unknown user or challenge"));
    }
    let c = st.challenges.mint(&req.user_id, now);
    Ok(Json(ChallengeResponse {
        challenge_id: c.challenge_id,
        nonce_b64: STANDARD_NO_PAD.encode(c.nonce),
        expires_in_s: 60,
    }))
}

pub async fn verify(
    State(st): State<AppState>,
    Json(req): Json<VerifyRequest>,
) -> Result<Json<VerifyResponse>, ApiError> {
    let now = OffsetDateTime::now_utc();
    let challenge = st.challenges.consume(&req.challenge_id, now).map_err(|e| match e {
        AuthError::ChallengeExpired => ApiError::unauthorized("challenge_expired", "challenge expired")
            .with_remediation("request a new challenge and sign it within 60 seconds"),
        _ => ApiError::not_found("not_found", "unknown user or challenge"),
    })?;
    let user = st
        .users
        .get(&challenge.user_id)
        .ok_or_else(|| ApiError::not_found("not_found", "unknown user or challenge"))?;
    let key = parse_public_key_wire(&user.public_key)
        .map_err(|_| ApiError::service_unavailable("bad_user_store", "stored public key is invalid"))?;
    let sig = b64_decode_flexible(&req.signature_b64)
        .map_err(|_| ApiError::bad_request("invalid_request", "signature_b64 is not valid base64"))?;
    verify_signature(&key, &challenge.nonce, &sig).map_err(|_| {
        ApiError::forbidden("invalid_signature", "signature verification failed")
            .with_remediation("check that the local keypair matches the registered public key")
    })?;
    let (token, expires_at) = mint_token(&st.secret, &user.user_id, user.role, now)
        .map_err(|_| ApiError::service_unavailable("jwt_error", "could not mint token"))?;
    tracing::info!(user = %user.user_id, "admin token issued");
    Ok(Json(VerifyResponse { token, user_id: user.user_id.clone(), expires_at }))
}
```

Add `pub mod auth;` to `routes/mod.rs`. In `app.rs` add an auth sub-router and merge it:

```rust
    let auth = Router::new()
        .route("/v1/auth/challenge", post(routes::auth::challenge))
        .route("/v1/auth/verify", post(routes::auth::verify));
    public.merge(ingest).merge(auth).with_state(state)
```

- [ ] **Step 4: Run to verify pass**

Run: `cargo test -p gauge-server`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/gauge-server
git commit -m "feat(server): ed25519 challenge/response auth endpoints issuing JWTs"
```

---

### Task 15: gauge-server — bearer middleware

**Files:**
- Create: `crates/gauge-server/src/middleware/mod.rs`, `crates/gauge-server/src/middleware/bearer.rs`, `crates/gauge-server/tests/bearer.rs`
- Modify: `crates/gauge-server/src/lib.rs` (add `pub mod middleware;`)

- [ ] **Step 1: Write the failing tests**

`crates/gauge-server/tests/bearer.rs`:

```rust
mod common;

use axum::http::StatusCode;
use axum::routing::get;
use axum::{Extension, Router, middleware};
use gauge_auth::{Role, mint_token};
use gauge_server::middleware::bearer::{AuthContext, require_bearer};
use sqlx::PgPool;

async fn probe(Extension(ctx): Extension<AuthContext>) -> String {
    ctx.sub
}

fn probe_router(state: gauge_server::state::AppState) -> Router {
    Router::new()
        .route("/probe", get(probe))
        .layer(middleware::from_fn_with_state(state.clone(), require_bearer))
        .with_state(state)
}

#[sqlx::test]
async fn valid_token_passes_and_injects_context(pool: PgPool) {
    let (state, _kp) = common::test_state(pool);
    let (token, _) = mint_token(&state.secret, "alice", Role::Admin, time::OffsetDateTime::now_utc()).unwrap();
    let app = probe_router(state);
    let (status, body) = common::send_json(&app, "GET", "/probe", None, Some(&token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, serde_json::Value::String("alice".into()));
}

#[sqlx::test]
async fn missing_token_is_401(pool: PgPool) {
    let (state, _kp) = common::test_state(pool);
    let app = probe_router(state);
    let (status, body) = common::send_json(&app, "GET", "/probe", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["code"], "missing_token");
}

#[sqlx::test]
async fn garbage_token_is_401(pool: PgPool) {
    let (state, _kp) = common::test_state(pool);
    let app = probe_router(state);
    let (status, body) = common::send_json(&app, "GET", "/probe", None, Some("garbage")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["code"], "invalid_token");
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p gauge-server bearer`
Expected: FAIL — module not found.

- [ ] **Step 3: Implement**

`crates/gauge-server/src/middleware/mod.rs`:

```rust
pub mod bearer;
```

`crates/gauge-server/src/middleware/bearer.rs`:

```rust
use axum::extract::{Request, State};
use axum::http::header::AUTHORIZATION;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use gauge_auth::{Role, verify_token};

use crate::error::ApiError;
use crate::state::AppState;

#[derive(Debug, Clone)]
pub struct AuthContext {
    pub sub: String,
    pub role: Role,
    pub jti: String,
}

pub async fn require_bearer(State(st): State<AppState>, mut req: Request, next: Next) -> Response {
    let token = req
        .headers()
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));
    let Some(token) = token else {
        return ApiError::unauthorized("missing_token", "missing Authorization: Bearer header")
            .with_remediation("run `gauge login`")
            .into_response();
    };
    match verify_token(&st.secret, token) {
        Ok(claims) => {
            req.extensions_mut().insert(AuthContext {
                sub: claims.sub,
                role: claims.role,
                jti: claims.jti,
            });
            next.run(req).await
        }
        Err(_) => ApiError::unauthorized("invalid_token", "token is invalid or expired")
            .with_remediation("run `gauge login`")
            .into_response(),
    }
}
```

- [ ] **Step 4: Run to verify pass**

Run: `cargo test -p gauge-server`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/gauge-server
git commit -m "feat(server): bearer JWT middleware injecting AuthContext"
```

---

### Task 16: gauge-server — query SQL builder

**Files:**
- Create: `crates/gauge-server/src/sqlbuild.rs`
- Modify: `crates/gauge-server/src/lib.rs` (add `pub mod sqlbuild;`)

- [ ] **Step 1: Write the failing tests**

Tests module inside `crates/gauge-server/src/sqlbuild.rs` (unit-level — pure function, no DB):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use gauge_query::*;
    use time::macros::datetime;

    const NOW: time::OffsetDateTime = datetime!(2026-06-12 12:00:00 UTC);

    fn spec_example() -> QueryRequest {
        serde_json::from_str(
            r#"{
              "measures": ["unique_installs"],
              "dimensions": ["app", "event_name"],
              "filters": [{"field": "app", "op": "eq", "value": "tome"},
                          {"field": "attr.surface", "op": "eq", "value": "mcp"}],
              "time_range": {"last": "7d"},
              "granularity": "day",
              "order": [{"field": "unique_installs", "dir": "desc"}],
              "limit": 100
            }"#,
        )
        .unwrap()
    }

    #[test]
    fn snapshot_spec_example() {
        let built = build(&spec_example(), NOW).unwrap();
        insta::assert_snapshot!(built.sql);
        assert_eq!(built.limit, 100);
        assert_eq!(built.columns.len(), 4); // time_bucket, app, event_name, unique_installs
    }

    #[test]
    fn snapshot_minimal_count() {
        let req: QueryRequest = serde_json::from_str(
            r#"{"measures":["count"],"time_range":{"last":"24h"}}"#,
        ).unwrap();
        insta::assert_snapshot!(build(&req, NOW).unwrap().sql);
    }

    #[test]
    fn snapshot_in_and_exists_filters() {
        let req: QueryRequest = serde_json::from_str(
            r#"{"measures":["count"],"dimensions":["os"],
                "filters":[{"field":"event_name","op":"in","value":["tome.search","tome.install"]},
                           {"field":"attr.surface","op":"exists"}],
                "time_range":{"last":"30d"}}"#,
        ).unwrap();
        insta::assert_snapshot!(build(&req, NOW).unwrap().sql);
    }

    #[test]
    fn user_values_never_appear_in_sql_text() {
        let built = build(&spec_example(), NOW).unwrap();
        assert!(!built.sql.contains("tome"));
        assert!(!built.sql.contains("mcp"));
        // attr keys are bound too, not spliced
        assert!(!built.sql.contains("surface"));
    }

    #[test]
    fn order_must_reference_selected_alias() {
        let mut req = spec_example();
        req.order = vec![Order { field: "nope".into(), dir: Dir::Desc }];
        assert!(matches!(build(&req, NOW), Err(QueryError::BadOrderField(f)) if f == "nope"));
    }

    #[test]
    fn default_limit_and_truncation_headroom() {
        let req: QueryRequest =
            serde_json::from_str(r#"{"measures":["count"],"time_range":{"last":"1d"}}"#).unwrap();
        let built = build(&req, NOW).unwrap();
        assert_eq!(built.limit, 1000);
        assert!(built.sql.ends_with("LIMIT 1001"));
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p gauge-server sqlbuild`
Expected: FAIL — module not found.

- [ ] **Step 3: Implement**

`crates/gauge-server/src/sqlbuild.rs` (above tests):

```rust
//! Translates a validated QueryRequest into ONE parameterized SQL statement.
//! Identifiers come only from closed enums; every user-supplied value
//! (filter values, attr keys) is a bind parameter — never string-spliced.

use gauge_query::{
    DEFAULT_LIMIT, Dir, Field, FilterOp, FilterValue, MAX_LIMIT, Measure, QueryError,
    QueryRequest, resolve_time_range, validate,
};
use time::OffsetDateTime;

#[derive(Debug, Clone)]
pub enum Bind {
    Text(String),
    TextArr(Vec<String>),
    Time(OffsetDateTime),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ColKind {
    Text,
    Int,
    TimeBucket,
}

#[derive(Debug, Clone)]
pub struct ColSpec {
    pub alias: String,
    pub kind: ColKind,
}

#[derive(Debug)]
pub struct BuiltQuery {
    pub sql: String,
    pub binds: Vec<Bind>,
    pub columns: Vec<ColSpec>,
    pub limit: usize,
}

fn ph(binds: &mut Vec<Bind>, b: Bind) -> String {
    binds.push(b);
    format!("${}", binds.len())
}

fn field_expr(f: &Field, binds: &mut Vec<Bind>) -> String {
    match f {
        Field::App => "app".into(),
        Field::EventName => "event_name".into(),
        Field::AppVersion => "app_version".into(),
        Field::Os => "os".into(),
        Field::Arch => "arch".into(),
        Field::Attr(k) => {
            let p = ph(binds, Bind::Text(k.clone()));
            format!("(attributes->>{p})")
        }
    }
}

pub fn build(req: &QueryRequest, now: OffsetDateTime) -> Result<BuiltQuery, QueryError> {
    validate(req)?;
    let (from, to) = resolve_time_range(&req.time_range, now)?;

    let mut binds: Vec<Bind> = Vec::new();
    let mut select: Vec<String> = Vec::new();
    let mut columns: Vec<ColSpec> = Vec::new();
    let mut group_count = 0usize;

    let p_from = ph(&mut binds, Bind::Time(from));
    let p_to = ph(&mut binds, Bind::Time(to));
    let mut wheres = vec![format!("time >= {p_from} AND time < {p_to}")];

    if let Some(g) = req.granularity {
        select.push(format!(
            "date_trunc('{}', time) AS \"time_bucket\"",
            g.date_trunc_unit()
        ));
        columns.push(ColSpec { alias: "time_bucket".into(), kind: ColKind::TimeBucket });
        group_count += 1;
    }
    for d in &req.dimensions {
        // alias chars are restricted by Field::parse — safe inside quotes
        let alias = d.to_string();
        let expr = field_expr(d, &mut binds);
        select.push(format!("{expr} AS \"{alias}\""));
        columns.push(ColSpec { alias, kind: ColKind::Text });
        group_count += 1;
    }
    for m in &req.measures {
        let expr = match m {
            Measure::Count => "COUNT(*)",
            Measure::UniqueInstalls => "COUNT(DISTINCT install_id)",
            Measure::UniqueSessions => "COUNT(DISTINCT session_id)",
        };
        select.push(format!("{expr} AS \"{}\"", m.alias()));
        columns.push(ColSpec { alias: m.alias().into(), kind: ColKind::Int });
    }

    for f in &req.filters {
        let expr = field_expr(&f.field, &mut binds);
        match (f.op, f.value.as_ref()) {
            (FilterOp::Eq, Some(FilterValue::One(v))) => {
                let p = ph(&mut binds, Bind::Text(v.clone()));
                wheres.push(format!("{expr} = {p}"));
            }
            (FilterOp::Neq, Some(FilterValue::One(v))) => {
                let p = ph(&mut binds, Bind::Text(v.clone()));
                wheres.push(format!("{expr} <> {p}"));
            }
            (FilterOp::In, Some(FilterValue::Many(v))) => {
                let p = ph(&mut binds, Bind::TextArr(v.clone()));
                wheres.push(format!("{expr} = ANY({p})"));
            }
            (FilterOp::Exists, None) => {
                let Field::Attr(k) = &f.field else { unreachable!("validated") };
                let p = ph(&mut binds, Bind::Text(k.clone()));
                wheres.push(format!("attributes ? {p}"));
            }
            _ => unreachable!("rejected by validate()"),
        }
    }

    let mut sql = format!(
        "SELECT {} FROM events WHERE {}",
        select.join(", "),
        wheres.join(" AND ")
    );
    if group_count > 0 {
        let ordinals: Vec<String> = (1..=group_count).map(|i| i.to_string()).collect();
        sql.push_str(&format!(" GROUP BY {}", ordinals.join(", ")));
    }

    let aliases: Vec<&str> = columns.iter().map(|c| c.alias.as_str()).collect();
    let order_terms: Vec<String> = if req.order.is_empty() {
        if req.granularity.is_some() {
            vec!["\"time_bucket\" ASC".into()]
        } else {
            vec![]
        }
    } else {
        req.order
            .iter()
            .map(|o| {
                if !aliases.contains(&o.field.as_str()) {
                    return Err(QueryError::BadOrderField(o.field.clone()));
                }
                let dir = match o.dir { Dir::Asc => "ASC", Dir::Desc => "DESC" };
                Ok(format!("\"{}\" {dir}", o.field))
            })
            .collect::<Result<_, _>>()?
    };
    if !order_terms.is_empty() {
        sql.push_str(&format!(" ORDER BY {}", order_terms.join(", ")));
    }

    let limit = req.limit.unwrap_or(DEFAULT_LIMIT).min(MAX_LIMIT) as usize;
    sql.push_str(&format!(" LIMIT {}", limit + 1)); // +1 row to detect truncation
    Ok(BuiltQuery { sql, binds, columns, limit })
}
```

- [ ] **Step 4: Run, review snapshots, accept**

Run: `cargo test -p gauge-server sqlbuild`
Expected: FAIL first run (new snapshots pending). Review with `cargo insta review` (or inspect `crates/gauge-server/src/snapshots/*.snap`) — verify the SQL reads correctly (e.g. the spec example should be `SELECT date_trunc('day', time) AS "time_bucket", app AS "app", event_name AS "event_name", COUNT(DISTINCT install_id) AS "unique_installs" FROM events WHERE time >= $1 AND time < $2 AND app = $3 AND (attributes->>$4) = $5 GROUP BY 1, 2, 3 ORDER BY "unique_installs" DESC LIMIT 101`) — then accept and re-run.
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/gauge-server
git commit -m "feat(server): query DSL to parameterized SQL builder with snapshots"
```

---

### Task 17: gauge-server — POST /v1/query endpoint

**Files:**
- Create: `crates/gauge-server/src/routes/query.rs`, `crates/gauge-server/tests/query.rs`
- Modify: `crates/gauge-server/src/routes/mod.rs`, `crates/gauge-server/src/app.rs`

- [ ] **Step 1: Write the failing tests**

`crates/gauge-server/tests/query.rs`:

```rust
mod common;

use axum::http::StatusCode;
use gauge_auth::{Role, mint_token};
use gauge_events::profile::{ParsedEvent, ResourceInfo};
use gauge_server::app::build_router;
use gauge_server::db;
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

async fn seed(pool: &PgPool) {
    let now = OffsetDateTime::now_utc();
    // two installs for tome, one for midnight-manual
    for (app, install, n_search) in [("tome", Uuid::new_v4(), 3), ("tome", Uuid::new_v4(), 1), ("midnight-manual", Uuid::new_v4(), 2)] {
        let res = ResourceInfo {
            app: app.into(), app_version: "0.1.0".into(),
            install_id: install, session_id: Uuid::new_v4(),
            os: "darwin".into(), arch: "arm64".into(),
        };
        let mut events = Vec::new();
        for _ in 0..n_search {
            let mut attributes = serde_json::Map::new();
            attributes.insert("surface".into(), serde_json::json!("cli"));
            events.push(ParsedEvent { event_name: format!("{app}.search"), time: now, attributes });
        }
        db::insert_events(pool, &res, &events).await.unwrap();
    }
}

fn token(state: &gauge_server::state::AppState) -> String {
    mint_token(&state.secret, "alice", Role::Admin, OffsetDateTime::now_utc()).unwrap().0
}

#[sqlx::test(migrations = "../../migrations")]
async fn query_requires_auth(pool: PgPool) {
    let (state, _kp) = common::test_state(pool);
    let app = build_router(state);
    let body = serde_json::json!({"measures":["count"],"time_range":{"last":"1d"}});
    let (status, _) = common::send_json(&app, "POST", "/v1/query", Some(body), None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[sqlx::test(migrations = "../../migrations")]
async fn aggregates_counts_and_uniques(pool: PgPool) {
    seed(&pool).await;
    let (state, _kp) = common::test_state(pool);
    let t = token(&state);
    let app = build_router(state);
    let body = serde_json::json!({
        "measures": ["count", "unique_installs"],
        "dimensions": ["app"],
        "time_range": {"last": "1d"},
        "order": [{"field": "app", "dir": "asc"}]
    });
    let (status, resp) = common::send_json(&app, "POST", "/v1/query", Some(body), Some(&t)).await;
    assert_eq!(status, StatusCode::OK);
    let rows = resp["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["app"], "midnight-manual");
    assert_eq!(rows[0]["count"], 2);
    assert_eq!(rows[0]["unique_installs"], 1);
    assert_eq!(rows[1]["app"], "tome");
    assert_eq!(rows[1]["count"], 4);
    assert_eq!(rows[1]["unique_installs"], 2);
    assert_eq!(resp["truncated"], false);
}

#[sqlx::test(migrations = "../../migrations")]
async fn attr_filter_and_dimension_work(pool: PgPool) {
    seed(&pool).await;
    let (state, _kp) = common::test_state(pool);
    let t = token(&state);
    let app = build_router(state);
    let body = serde_json::json!({
        "measures": ["count"],
        "dimensions": ["attr.surface"],
        "filters": [{"field": "app", "op": "eq", "value": "tome"}],
        "time_range": {"last": "1d"}
    });
    let (status, resp) = common::send_json(&app, "POST", "/v1/query", Some(body), Some(&t)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(resp["rows"][0]["attr.surface"], "cli");
    assert_eq!(resp["rows"][0]["count"], 4);
}

#[sqlx::test(migrations = "../../migrations")]
async fn invalid_query_is_422_naming_the_field(pool: PgPool) {
    let (state, _kp) = common::test_state(pool);
    let t = token(&state);
    let app = build_router(state);
    let body = serde_json::json!({"measures":["count"],"dimensions":["install_id"],"time_range":{"last":"1d"}});
    let (status, resp) = common::send_json(&app, "POST", "/v1/query", Some(body), Some(&t)).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(resp["code"], "invalid_query");
    assert!(resp["message"].as_str().unwrap().contains("install_id"));
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p gauge-server query`
Expected: FAIL — 404s.

- [ ] **Step 3: Implement**

`crates/gauge-server/src/routes/query.rs`:

```rust
use axum::body::Bytes;
use axum::extract::State;
use axum::{Extension, Json};
use gauge_query::{QueryRequest, QueryResponse};
use serde_json::Value;
use sqlx::Row as _;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::error::ApiError;
use crate::middleware::bearer::AuthContext;
use crate::sqlbuild::{self, Bind, ColKind};
use crate::state::AppState;

fn db_unavailable(e: sqlx::Error) -> ApiError {
    tracing::error!(kind = %e.to_string(), "query db error");
    ApiError::service_unavailable("db_unavailable", "database error; retry later")
}

pub async fn query(
    State(st): State<AppState>,
    Extension(_ctx): Extension<AuthContext>,
    body: Bytes,
) -> Result<Json<QueryResponse>, ApiError> {
    let req: QueryRequest = serde_json::from_slice(&body)
        .map_err(|e| ApiError::unprocessable("invalid_query", format!("invalid query request: {e}")))?;
    let started = std::time::Instant::now();
    let built = sqlbuild::build(&req, OffsetDateTime::now_utc())
        .map_err(|e| ApiError::unprocessable("invalid_query", e.to_string()))?;

    let mut tx = st.pool.begin().await.map_err(db_unavailable)?;
    sqlx::query("SET TRANSACTION READ ONLY").execute(&mut *tx).await.map_err(db_unavailable)?;
    sqlx::query("SET LOCAL statement_timeout = '5s'").execute(&mut *tx).await.map_err(db_unavailable)?;

    let mut q = sqlx::query(&built.sql);
    for b in &built.binds {
        q = match b {
            Bind::Text(s) => q.bind(s),
            Bind::TextArr(v) => q.bind(v),
            Bind::Time(t) => q.bind(*t),
        };
    }
    let rows = q.fetch_all(&mut *tx).await.map_err(|e| {
        if e.to_string().contains("statement timeout") {
            ApiError::unprocessable("query_timeout", "query exceeded the 5s statement timeout")
                .with_remediation("narrow the time range or reduce dimensions")
        } else {
            db_unavailable(e)
        }
    })?;
    drop(tx); // read-only; rollback-on-drop is fine

    let truncated = rows.len() > built.limit;
    let mut out = Vec::with_capacity(rows.len().min(built.limit));
    for row in rows.iter().take(built.limit) {
        let mut obj = serde_json::Map::new();
        for col in &built.columns {
            let v = match col.kind {
                ColKind::Text => row
                    .try_get::<Option<String>, _>(col.alias.as_str())
                    .map(|o| o.map(Value::String).unwrap_or(Value::Null)),
                ColKind::Int => row
                    .try_get::<i64, _>(col.alias.as_str())
                    .map(|n| Value::Number(n.into())),
                ColKind::TimeBucket => row
                    .try_get::<OffsetDateTime, _>(col.alias.as_str())
                    .map(|t| Value::String(t.format(&Rfc3339).unwrap_or_default())),
            }
            .map_err(|_| ApiError::service_unavailable("row_decode", "failed to decode result row"))?;
            obj.insert(col.alias.clone(), v);
        }
        out.push(Value::Object(obj));
    }
    Ok(Json(QueryResponse {
        rows: out,
        truncated,
        elapsed_ms: started.elapsed().as_millis() as u64,
    }))
}
```

Add `pub mod query;` to `routes/mod.rs`. In `app.rs` add a protected sub-router:

```rust
    let protected = Router::new()
        .route("/v1/query", post(routes::query::query))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            crate::middleware::bearer::require_bearer,
        ));
    public.merge(ingest).merge(auth).merge(protected).with_state(state)
```

(`build_router` now needs `state` before the final `with_state`; take `state: AppState` and clone for the layer as shown.)

- [ ] **Step 4: Run to verify pass**

Run: `cargo test -p gauge-server`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/gauge-server
git commit -m "feat(server): authenticated /v1/query endpoint with read-only tx and timeout"
```

---

### Task 18: gauge-server — GET /v1/meta endpoint

**Files:**
- Create: `crates/gauge-server/src/routes/meta.rs`, `crates/gauge-server/tests/meta.rs`
- Modify: `crates/gauge-server/src/routes/mod.rs`, `crates/gauge-server/src/app.rs`

- [ ] **Step 1: Write the failing test**

`crates/gauge-server/tests/meta.rs`:

```rust
mod common;

use axum::http::StatusCode;
use gauge_auth::{Role, mint_token};
use gauge_events::profile::{ParsedEvent, ResourceInfo};
use gauge_server::app::build_router;
use gauge_server::db;
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

#[sqlx::test(migrations = "../../migrations")]
async fn meta_reports_apps_events_and_keys(pool: PgPool) {
    let res = ResourceInfo {
        app: "tome".into(), app_version: "0.1.0".into(),
        install_id: Uuid::new_v4(), session_id: Uuid::new_v4(),
        os: "linux".into(), arch: "amd64".into(),
    };
    let mut attributes = serde_json::Map::new();
    attributes.insert("surface".into(), serde_json::json!("cli"));
    attributes.insert("latency_bucket".into(), serde_json::json!("50-200ms"));
    let ev = ParsedEvent { event_name: "tome.search".into(), time: OffsetDateTime::now_utc(), attributes };
    db::insert_events(&pool, &res, &[ev]).await.unwrap();

    let (state, _kp) = common::test_state(pool);
    let t = mint_token(&state.secret, "alice", Role::Admin, OffsetDateTime::now_utc()).unwrap().0;
    let app = build_router(state);
    let (status, resp) = common::send_json(&app, "GET", "/v1/meta", None, Some(&t)).await;
    assert_eq!(status, StatusCode::OK);
    let apps = resp["apps"].as_array().unwrap();
    assert_eq!(apps.len(), 1);
    assert_eq!(apps[0]["app"], "tome");
    assert_eq!(apps[0]["event_names"], serde_json::json!(["tome.search"]));
    assert_eq!(apps[0]["attribute_keys"], serde_json::json!(["latency_bucket", "surface"]));
    assert_eq!(apps[0]["total_events"], 1);
    assert!(apps[0]["first_event"].is_string());
}

#[sqlx::test(migrations = "../../migrations")]
async fn meta_requires_auth(pool: PgPool) {
    let (state, _kp) = common::test_state(pool);
    let app = build_router(state);
    let (status, _) = common::send_json(&app, "GET", "/v1/meta", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p gauge-server meta`
Expected: FAIL — 404.

- [ ] **Step 3: Implement**

`crates/gauge-server/src/routes/meta.rs`:

```rust
use std::collections::BTreeMap;

use axum::extract::State;
use axum::{Extension, Json};
use gauge_query::{AppMeta, MetaResponse};
use sqlx::Row as _;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::error::ApiError;
use crate::middleware::bearer::AuthContext;
use crate::state::AppState;

fn db_err(e: sqlx::Error) -> ApiError {
    tracing::error!(kind = %e.to_string(), "meta db error");
    ApiError::service_unavailable("db_unavailable", "database error; retry later")
}

pub async fn meta(
    State(st): State<AppState>,
    Extension(_ctx): Extension<AuthContext>,
) -> Result<Json<MetaResponse>, ApiError> {
    let stats = sqlx::query(
        "SELECT app, COUNT(*) AS total, MIN(time) AS first, MAX(time) AS last FROM events GROUP BY app ORDER BY app",
    )
    .fetch_all(&st.pool)
    .await
    .map_err(db_err)?;
    let names = sqlx::query("SELECT DISTINCT app, event_name FROM events ORDER BY app, event_name")
        .fetch_all(&st.pool)
        .await
        .map_err(db_err)?;
    let keys = sqlx::query(
        "SELECT DISTINCT app, jsonb_object_keys(attributes) AS key FROM events ORDER BY 1, 2",
    )
    .fetch_all(&st.pool)
    .await
    .map_err(db_err)?;

    let mut apps: BTreeMap<String, AppMeta> = BTreeMap::new();
    for row in &stats {
        let app: String = row.get("app");
        let fmt = |t: Option<OffsetDateTime>| t.and_then(|t| t.format(&Rfc3339).ok());
        apps.insert(app.clone(), AppMeta {
            app,
            event_names: vec![],
            attribute_keys: vec![],
            first_event: fmt(row.get("first")),
            last_event: fmt(row.get("last")),
            total_events: row.get("total"),
        });
    }
    for row in &names {
        let app: String = row.get("app");
        if let Some(m) = apps.get_mut(&app) {
            m.event_names.push(row.get("event_name"));
        }
    }
    for row in &keys {
        let app: String = row.get("app");
        if let Some(m) = apps.get_mut(&app) {
            m.attribute_keys.push(row.get("key"));
        }
    }
    Ok(Json(MetaResponse { apps: apps.into_values().collect() }))
}
```

Add `pub mod meta;` to `routes/mod.rs`; add to the protected router in `app.rs`:

```rust
        .route("/v1/meta", get(routes::meta::meta))
```

- [ ] **Step 4: Run to verify pass**

Run: `cargo test -p gauge-server`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/gauge-server
git commit -m "feat(server): /v1/meta discovery endpoint"
```

---

### Task 19: gauge-server — per-IP/per-user rate limiting

**Files:**
- Create: `crates/gauge-server/src/middleware/rate_limit.rs`, `crates/gauge-server/tests/rate_limit.rs`
- Modify: `crates/gauge-server/src/middleware/mod.rs`, `src/state.rs`, `src/app.rs`, `tests/common/mod.rs`

- [ ] **Step 1: Write the failing tests**

Unit tests inside `rate_limit.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};
    use std::time::{Duration, Instant};

    const IP: IpAddr = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1));

    #[test]
    fn allows_burst_then_blocks() {
        let l = KeyedLimiter::new(60, 2); // 1/sec refill, burst 2
        let t0 = Instant::now();
        assert!(l.check(IP, t0).is_ok());
        assert!(l.check(IP, t0).is_ok());
        let retry = l.check(IP, t0).unwrap_err();
        assert!(retry >= 1);
    }

    #[test]
    fn refills_over_time() {
        let l = KeyedLimiter::new(60, 1);
        let t0 = Instant::now();
        assert!(l.check(IP, t0).is_ok());
        assert!(l.check(IP, t0).is_err());
        assert!(l.check(IP, t0 + Duration::from_secs(2)).is_ok());
    }

    #[test]
    fn keys_are_independent() {
        let l = KeyedLimiter::new(60, 1);
        let t0 = Instant::now();
        let other = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2));
        assert!(l.check(IP, t0).is_ok());
        assert!(l.check(other, t0).is_ok());
    }
}
```

`crates/gauge-server/tests/rate_limit.rs` (integration — uses a tight limiter):

```rust
mod common;

use axum::http::StatusCode;
use gauge_server::app::build_router;
use gauge_server::middleware::rate_limit::Limiters;
use sqlx::PgPool;
use std::sync::Arc;

#[sqlx::test]
async fn auth_endpoint_returns_429_with_retry_after(pool: PgPool) {
    let (mut state, _kp) = common::test_state(pool);
    state.limiters = Arc::new(Limiters::new(1000, 2, 1000)); // auth: burst 2
    let app = build_router(state);
    let body = serde_json::json!({"user_id": "alice"});
    for _ in 0..2 {
        let (status, _) = common::send_json(&app, "POST", "/v1/auth/challenge", Some(body.clone()), None).await;
        assert_eq!(status, StatusCode::OK);
    }
    let (status, resp) = common::send_json(&app, "POST", "/v1/auth/challenge", Some(body), None).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(resp["code"], "rate_limited");
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p gauge-server rate_limit`
Expected: FAIL — module not found.

- [ ] **Step 3: Implement**

`crates/gauge-server/src/middleware/rate_limit.rs` (above unit tests):

```rust
//! Hand-rolled keyed token buckets. IPs live ONLY here, in memory —
//! never on disk, never on event rows (spec privacy guarantee).

use std::collections::HashMap;
use std::hash::Hash;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::Mutex;
use std::time::Instant;

use axum::extract::{Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::middleware::bearer::AuthContext;
use crate::state::AppState;

pub struct KeyedLimiter<K: Eq + Hash> {
    capacity: f64,
    refill_per_sec: f64,
    buckets: Mutex<HashMap<K, (f64, Instant)>>,
}

impl<K: Eq + Hash> KeyedLimiter<K> {
    pub fn new(per_min: u32, burst: u32) -> Self {
        Self {
            capacity: burst as f64,
            refill_per_sec: per_min as f64 / 60.0,
            buckets: Mutex::new(HashMap::new()),
        }
    }

    /// Ok(()) consumes one token; Err(retry_after_secs) when exhausted.
    pub fn check(&self, key: K, now: Instant) -> Result<(), u64> {
        let mut map = self.buckets.lock().unwrap();
        let (tokens, last) = map.remove(&key).unwrap_or((self.capacity, now));
        let tokens = (tokens + now.duration_since(last).as_secs_f64() * self.refill_per_sec)
            .min(self.capacity);
        if tokens >= 1.0 {
            map.insert(key, (tokens - 1.0, now));
            Ok(())
        } else {
            let retry = ((1.0 - tokens) / self.refill_per_sec).ceil() as u64;
            map.insert(key, (tokens, now));
            Err(retry.max(1))
        }
    }
}

pub struct Limiters {
    pub logs: KeyedLimiter<IpAddr>,
    pub auth: KeyedLimiter<IpAddr>,
    pub user: KeyedLimiter<String>,
}

impl Limiters {
    /// burst = 2x for ingest (sender flushes are bursty), 1x elsewhere.
    pub fn new(logs_per_min: u32, auth_per_min: u32, user_per_min: u32) -> Self {
        Self {
            logs: KeyedLimiter::new(logs_per_min, logs_per_min * 2),
            auth: KeyedLimiter::new(auth_per_min, auth_per_min),
            user: KeyedLimiter::new(user_per_min, user_per_min),
        }
    }
}

/// Fly terminates TLS and sets Fly-Client-IP. Absent (local/tests) → loopback.
pub fn client_ip(headers: &HeaderMap) -> IpAddr {
    headers
        .get("Fly-Client-IP")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse().ok())
        .unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST))
}

fn too_many(retry_after: u64) -> Response {
    let body = serde_json::json!({
        "code": "rate_limited",
        "message": "rate limit exceeded",
        "remediation": format!("retry after {retry_after}s"),
    });
    (
        StatusCode::TOO_MANY_REQUESTS,
        [(header::RETRY_AFTER, retry_after.to_string())],
        axum::Json(body),
    )
        .into_response()
}

pub async fn limit_logs(State(st): State<AppState>, req: Request, next: Next) -> Response {
    match st.limiters.logs.check(client_ip(req.headers()), Instant::now()) {
        Ok(()) => next.run(req).await,
        Err(r) => too_many(r),
    }
}

pub async fn limit_auth(State(st): State<AppState>, req: Request, next: Next) -> Response {
    match st.limiters.auth.check(client_ip(req.headers()), Instant::now()) {
        Ok(()) => next.run(req).await,
        Err(r) => too_many(r),
    }
}

/// Must run AFTER require_bearer (reads AuthContext from extensions).
pub async fn limit_user(State(st): State<AppState>, req: Request, next: Next) -> Response {
    let sub = req
        .extensions()
        .get::<AuthContext>()
        .map(|c| c.sub.clone())
        .unwrap_or_default();
    match st.limiters.user.check(sub, Instant::now()) {
        Ok(()) => next.run(req).await,
        Err(r) => too_many(r),
    }
}
```

Modify `state.rs`: add field `pub limiters: Arc<Limiters>` (import `crate::middleware::rate_limit::Limiters`), and in `from_config`:

```rust
            limiters: Arc::new(Limiters::new(
                cfg.rate_logs_per_min,
                cfg.rate_auth_per_min,
                cfg.rate_user_per_min,
            )),
```

Modify `tests/common/mod.rs` `test_state`: add to the struct literal:

```rust
        limiters: Arc::new(gauge_server::middleware::rate_limit::Limiters::new(100_000, 100_000, 100_000)),
```

(huge defaults so other integration tests never trip the limiter). Add `pub mod rate_limit;` to `middleware/mod.rs`. Wire layers in `app.rs` — note layer order: the **last** `.layer()` added is outermost and runs first:

```rust
    let ingest = Router::new()
        .route("/v1/logs", post(routes::ingest::ingest))
        .layer(DefaultBodyLimit::max(gauge_events::profile::MAX_BODY_BYTES))
        .layer(axum::middleware::from_fn_with_state(state.clone(), crate::middleware::rate_limit::limit_logs));
    let auth = Router::new()
        .route("/v1/auth/challenge", post(routes::auth::challenge))
        .route("/v1/auth/verify", post(routes::auth::verify))
        .layer(axum::middleware::from_fn_with_state(state.clone(), crate::middleware::rate_limit::limit_auth));
    let protected = Router::new()
        .route("/v1/query", post(routes::query::query))
        .route("/v1/meta", get(routes::meta::meta))
        .layer(axum::middleware::from_fn_with_state(state.clone(), crate::middleware::rate_limit::limit_user))
        .layer(axum::middleware::from_fn_with_state(state.clone(), crate::middleware::bearer::require_bearer));
```

- [ ] **Step 4: Run to verify pass**

Run: `cargo test -p gauge-server`
Expected: PASS (all server tests, including earlier ones with the generous test limiter).

- [ ] **Step 5: Commit**

```bash
git add crates/gauge-server
git commit -m "feat(server): per-IP and per-user token-bucket rate limiting"
```

---

### Task 20: gauge-server — privacy canary tests

**Files:**
- Create: `crates/gauge-server/tests/privacy_canary.rs`

- [ ] **Step 1: Write the canary tests**

`crates/gauge-server/tests/privacy_canary.rs`:

```rust
mod common;

use std::io::Write;
use std::sync::{Arc, Mutex};

use gauge_server::app::build_router;
use sqlx::PgPool;
use tracing_subscriber::fmt::MakeWriter;

/// Canary 1: the events schema must contain exactly the spec's columns —
/// catching any accidental addition of IP/UA/identity columns in review.
#[sqlx::test(migrations = "../../migrations")]
async fn events_schema_has_exactly_the_spec_columns(pool: PgPool) {
    let cols: Vec<String> = sqlx::query_scalar(
        "SELECT column_name FROM information_schema.columns WHERE table_name = 'events' ORDER BY column_name",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        cols,
        vec![
            "app", "app_version", "arch", "attributes", "event_name", "id",
            "install_id", "os", "received_at", "session_id", "time",
        ],
        "events table columns drifted from the spec — privacy review required"
    );
}

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);

impl Write for Capture {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for Capture {
    type Writer = Capture;
    fn make_writer(&'a self) -> Capture {
        self.clone()
    }
}

/// Canary 2: nothing logged on the ingest path may contain attribute values.
/// (sqlx::test uses a current-thread runtime, so the thread-local default
/// subscriber covers the handler.)
#[sqlx::test(migrations = "../../migrations")]
async fn ingest_path_never_logs_attribute_values(pool: PgPool) {
    const CANARY: &str = "SECRET_CANARY_VALUE_DO_NOT_LOG";
    let capture = Capture::default();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(capture.clone())
        .with_max_level(tracing::Level::TRACE)
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);

    let (state, _kp) = common::test_state(pool);
    let app = build_router(state);
    let mut body: serde_json::Value =
        serde_json::from_str(include_str!("../../gauge-events/tests/fixtures/valid_batch.json")).unwrap();
    body["resourceLogs"][0]["scopeLogs"][0]["logRecords"][0]["attributes"][1]["value"]["stringValue"] =
        CANARY.into();
    let (status, _) = common::send_json(&app, "POST", "/v1/logs", Some(body), None).await;
    assert_eq!(status, axum::http::StatusCode::OK);

    let logs = String::from_utf8(capture.0.lock().unwrap().clone()).unwrap();
    assert!(
        !logs.contains(CANARY),
        "ingest path logged an attribute value:\n{logs}"
    );
}
```

Add to `crates/gauge-server/Cargo.toml` `[dev-dependencies]`: `tracing-subscriber.workspace = true` (already a main dependency — only needed if it was not; skip if present).

- [ ] **Step 2: Run to verify pass**

Run: `cargo test -p gauge-server privacy_canary`
Expected: PASS (2 tests). If canary 2 fails, fix the leaking log statement — never the test.

- [ ] **Step 3: Commit**

```bash
git add crates/gauge-server
git commit -m "test(server): privacy canaries for schema and ingest logging"
```

---

### Task 21: Deployment — Dockerfile, fly.toml, deploy runbook

**Files:**
- Create: `Dockerfile.server`, `fly.toml`, `docs/deploy.md`

- [ ] **Step 1: Write the artifacts**

`Dockerfile.server` (cargo-chef → distroless; migrations are compiled into the binary by `sqlx::migrate!`):

```dockerfile
FROM rust:1.93-bookworm AS chef
RUN cargo install cargo-chef --locked
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json -p gauge-server
COPY . .
RUN cargo build --release -p gauge-server

FROM gcr.io/distroless/cc-debian12
COPY --from=builder /app/target/release/gauge-server /usr/local/bin/gauge-server
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/gauge-server"]
```

`fly.toml`:

```toml
app = "gauge-telemetry"
primary_region = "lhr"

[build]
dockerfile = "Dockerfile.server"

[http_service]
internal_port = 8080
force_https = true
auto_stop_machines = "stop"
auto_start_machines = true
min_machines_running = 1

[[http_service.checks]]
interval = "30s"
timeout = "2s"
method = "GET"
path = "/healthz"

[[http_service.checks]]
interval = "10s"
timeout = "2s"
method = "GET"
path = "/readyz"

[[vm]]
size = "shared-cpu-1x"
memory = "2gb"
```

`docs/deploy.md`:

```markdown
# Deploying gauge-server to Fly.io

One-time setup:

​```bash
fly apps create gauge-telemetry
# Managed Postgres (verify current syntax with `fly mpg --help`):
fly mpg create --name gauge-pg --region lhr
fly mpg attach gauge-pg --app gauge-telemetry   # sets DATABASE_URL

# Secrets
fly secrets set --app gauge-telemetry \
  GAUGE_JWT_SECRET="$(openssl rand -base64 48)" \
  GAUGE_APP_ALLOWLIST="tome,midnight-manual" \
  GAUGE_USER_STORE="$(cat users.toml)"
​```

`users.toml` (never committed; lives in your password manager):

​```toml
schema_version = 1

[[users]]
user_id = "aaron"
role = "admin"
public_key = "ed25519:<output of `gauge keys generate --user-id aaron`>"
created_at = "2026-06-12"
​```

Deploy + verify:

​```bash
fly deploy
curl https://gauge-telemetry.fly.dev/healthz   # -> ok
curl https://gauge-telemetry.fly.dev/readyz    # -> ok
​```

Rotating GAUGE_JWT_SECRET invalidates all issued tokens (1h TTL anyway).
Adding a reader = add a [[users]] row, re-run `fly secrets set GAUGE_USER_STORE=...`.
```

(Strip the zero-width markers from the nested code fences when writing the real file.)

- [ ] **Step 2: Verify the image builds (requires Docker; skip in CI)**

Run: `docker build -f Dockerfile.server -t gauge-server:dev .`
Expected: image builds. If Docker is unavailable locally, mark this checked after the first successful `fly deploy`.

- [ ] **Step 3: Commit**

```bash
git add Dockerfile.server fly.toml docs/deploy.md
git commit -m "feat(deploy): Dockerfile, fly.toml, and deploy runbook"
```

---

## PHASE GATE 1 → 2

- [ ] Run `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace` — all green.
- [ ] (Optional but recommended) Execute `docs/deploy.md` once for real; confirm `/healthz`, a curl OTLP POST of the SPEC.md worked example, and a full auth handshake against the deployed app.
- [ ] Re-read Phases 2 and 3 below against everything learned in Phase 1 (actual crate versions resolved, axum/sqlx API drift, any renamed types or modules). Edit the affected task steps in this document so they match reality.
- [ ] Update the Plan changelog at the top of this file (gate passed; what was revised and why, or "unmodified").
- [ ] Commit: `git add docs/superpowers/plans && git commit -m "docs: phase 1 gate review of implementation plan"`

---

# PHASE 2 — Client (`gauge` binary)

> Phase 2 assumes Phase 1 is merged and (ideally) deployed. Re-validate this phase at Phase Gate 1 before starting.

### Task 22: gauge — CLI scaffold, paths, config, error type

**Files:**
- Create: `crates/gauge/src/{lib.rs,paths.rs,config.rs,error.rs}`
- Modify: `crates/gauge/src/main.rs`

(Structure gauge as lib + thin binary, same as gauge-server, so integration tests can use modules.)

- [ ] **Step 1: Write the failing tests**

`crates/gauge/tests/config.rs`:

```rust
use std::sync::{Mutex, OnceLock};

/// Env vars are process-global; serialize tests that touch GAUGE_CONFIG_DIR.
pub fn env_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(())).lock().unwrap()
}

#[test]
fn config_dir_honours_env_override() {
    let _g = env_lock();
    let tmp = tempfile::tempdir().unwrap();
    unsafe { std::env::set_var("GAUGE_CONFIG_DIR", tmp.path()) };
    assert_eq!(gauge::paths::config_dir().unwrap(), tmp.path());
    unsafe { std::env::remove_var("GAUGE_CONFIG_DIR") };
}

#[test]
fn config_loads_and_normalizes_server_url() {
    let _g = env_lock();
    let tmp = tempfile::tempdir().unwrap();
    unsafe { std::env::set_var("GAUGE_CONFIG_DIR", tmp.path()) };
    std::fs::write(
        tmp.path().join("config.toml"),
        "server_url = \"https://gauge-telemetry.fly.dev/\"\nuser_id = \"aaron\"\n",
    )
    .unwrap();
    let cfg = gauge::config::ClientConfig::load().unwrap();
    assert_eq!(cfg.server_url, "https://gauge-telemetry.fly.dev"); // trailing slash stripped
    assert_eq!(cfg.user_id, "aaron");
    unsafe { std::env::remove_var("GAUGE_CONFIG_DIR") };
}

#[test]
fn missing_config_names_the_path() {
    let _g = env_lock();
    let tmp = tempfile::tempdir().unwrap();
    unsafe { std::env::set_var("GAUGE_CONFIG_DIR", tmp.path()) };
    let err = gauge::config::ClientConfig::load().unwrap_err();
    assert!(err.to_string().contains("config.toml"));
    unsafe { std::env::remove_var("GAUGE_CONFIG_DIR") };
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p gauge`
Expected: FAIL — modules not found.

- [ ] **Step 3: Implement**

`crates/gauge/src/error.rs`:

```rust
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ClientError {
    #[error("could not determine a config directory (set GAUGE_CONFIG_DIR, XDG_CONFIG_HOME, or HOME)")]
    NoConfigDir,
    #[error("missing config file {0} — create it with server_url and user_id")]
    ConfigMissing(PathBuf),
    #[error("invalid config: {0}")]
    ConfigInvalid(String),
    #[error("no private key at {0} — run `gauge keys generate --user-id <id>`")]
    KeyMissing(PathBuf),
    #[error("refusing to overwrite existing key at {0}")]
    KeyExists(PathBuf),
    #[error("auth error: {0} — run `gauge login`")]
    Auth(#[from] gauge_auth::AuthError),
    #[error("http error: {0}")]
    Http(String),
    #[error("server error {status} ({code}): {message}{}", remediation.as_deref().map(|r| format!(" — {r}")).unwrap_or_default())]
    Api { status: u16, code: String, message: String, remediation: Option<String> },
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}
```

`crates/gauge/src/paths.rs`:

```rust
use std::path::PathBuf;

use crate::error::ClientError;

pub fn config_dir() -> Result<PathBuf, ClientError> {
    if let Ok(d) = std::env::var("GAUGE_CONFIG_DIR") {
        return Ok(PathBuf::from(d));
    }
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        return Ok(PathBuf::from(xdg).join("gauge"));
    }
    std::env::var("HOME")
        .map(|h| PathBuf::from(h).join(".config").join("gauge"))
        .map_err(|_| ClientError::NoConfigDir)
}

pub fn config_path() -> Result<PathBuf, ClientError> {
    Ok(config_dir()?.join("config.toml"))
}

pub fn key_path(user_id: &str) -> Result<PathBuf, ClientError> {
    Ok(config_dir()?.join(format!("{user_id}.private")))
}

pub fn token_path() -> Result<PathBuf, ClientError> {
    Ok(config_dir()?.join("token.json"))
}
```

`crates/gauge/src/config.rs`:

```rust
use crate::error::ClientError;
use crate::paths;

#[derive(Debug, Clone, serde::Deserialize)]
pub struct ClientConfig {
    pub server_url: String,
    pub user_id: String,
}

impl ClientConfig {
    pub fn load() -> Result<Self, ClientError> {
        let path = paths::config_path()?;
        let raw = std::fs::read_to_string(&path).map_err(|_| ClientError::ConfigMissing(path))?;
        let mut cfg: ClientConfig =
            toml::from_str(&raw).map_err(|e| ClientError::ConfigInvalid(e.to_string()))?;
        while cfg.server_url.ends_with('/') {
            cfg.server_url.pop();
        }
        Ok(cfg)
    }
}
```

`crates/gauge/src/lib.rs`:

```rust
pub mod config;
pub mod error;
pub mod paths;
```

`crates/gauge/src/main.rs` (subcommands stubbed; filled in by later tasks):

```rust
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "gauge", about = "Gauge telemetry dashboard, MCP server, and admin CLI")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Manage Ed25519 keys for API authentication
    Keys {
        #[command(subcommand)]
        cmd: KeysCmd,
    },
    /// Authenticate against the gauge server and cache a token
    Login,
    /// Run a one-shot query (JSON QueryRequest as the argument)
    Query { request: String },
    /// Launch the dashboard TUI
    Tui,
    /// MCP server commands
    Mcp {
        #[command(subcommand)]
        cmd: McpCmd,
    },
}

#[derive(Subcommand)]
enum KeysCmd {
    Generate {
        #[arg(long)]
        user_id: String,
    },
}

#[derive(Subcommand)]
enum McpCmd {
    /// Serve MCP over stdio
    Serve,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let result: Result<(), Box<dyn std::error::Error>> = match cli.cmd {
        Cmd::Keys { cmd: KeysCmd::Generate { user_id } } => todo_stub("keys generate", &user_id),
        Cmd::Login => todo_stub("login", ""),
        Cmd::Query { request } => todo_stub("query", &request),
        Cmd::Tui => todo_stub("tui", ""),
        Cmd::Mcp { cmd: McpCmd::Serve } => todo_stub("mcp serve", ""),
    };
    if let Err(e) = result {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn todo_stub(name: &str, _arg: &str) -> Result<(), Box<dyn std::error::Error>> {
    Err(format!("`gauge {name}` is not implemented yet (see implementation plan)").into())
}
```

(The `todo_stub` calls are replaced task-by-task below; by Task 30 none remain. This is scaffolding, not a placeholder left in shipped code — Phase Gate 2 verifies no stub survives.)

- [ ] **Step 4: Run to verify pass**

Run: `cargo test -p gauge`
Expected: PASS (3 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/gauge
git commit -m "feat(client): CLI scaffold with paths, config, and error types"
```

---

### Task 23: gauge — `keys generate`

**Files:**
- Create: `crates/gauge/src/keys.rs`, `crates/gauge/tests/keys.rs`
- Modify: `crates/gauge/src/lib.rs`, `crates/gauge/src/main.rs`

- [ ] **Step 1: Write the failing tests**

`crates/gauge/tests/keys.rs`:

```rust
// Integration tests are separate binaries; the env_lock helper is duplicated
// from tests/config.rs deliberately.
use std::sync::{Mutex, OnceLock};
fn env_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(())).lock().unwrap()
}

#[test]
fn generate_writes_0600_key_and_returns_wire_pubkey() {
    let _g = env_lock();
    let tmp = tempfile::tempdir().unwrap();
    unsafe { std::env::set_var("GAUGE_CONFIG_DIR", tmp.path()) };

    let wire = gauge::keys::generate("alice").unwrap();
    assert!(wire.starts_with("ed25519:"));
    gauge_auth::wire::parse_public_key_wire(&wire).unwrap();

    let path = tmp.path().join("alice.private");
    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);

    // load_keypair restores the same public key
    let kp = gauge::keys::load_keypair("alice").unwrap();
    assert_eq!(kp.public_wire(), wire);

    // refuses overwrite
    assert!(matches!(gauge::keys::generate("alice"), Err(gauge::error::ClientError::KeyExists(_))));
    unsafe { std::env::remove_var("GAUGE_CONFIG_DIR") };
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p gauge keys`
Expected: FAIL — module not found.

- [ ] **Step 3: Implement**

`crates/gauge/src/keys.rs`:

```rust
use std::io::Write as _;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD_NO_PAD;
use gauge_auth::wire::b64_decode_flexible;
use gauge_auth::{AuthError, Keypair};

use crate::error::ClientError;
use crate::paths;

/// Generates a keypair, stores the seed (base64, mode 0600), returns the
/// public key wire form for registration in the server's users.toml.
pub fn generate(user_id: &str) -> Result<String, ClientError> {
    let dir = paths::config_dir()?;
    std::fs::create_dir_all(&dir)?;
    let path = paths::key_path(user_id)?;
    if path.exists() {
        return Err(ClientError::KeyExists(path));
    }
    let kp = Keypair::generate();
    let seed_b64 = STANDARD_NO_PAD.encode(kp.seed());
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        opts.mode(0o600);
    }
    let mut f = opts.open(&path)?;
    f.write_all(seed_b64.as_bytes())?;
    Ok(kp.public_wire())
}

pub fn load_keypair(user_id: &str) -> Result<Keypair, ClientError> {
    let path = paths::key_path(user_id)?;
    let b64 = std::fs::read_to_string(&path).map_err(|_| ClientError::KeyMissing(path))?;
    let bytes = b64_decode_flexible(b64.trim())?;
    let seed: [u8; 32] = bytes.try_into().map_err(|_| ClientError::Auth(AuthError::InvalidLength))?;
    Ok(Keypair::from_seed(&seed))
}
```

Add `pub mod keys;` to `lib.rs`. In `main.rs` replace the keys stub:

```rust
        Cmd::Keys { cmd: KeysCmd::Generate { user_id } } => {
            gauge::keys::generate(&user_id).map(|wire| {
                println!("Public key (register this in the server's users.toml):\n");
                println!("[[users]]");
                println!("user_id = \"{user_id}\"");
                println!("role = \"viewer\"");
                println!("public_key = \"{wire}\"");
            }).map_err(Into::into)
        }
```

- [ ] **Step 4: Run to verify pass**

Run: `cargo test -p gauge`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/gauge
git commit -m "feat(client): keys generate command with 0600 seed storage"
```

---

### Task 24: gauge — ApiClient (login flow, token cache, 401 retry)

**Files:**
- Create: `crates/gauge/src/api.rs`, `crates/gauge/tests/api.rs`
- Modify: `crates/gauge/src/lib.rs`, `crates/gauge/src/main.rs`

- [ ] **Step 1: Write the failing tests**

`crates/gauge/tests/api.rs`:

```rust
use std::sync::{Mutex, OnceLock};

use gauge::api::ApiClient;
use gauge::config::ClientConfig;
use gauge_auth::{Keypair, verify_signature};
use wiremock::matchers::{body_partial_json, method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

fn env_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(())).lock().unwrap()
}

const NONCE: [u8; 32] = [9u8; 32];

async fn mock_auth(server: &MockServer) {
    use base64::Engine as _;
    let nonce_b64 = base64::engine::general_purpose::STANDARD_NO_PAD.encode(NONCE);
    Mock::given(method("POST")).and(path("/v1/auth/challenge"))
        .and(body_partial_json(serde_json::json!({"user_id": "alice"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "challenge_id": "00000000-0000-4000-8000-000000000001",
            "nonce_b64": nonce_b64,
            "expires_in_s": 60
        })))
        .mount(server).await;
    Mock::given(method("POST")).and(path("/v1/auth/verify"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "token": "test-token",
            "user_id": "alice",
            "expires_at": time::OffsetDateTime::now_utc().unix_timestamp() + 3600
        })))
        .mount(server).await;
}

fn setup(tmp: &tempfile::TempDir, server_url: &str) -> ApiClient {
    unsafe { std::env::set_var("GAUGE_CONFIG_DIR", tmp.path()) };
    gauge::keys::generate("alice").unwrap();
    ApiClient::from_config(&ClientConfig { server_url: server_url.trim_end_matches('/').into(), user_id: "alice".into() })
}

#[tokio::test]
async fn login_signs_nonce_and_caches_token() {
    let _g = env_lock();
    let tmp = tempfile::tempdir().unwrap();
    let server = MockServer::start().await;
    mock_auth(&server).await;
    let api = setup(&tmp, &server.uri());

    let cache = api.login().await.unwrap();
    assert_eq!(cache.token, "test-token");
    assert!(tmp.path().join("token.json").exists());

    // the signature sent to /verify must verify against our key + the nonce
    let reqs: Vec<Request> = server.received_requests().await.unwrap();
    let verify_body: serde_json::Value =
        serde_json::from_slice(&reqs.iter().find(|r| r.url.path() == "/v1/auth/verify").unwrap().body).unwrap();
    let sig = gauge_auth::wire::b64_decode_flexible(verify_body["signature_b64"].as_str().unwrap()).unwrap();
    let kp: Keypair = gauge::keys::load_keypair("alice").unwrap();
    assert!(verify_signature(&kp.verifying_key(), &NONCE, &sig).is_ok());
    unsafe { std::env::remove_var("GAUGE_CONFIG_DIR") };
}

#[tokio::test]
async fn query_reauths_once_on_401() {
    let _g = env_lock();
    let tmp = tempfile::tempdir().unwrap();
    let server = MockServer::start().await;
    mock_auth(&server).await;
    // first /v1/query call → 401; subsequent → 200
    Mock::given(method("POST")).and(path("/v1/query"))
        .respond_with(ResponseTemplate::new(401).set_body_json(serde_json::json!({
            "code": "invalid_token", "message": "expired", "remediation": "run `gauge login`"
        })))
        .up_to_n_times(1)
        .mount(&server).await;
    Mock::given(method("POST")).and(path("/v1/query"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "rows": [{"count": 5}], "truncated": false, "elapsed_ms": 3
        })))
        .mount(&server).await;

    let api = setup(&tmp, &server.uri());
    let req: gauge_query::QueryRequest =
        serde_json::from_str(r#"{"measures":["count"],"time_range":{"last":"1d"}}"#).unwrap();
    let resp = api.query(&req).await.unwrap();
    assert_eq!(resp.rows[0]["count"], 5);
    unsafe { std::env::remove_var("GAUGE_CONFIG_DIR") };
}

#[tokio::test]
async fn api_error_envelope_is_surfaced() {
    let _g = env_lock();
    let tmp = tempfile::tempdir().unwrap();
    let server = MockServer::start().await;
    mock_auth(&server).await;
    Mock::given(method("POST")).and(path("/v1/query"))
        .respond_with(ResponseTemplate::new(422).set_body_json(serde_json::json!({
            "code": "invalid_query", "message": "unknown field `nope`", "remediation": null
        })))
        .mount(&server).await;
    let api = setup(&tmp, &server.uri());
    let req: gauge_query::QueryRequest =
        serde_json::from_str(r#"{"measures":["count"],"time_range":{"last":"1d"}}"#).unwrap();
    let err = api.query(&req).await.unwrap_err();
    assert!(err.to_string().contains("invalid_query"));
    assert!(err.to_string().contains("unknown field"));
    unsafe { std::env::remove_var("GAUGE_CONFIG_DIR") };
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p gauge api`
Expected: FAIL — module not found.

- [ ] **Step 3: Implement**

`crates/gauge/src/api.rs`:

```rust
use gauge_auth::protocol::{ChallengeRequest, ChallengeResponse, VerifyRequest, VerifyResponse};
use gauge_auth::sign_challenge;
use gauge_query::{MetaResponse, QueryRequest, QueryResponse};
use serde::de::DeserializeOwned;

use crate::config::ClientConfig;
use crate::error::ClientError;
use crate::{keys, paths};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TokenCache {
    pub token: String,
    pub user_id: String,
    pub expires_at: i64,
}

impl TokenCache {
    fn save(&self) -> Result<(), ClientError> {
        let path = paths::token_path()?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&path, serde_json::to_vec(self)?)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        }
        Ok(())
    }

    fn load() -> Option<TokenCache> {
        let path = paths::token_path().ok()?;
        serde_json::from_slice(&std::fs::read(path).ok()?).ok()
    }
}

pub struct ApiClient {
    http: reqwest::Client,
    base: String,
    user_id: String,
}

impl ApiClient {
    pub fn from_config(cfg: &ClientConfig) -> Self {
        Self {
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .build()
                .expect("reqwest client"),
            base: cfg.server_url.clone(),
            user_id: cfg.user_id.clone(),
        }
    }

    /// Full challenge/response using the local private key; caches the JWT.
    pub async fn login(&self) -> Result<TokenCache, ClientError> {
        let kp = keys::load_keypair(&self.user_id)?;
        let ch: ChallengeResponse = self
            .post_unauthed("/v1/auth/challenge", &ChallengeRequest { user_id: self.user_id.clone() })
            .await?;
        let signature_b64 = sign_challenge(&kp, &ch.nonce_b64)?;
        let v: VerifyResponse = self
            .post_unauthed("/v1/auth/verify", &VerifyRequest { challenge_id: ch.challenge_id, signature_b64 })
            .await?;
        let cache = TokenCache { token: v.token, user_id: v.user_id, expires_at: v.expires_at };
        cache.save()?;
        Ok(cache)
    }

    pub async fn query(&self, req: &QueryRequest) -> Result<QueryResponse, ClientError> {
        self.authed(reqwest::Method::POST, "/v1/query", Some(serde_json::to_value(req)?)).await
    }

    pub async fn meta(&self) -> Result<MetaResponse, ClientError> {
        self.authed(reqwest::Method::GET, "/v1/meta", None).await
    }

    async fn token(&self) -> Result<String, ClientError> {
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        if let Some(c) = TokenCache::load()
            && c.user_id == self.user_id
            && c.expires_at > now + 60
        {
            return Ok(c.token);
        }
        Ok(self.login().await?.token)
    }

    async fn authed<T: DeserializeOwned>(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<serde_json::Value>,
    ) -> Result<T, ClientError> {
        let mut token = self.token().await?;
        for attempt in 0..2 {
            let mut req = self
                .http
                .request(method.clone(), format!("{}{path}", self.base))
                .bearer_auth(&token);
            if let Some(b) = &body {
                req = req.json(b);
            }
            let resp = req.send().await.map_err(|e| ClientError::Http(e.to_string()))?;
            if resp.status().as_u16() == 401 && attempt == 0 {
                token = self.login().await?.token; // expired mid-session: transparent re-auth
                continue;
            }
            return Self::handle(resp).await;
        }
        unreachable!("loop always returns by attempt 1")
    }

    async fn post_unauthed<T: DeserializeOwned, B: serde::Serialize>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T, ClientError> {
        let resp = self
            .http
            .post(format!("{}{path}", self.base))
            .json(body)
            .send()
            .await
            .map_err(|e| ClientError::Http(e.to_string()))?;
        Self::handle(resp).await
    }

    async fn handle<T: DeserializeOwned>(resp: reqwest::Response) -> Result<T, ClientError> {
        let status = resp.status().as_u16();
        let bytes = resp.bytes().await.map_err(|e| ClientError::Http(e.to_string()))?;
        if (200..300).contains(&status) {
            return Ok(serde_json::from_slice(&bytes)?);
        }
        #[derive(serde::Deserialize)]
        struct Envelope {
            code: String,
            message: String,
            remediation: Option<String>,
        }
        let env: Envelope = serde_json::from_slice(&bytes).unwrap_or(Envelope {
            code: "unknown".into(),
            message: format!("HTTP {status}"),
            remediation: None,
        });
        Err(ClientError::Api { status, code: env.code, message: env.message, remediation: env.remediation })
    }
}
```

Add `pub mod api;` to `lib.rs`. In `main.rs` replace the login stub:

```rust
        Cmd::Login => {
            let cfg = gauge::config::ClientConfig::load()?;
            let api = gauge::api::ApiClient::from_config(&cfg);
            let cache = api.login().await?;
            println!("logged in as {} (token expires at unix {})", cache.user_id, cache.expires_at);
            Ok(())
        }
```

(`main` is already `async`, so the arm awaits inline; rework the `match` arms that were sync stubs into awaited expressions as they get filled in.)

- [ ] **Step 4: Run to verify pass**

Run: `cargo test -p gauge`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/gauge
git commit -m "feat(client): ApiClient with challenge/response login and 401 re-auth"
```

---

### Task 25: gauge — `query` one-shot command

**Files:**
- Create: `crates/gauge/src/query_cmd.rs`
- Modify: `crates/gauge/src/lib.rs`, `crates/gauge/src/main.rs`

- [ ] **Step 1: Write the failing test**

Tests module in `crates/gauge/src/query_cmd.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_invalid_request_json_with_helpful_error() {
        let err = parse_request(r#"{"measures":["count"]}"#).unwrap_err();
        assert!(err.to_string().contains("time_range"));
        let err = parse_request("not json").unwrap_err();
        assert!(err.to_string().contains("expected"));
    }

    #[test]
    fn accepts_valid_request() {
        parse_request(r#"{"measures":["count"],"time_range":{"last":"1d"}}"#).unwrap();
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p gauge query_cmd`
Expected: FAIL.

- [ ] **Step 3: Implement**

`crates/gauge/src/query_cmd.rs` (above tests):

```rust
use gauge_query::QueryRequest;

use crate::api::ApiClient;
use crate::error::ClientError;

pub fn parse_request(json: &str) -> Result<QueryRequest, ClientError> {
    Ok(serde_json::from_str(json)?)
}

pub async fn run(api: &ApiClient, request_json: &str) -> Result<String, ClientError> {
    let req = parse_request(request_json)?;
    let resp = api.query(&req).await?;
    Ok(serde_json::to_string_pretty(&resp)?)
}
```

Wire into `main.rs` (replace stub): load config, build ApiClient, `println!("{}", gauge::query_cmd::run(&api, &request).await?)`.

- [ ] **Step 4: Run to verify pass**

Run: `cargo test -p gauge`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/gauge
git commit -m "feat(client): one-shot query command"
```

---

### Task 26: gauge — MCP tool query builders (pure)

**Files:**
- Create: `crates/gauge/src/mcp/mod.rs`, `crates/gauge/src/mcp/tools.rs`
- Modify: `crates/gauge/src/lib.rs`

- [ ] **Step 1: Write the failing tests**

Tests module in `crates/gauge/src/mcp/tools.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unique_users_builds_expected_query() {
        let q = unique_users_query(&UniqueUsersParams {
            period: "7d".into(),
            app: Some("tome".into()),
            event_name: Some("tome.search".into()),
        });
        let json = serde_json::to_value(&q).unwrap();
        assert_eq!(json["measures"], serde_json::json!(["unique_installs"]));
        assert_eq!(json["time_range"], serde_json::json!({"last": "7d"}));
        assert_eq!(json["filters"].as_array().unwrap().len(), 2);
        gauge_query::validate(&q).unwrap();
    }

    #[test]
    fn top_events_defaults_and_orders_desc() {
        let q = top_events_query(&TopEventsParams { period: "30d".into(), app: None, by: None, limit: None });
        let json = serde_json::to_value(&q).unwrap();
        assert_eq!(json["dimensions"], serde_json::json!(["event_name"]));
        assert_eq!(json["order"], serde_json::json!([{"field": "count", "dir": "desc"}]));
        assert_eq!(json["limit"], 10);
        gauge_query::validate(&q).unwrap();
    }

    #[test]
    fn events_over_time_sets_granularity() {
        let q = events_over_time_query(&EventsOverTimeParams {
            period: "7d".into(),
            granularity: gauge_query::Granularity::Day,
            app: Some("midnight-manual".into()),
            event_name: None,
        });
        let json = serde_json::to_value(&q).unwrap();
        assert_eq!(json["granularity"], "day");
        gauge_query::validate(&q).unwrap();
    }

    #[test]
    fn tool_param_schemas_generate_and_describe_fields() {
        // Guards the MCP tool surface: schemars must produce schemas agents can read.
        let schema = serde_json::to_value(schemars::schema_for!(UniqueUsersParams)).unwrap();
        assert!(schema["properties"]["period"].is_object());
        let schema = serde_json::to_value(schemars::schema_for!(gauge_query::QueryRequest)).unwrap();
        let props = schema["properties"].as_object().unwrap();
        for key in ["measures", "dimensions", "filters", "time_range", "granularity", "order", "limit"] {
            assert!(props.contains_key(key), "QueryRequest schema missing `{key}`");
        }
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p gauge tools`
Expected: FAIL.

- [ ] **Step 3: Implement**

`crates/gauge/src/mcp/tools.rs` (above tests):

```rust
//! Pure parameter→QueryRequest builders for the MCP convenience tools.
//! Separated from rmcp glue so they unit-test without a server.

use gauge_query::{
    Dir, Field, Filter, FilterOp, FilterValue, Granularity, Measure, Order, QueryRequest, TimeRange,
};
use schemars::JsonSchema;
use serde::Deserialize;

fn eq_filter(field: Field, value: &str) -> Filter {
    Filter { field, op: FilterOp::Eq, value: Some(FilterValue::One(value.to_string())) }
}

fn base_filters(app: &Option<String>, event_name: &Option<String>) -> Vec<Filter> {
    let mut f = Vec::new();
    if let Some(a) = app {
        f.push(eq_filter(Field::App, a));
    }
    if let Some(e) = event_name {
        f.push(eq_filter(Field::EventName, e));
    }
    f
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct UniqueUsersParams {
    /// Relative period like "24h", "7d", "30d"
    pub period: String,
    pub app: Option<String>,
    pub event_name: Option<String>,
}

pub fn unique_users_query(p: &UniqueUsersParams) -> QueryRequest {
    QueryRequest {
        measures: vec![Measure::UniqueInstalls],
        dimensions: vec![],
        filters: base_filters(&p.app, &p.event_name),
        time_range: TimeRange::Last { last: p.period.clone() },
        granularity: None,
        order: vec![],
        limit: None,
    }
}

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TopBy {
    Count,
    UniqueInstalls,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct TopEventsParams {
    pub period: String,
    pub app: Option<String>,
    /// Rank by total count (default) or by unique installs
    pub by: Option<TopBy>,
    pub limit: Option<u32>,
}

pub fn top_events_query(p: &TopEventsParams) -> QueryRequest {
    let measure = match p.by.unwrap_or(TopBy::Count) {
        TopBy::Count => Measure::Count,
        TopBy::UniqueInstalls => Measure::UniqueInstalls,
    };
    QueryRequest {
        measures: vec![measure],
        dimensions: vec![Field::EventName],
        filters: base_filters(&p.app, &None),
        time_range: TimeRange::Last { last: p.period.clone() },
        granularity: None,
        order: vec![Order { field: measure.alias().into(), dir: Dir::Desc }],
        limit: Some(p.limit.unwrap_or(10)),
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct EventsOverTimeParams {
    pub period: String,
    pub granularity: Granularity,
    pub app: Option<String>,
    pub event_name: Option<String>,
}

pub fn events_over_time_query(p: &EventsOverTimeParams) -> QueryRequest {
    QueryRequest {
        measures: vec![Measure::Count],
        dimensions: vec![],
        filters: base_filters(&p.app, &p.event_name),
        time_range: TimeRange::Last { last: p.period.clone() },
        granularity: Some(p.granularity),
        order: vec![],
        limit: None,
    }
}
```

`crates/gauge/src/mcp/mod.rs`: `pub mod tools;` — and add `pub mod mcp;` to `lib.rs`.

- [ ] **Step 4: Run to verify pass**

Run: `cargo test -p gauge`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/gauge
git commit -m "feat(client): MCP convenience tool query builders"
```

---

### Task 27: gauge — MCP server (rmcp, stdio)

**Files:**
- Create: `crates/gauge/src/mcp/server.rs`
- Modify: `crates/gauge/src/mcp/mod.rs`, `crates/gauge/src/main.rs`, `crates/gauge/Cargo.toml`

**rmcp drift warning:** rmcp's macro API has moved between minor versions. First run `cargo add rmcp --features server,transport-io -p gauge`, note the resolved version, and check docs.rs for the current `#[tool_router]`/`#[tool]`/`Parameters` idiom. The code below targets the 0.x idiom current at plan time; adapt mechanically if names moved, and record the drift at Phase Gate 2.

- [ ] **Step 1: Add the dependency**

In `crates/gauge/Cargo.toml` add `rmcp.workspace = true`.

- [ ] **Step 2: Implement (test follows — rmcp servers are easiest to verify through their handler methods)**

`crates/gauge/src/mcp/server.rs`:

```rust
use std::sync::Arc;

use rmcp::handler::server::tool::{Parameters, ToolRouter};
use rmcp::model::{CallToolResult, Content, ServerCapabilities, ServerInfo};
use rmcp::{ErrorData as McpError, ServerHandler, ServiceExt, tool, tool_handler, tool_router};

use crate::api::ApiClient;
use crate::error::ClientError;
use crate::mcp::tools::{
    EventsOverTimeParams, TopEventsParams, UniqueUsersParams, events_over_time_query,
    top_events_query, unique_users_query,
};

#[derive(Clone)]
pub struct GaugeMcp {
    api: Arc<ApiClient>,
    tool_router: ToolRouter<Self>,
}

fn to_mcp_err(e: ClientError) -> McpError {
    // ClientError Display already carries remediation ("run `gauge login`" etc.)
    McpError::internal_error(e.to_string(), None)
}

fn ok_json<T: serde::Serialize>(v: &T) -> Result<CallToolResult, McpError> {
    let json = serde_json::to_string_pretty(v).map_err(|e| McpError::internal_error(e.to_string(), None))?;
    Ok(CallToolResult::success(vec![Content::text(json)]))
}

#[tool_router]
impl GaugeMcp {
    pub fn new(api: Arc<ApiClient>) -> Self {
        Self { api, tool_router: Self::tool_router() }
    }

    #[tool(description = "Run an analytics query over anonymous telemetry events. Measures: count, unique_installs, unique_sessions. Dimensions: app, event_name, app_version, os, arch, attr.<key>. Time ranges: {\"last\":\"7d\"} or RFC3339 from/to. Use get_meta first to discover apps, event names, and attribute keys.")]
    pub async fn query_telemetry(
        &self,
        Parameters(req): Parameters<gauge_query::QueryRequest>,
    ) -> Result<CallToolResult, McpError> {
        ok_json(&self.api.query(&req).await.map_err(to_mcp_err)?)
    }

    #[tool(description = "Discover what is queryable: apps, their event names, attribute keys, totals, and time span.")]
    pub async fn get_meta(&self) -> Result<CallToolResult, McpError> {
        ok_json(&self.api.meta().await.map_err(to_mcp_err)?)
    }

    #[tool(description = "How many unique users (anonymous installs) in a period, optionally filtered by app and/or event name. Example: unique users who ran a search in the last week.")]
    pub async fn unique_users(
        &self,
        Parameters(p): Parameters<UniqueUsersParams>,
    ) -> Result<CallToolResult, McpError> {
        ok_json(&self.api.query(&unique_users_query(&p)).await.map_err(to_mcp_err)?)
    }

    #[tool(description = "The most used events (top-N event types) in a period, ranked by count or unique installs. Answers 'what is our most used X'.")]
    pub async fn top_events(
        &self,
        Parameters(p): Parameters<TopEventsParams>,
    ) -> Result<CallToolResult, McpError> {
        ok_json(&self.api.query(&top_events_query(&p)).await.map_err(to_mcp_err)?)
    }

    #[tool(description = "Event volume over time (hour/day/week buckets) for trend questions.")]
    pub async fn events_over_time(
        &self,
        Parameters(p): Parameters<EventsOverTimeParams>,
    ) -> Result<CallToolResult, McpError> {
        ok_json(&self.api.query(&events_over_time_query(&p)).await.map_err(to_mcp_err)?)
    }
}

#[tool_handler]
impl ServerHandler for GaugeMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            instructions: Some(
                "Query anonymous product telemetry for Midnight/DevRel apps (Tome, Midnight Manual). \
                 Start with get_meta to see what exists. Telemetry is anonymous: there is no way to \
                 query individual users — only aggregate counts and unique-install counts."
                    .into(),
            ),
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            ..Default::default()
        }
    }
}

pub async fn serve(api: Arc<ApiClient>) -> Result<(), Box<dyn std::error::Error>> {
    let service = GaugeMcp::new(api).serve(rmcp::transport::stdio()).await?;
    service.waiting().await?;
    Ok(())
}
```

Add `pub mod server;` to `mcp/mod.rs`. Wire `Cmd::Mcp { cmd: McpCmd::Serve }` in `main.rs`:

```rust
        Cmd::Mcp { cmd: McpCmd::Serve } => {
            let cfg = gauge::config::ClientConfig::load()?;
            let api = std::sync::Arc::new(gauge::api::ApiClient::from_config(&cfg));
            gauge::mcp::server::serve(api).await
        }
```

- [ ] **Step 3: Write the verification test**

Append to `crates/gauge/tests/api.rs` (reuses the wiremock setup):

```rust
#[tokio::test]
async fn mcp_tools_call_through_to_api() {
    let _g = env_lock();
    let tmp = tempfile::tempdir().unwrap();
    let server = MockServer::start().await;
    mock_auth(&server).await;
    Mock::given(method("POST")).and(path("/v1/query"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "rows": [{"unique_installs": 42}], "truncated": false, "elapsed_ms": 2
        })))
        .mount(&server).await;
    let api = std::sync::Arc::new(setup(&tmp, &server.uri()));
    let mcp = gauge::mcp::server::GaugeMcp::new(api);
    let result = mcp
        .unique_users(rmcp::handler::server::tool::Parameters(gauge::mcp::tools::UniqueUsersParams {
            period: "7d".into(), app: None, event_name: None,
        }))
        .await
        .unwrap();
    let text = format!("{result:?}");
    assert!(text.contains("42"));
    unsafe { std::env::remove_var("GAUGE_CONFIG_DIR") };
}
```

- [ ] **Step 4: Run to verify pass + smoke the stdio server**

Run: `cargo test -p gauge`
Expected: PASS.

Manual smoke (requires a deployed/local server and a logged-in config): `echo '{"jsonrpc":"2.0","id":1,"method":"tools/list"}' | cargo run -p gauge -- mcp serve` — expect a JSON-RPC response listing 5 tools.

- [ ] **Step 5: Commit**

```bash
git add crates/gauge
git commit -m "feat(client): MCP server over stdio with five query tools"
```

---

### Task 28: gauge — TUI data layer

**Files:**
- Create: `crates/gauge/src/tui/mod.rs`, `crates/gauge/src/tui/data.rs`, `crates/gauge/tests/tui_data.rs`
- Modify: `crates/gauge/src/lib.rs`, `crates/gauge/Cargo.toml`

Add to `crates/gauge/Cargo.toml`: `ratatui.workspace = true`, `crossterm = { workspace = true, features = ["event-stream"] }`, `futures = "0.3"` (add `futures = "0.3"` to `[workspace.dependencies]` too).

- [ ] **Step 1: Write the failing test**

`crates/gauge/tests/tui_data.rs`:

```rust
use std::sync::{Mutex, OnceLock};

use gauge::tui::data::{TimeWindow, fetch};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn env_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(())).lock().unwrap()
}

#[tokio::test]
async fn fetch_assembles_snapshot_from_query_and_meta() {
    let _g = env_lock();
    let tmp = tempfile::tempdir().unwrap();
    unsafe { std::env::set_var("GAUGE_CONFIG_DIR", tmp.path()) };
    gauge::keys::generate("alice").unwrap();
    let server = MockServer::start().await;
    // auth mocks (same shape as tests/api.rs)
    use base64::Engine as _;
    let nonce_b64 = base64::engine::general_purpose::STANDARD_NO_PAD.encode([9u8; 32]);
    Mock::given(method("POST")).and(path("/v1/auth/challenge"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "challenge_id": "00000000-0000-4000-8000-000000000001",
            "nonce_b64": nonce_b64, "expires_in_s": 60
        }))).mount(&server).await;
    Mock::given(method("POST")).and(path("/v1/auth/verify"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "token": "t", "user_id": "alice",
            "expires_at": time::OffsetDateTime::now_utc().unix_timestamp() + 3600
        }))).mount(&server).await;
    Mock::given(method("POST")).and(path("/v1/query"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "rows": [{"app": "tome", "count": 7, "unique_installs": 3}],
            "truncated": false, "elapsed_ms": 1
        }))).mount(&server).await;
    Mock::given(method("GET")).and(path("/v1/meta"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "apps": [{"app": "tome", "event_names": ["tome.search"], "attribute_keys": ["surface"],
                       "first_event": null, "last_event": null, "total_events": 7}]
        }))).mount(&server).await;

    let api = gauge::api::ApiClient::from_config(&gauge::config::ClientConfig {
        server_url: server.uri(), user_id: "alice".into(),
    });
    let snap = fetch(&api, TimeWindow::D7).await.unwrap();
    assert_eq!(snap.apps.len(), 1);
    assert_eq!(snap.totals[0]["count"], 7);
    assert!(!snap.timeseries.is_empty());
    assert!(!snap.top_events.is_empty());
    unsafe { std::env::remove_var("GAUGE_CONFIG_DIR") };
}

#[test]
fn time_windows_cycle_and_map_to_dsl() {
    assert_eq!(TimeWindow::H1.last(), "1h");
    assert_eq!(TimeWindow::D30.next(), TimeWindow::H1);
    assert_eq!(TimeWindow::H24.granularity(), gauge_query::Granularity::Hour);
    assert_eq!(TimeWindow::D7.granularity(), gauge_query::Granularity::Day);
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p gauge tui_data`
Expected: FAIL.

- [ ] **Step 3: Implement**

`crates/gauge/src/tui/data.rs`:

```rust
use gauge_query::{
    AppMeta, Dir, Field, Granularity, Measure, Order, QueryRequest, TimeRange,
};
use time::OffsetDateTime;

use crate::api::ApiClient;
use crate::error::ClientError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeWindow {
    H1,
    H24,
    D7,
    D30,
}

impl TimeWindow {
    pub fn next(self) -> Self {
        match self {
            Self::H1 => Self::H24,
            Self::H24 => Self::D7,
            Self::D7 => Self::D30,
            Self::D30 => Self::H1,
        }
    }
    pub fn last(&self) -> &'static str {
        match self {
            Self::H1 => "1h",
            Self::H24 => "24h",
            Self::D7 => "7d",
            Self::D30 => "30d",
        }
    }
    pub fn granularity(&self) -> Granularity {
        match self {
            Self::H1 | Self::H24 => Granularity::Hour,
            Self::D7 | Self::D30 => Granularity::Day,
        }
    }
    pub fn label(&self) -> &'static str {
        match self {
            Self::H1 => "last hour",
            Self::H24 => "last 24h",
            Self::D7 => "last 7d",
            Self::D30 => "last 30d",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Snapshot {
    pub fetched_at: OffsetDateTime,
    pub window: TimeWindow,
    /// rows: {time_bucket, app, count}
    pub timeseries: Vec<serde_json::Value>,
    /// rows: {app, count, unique_installs, unique_sessions}
    pub totals: Vec<serde_json::Value>,
    /// rows: {event_name, count}
    pub top_events: Vec<serde_json::Value>,
    pub apps: Vec<AppMeta>,
}

fn base(w: TimeWindow) -> QueryRequest {
    QueryRequest {
        measures: vec![Measure::Count],
        dimensions: vec![],
        filters: vec![],
        time_range: TimeRange::Last { last: w.last().into() },
        granularity: None,
        order: vec![],
        limit: None,
    }
}

pub async fn fetch(api: &ApiClient, w: TimeWindow) -> Result<Snapshot, ClientError> {
    let timeseries = api
        .query(&QueryRequest {
            dimensions: vec![Field::App],
            granularity: Some(w.granularity()),
            ..base(w)
        })
        .await?
        .rows;
    let totals = api
        .query(&QueryRequest {
            measures: vec![Measure::Count, Measure::UniqueInstalls, Measure::UniqueSessions],
            dimensions: vec![Field::App],
            order: vec![Order { field: "app".into(), dir: Dir::Asc }],
            ..base(w)
        })
        .await?
        .rows;
    let top_events = api
        .query(&QueryRequest {
            dimensions: vec![Field::EventName],
            order: vec![Order { field: "count".into(), dir: Dir::Desc }],
            limit: Some(10),
            ..base(w)
        })
        .await?
        .rows;
    let apps = api.meta().await?.apps;
    Ok(Snapshot { fetched_at: OffsetDateTime::now_utc(), window: w, timeseries, totals, top_events, apps })
}
```

`crates/gauge/src/tui/mod.rs`: `pub mod data;` — add `pub mod tui;` to `lib.rs`.

- [ ] **Step 4: Run to verify pass**

Run: `cargo test -p gauge`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/gauge Cargo.toml
git commit -m "feat(client): TUI data layer with windowed snapshot fetching"
```

---

### Task 29: gauge — TUI app state + rendering

**Files:**
- Create: `crates/gauge/src/tui/app.rs`, `crates/gauge/src/tui/ui.rs`, `crates/gauge/tests/tui_render.rs`
- Modify: `crates/gauge/src/tui/mod.rs`

- [ ] **Step 1: Write the failing test**

`crates/gauge/tests/tui_render.rs`:

```rust
use gauge::tui::app::{App, Page};
use gauge::tui::data::{Snapshot, TimeWindow};
use gauge::tui::ui;
use ratatui::Terminal;
use ratatui::backend::TestBackend;

fn synthetic_snapshot() -> Snapshot {
    Snapshot {
        fetched_at: time::OffsetDateTime::now_utc(),
        window: TimeWindow::D7,
        timeseries: vec![
            serde_json::json!({"time_bucket": "2026-06-10T00:00:00Z", "app": "tome", "count": 5}),
            serde_json::json!({"time_bucket": "2026-06-11T00:00:00Z", "app": "tome", "count": 9}),
        ],
        totals: vec![serde_json::json!({"app": "tome", "count": 14, "unique_installs": 4, "unique_sessions": 6})],
        top_events: vec![serde_json::json!({"event_name": "tome.search", "count": 11})],
        apps: vec![gauge_query::AppMeta {
            app: "tome".into(), event_names: vec!["tome.search".into()],
            attribute_keys: vec!["surface".into()], first_event: None, last_event: None, total_events: 14,
        }],
    }
}

fn buffer_text(terminal: &Terminal<TestBackend>) -> String {
    let buf = terminal.backend().buffer();
    let mut s = String::new();
    for y in 0..buf.area.height {
        for x in 0..buf.area.width {
            s.push_str(buf[(x, y)].symbol());
        }
        s.push('\n');
    }
    s
}

#[test]
fn overview_renders_key_widgets() {
    let mut app = App::new();
    app.snapshot = Some(synthetic_snapshot());
    let backend = TestBackend::new(120, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|f| ui::render(f, &app)).unwrap();
    let text = buffer_text(&terminal);
    assert!(text.contains("Events over time"));
    assert!(text.contains("Top events"));
    assert!(text.contains("tome.search"));
    assert!(text.contains("Unique installs"));
    assert!(text.contains("last 7d"));
}

#[test]
fn stale_banner_renders_when_set() {
    let mut app = App::new();
    app.snapshot = Some(synthetic_snapshot());
    app.stale = Some("connection refused".into());
    let backend = TestBackend::new(120, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|f| ui::render(f, &app)).unwrap();
    assert!(buffer_text(&terminal).contains("STALE"));
}

#[test]
fn keys_drive_state() {
    use crossterm::event::KeyCode;
    let mut app = App::new();
    assert_eq!(app.page, Page::Overview);
    app.on_key(KeyCode::Tab);
    assert_eq!(app.page, Page::Apps);
    app.on_key(KeyCode::Char('t'));
    assert!(app.refresh_requested);
    app.on_key(KeyCode::Char('q'));
    assert!(app.should_quit);
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p gauge tui_render`
Expected: FAIL.

- [ ] **Step 3: Implement**

`crates/gauge/src/tui/app.rs`:

```rust
use crossterm::event::KeyCode;
use gauge_query::QueryResponse;

use crate::tui::data::{Snapshot, TimeWindow};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Overview,
    Apps,
    Explore,
}

pub const EXPLORE_MEASURES: &[&str] = &["count", "unique_installs", "unique_sessions"];
pub const EXPLORE_DIMENSIONS: &[&str] = &["app", "event_name", "os", "arch", "app_version"];

#[derive(Debug, Default)]
pub struct ExploreState {
    pub measure_idx: usize,
    pub dimension_idx: usize,
    pub run_requested: bool,
    pub result: Option<QueryResponse>,
}

pub struct App {
    pub page: Page,
    pub window: TimeWindow,
    pub snapshot: Option<Snapshot>,
    /// Some(reason) → keep last snapshot, show stale banner.
    pub stale: Option<String>,
    pub selected_app: usize,
    pub explore: ExploreState,
    pub should_quit: bool,
    pub refresh_requested: bool,
}

impl App {
    pub fn new() -> Self {
        Self {
            page: Page::Overview,
            window: TimeWindow::D7,
            snapshot: None,
            stale: None,
            selected_app: 0,
            explore: ExploreState::default(),
            should_quit: false,
            refresh_requested: true, // fetch immediately on start
        }
    }

    pub fn on_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Char('q') | KeyCode::Esc => self.should_quit = true,
            KeyCode::Tab => {
                self.page = match self.page {
                    Page::Overview => Page::Apps,
                    Page::Apps => Page::Explore,
                    Page::Explore => Page::Overview,
                }
            }
            KeyCode::Char('t') => {
                self.window = self.window.next();
                self.refresh_requested = true;
            }
            KeyCode::Char('r') => self.refresh_requested = true,
            KeyCode::Left if self.page == Page::Apps => {
                self.selected_app = self.selected_app.saturating_sub(1)
            }
            KeyCode::Right if self.page == Page::Apps => {
                let max = self.snapshot.as_ref().map(|s| s.apps.len().saturating_sub(1)).unwrap_or(0);
                self.selected_app = (self.selected_app + 1).min(max);
            }
            KeyCode::Up if self.page == Page::Explore => {
                self.explore.measure_idx = (self.explore.measure_idx + 1) % EXPLORE_MEASURES.len()
            }
            KeyCode::Down if self.page == Page::Explore => {
                self.explore.dimension_idx = (self.explore.dimension_idx + 1) % EXPLORE_DIMENSIONS.len()
            }
            KeyCode::Enter if self.page == Page::Explore => self.explore.run_requested = true,
            _ => {}
        }
    }

    /// QueryRequest for the current Explore selection.
    pub fn explore_request(&self) -> gauge_query::QueryRequest {
        let json = serde_json::json!({
            "measures": [EXPLORE_MEASURES[self.explore.measure_idx]],
            "dimensions": [EXPLORE_DIMENSIONS[self.explore.dimension_idx]],
            "time_range": {"last": self.window.last()},
            "order": [{"field": EXPLORE_MEASURES[self.explore.measure_idx], "dir": "desc"}],
            "limit": 50
        });
        serde_json::from_value(json).expect("explore request is always valid")
    }
}
```

`crates/gauge/src/tui/ui.rs`:

```rust
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::symbols;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Axis, Bar, BarChart, BarGroup, Block, Borders, Chart, Dataset, GraphType, Paragraph, Row, Table};

use crate::tui::app::{App, EXPLORE_DIMENSIONS, EXPLORE_MEASURES, Page};

pub fn render(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(0)])
        .split(f.area());
    render_status(f, app, chunks[0]);
    match app.page {
        Page::Overview => render_overview(f, app, chunks[1]),
        Page::Apps => render_apps(f, app, chunks[1]),
        Page::Explore => render_explore(f, app, chunks[1]),
    }
}

fn render_status(f: &mut Frame, app: &App, area: Rect) {
    let mut spans = vec![
        Span::styled(" gauge ", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(format!("[{:?}] ", app.page)),
        Span::raw(format!("({}) ", app.window.label())),
        Span::raw("tab:page  t:range  r:refresh  q:quit"),
    ];
    if let Some(reason) = &app.stale {
        spans.push(Span::styled(
            format!("  STALE: {reason}"),
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        ));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn render_overview(f: &mut Frame, app: &App, area: Rect) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);
    let bottom = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(34), Constraint::Percentage(33), Constraint::Percentage(33)])
        .split(rows[1]);
    render_timeseries(f, app, rows[0]);
    render_totals(f, app, bottom[0]);
    render_top_events(f, app, bottom[1]);
    render_apps_table(f, app, bottom[2]);
}

const SERIES_COLORS: &[Color] = &[Color::Cyan, Color::Magenta, Color::Yellow, Color::Green, Color::Blue];

fn render_timeseries(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default().borders(Borders::ALL).title("Events over time");
    let Some(snap) = &app.snapshot else {
        f.render_widget(Paragraph::new("loading…").block(block), area);
        return;
    };
    // group rows by app; x = index of sorted distinct time buckets
    let mut buckets: Vec<&str> = snap.timeseries.iter().filter_map(|r| r["time_bucket"].as_str()).collect();
    buckets.sort_unstable();
    buckets.dedup();
    let mut series: std::collections::BTreeMap<&str, Vec<(f64, f64)>> = Default::default();
    let mut y_max: f64 = 1.0;
    for row in &snap.timeseries {
        let (Some(appn), Some(bucket)) = (row["app"].as_str(), row["time_bucket"].as_str()) else { continue };
        let count = row["count"].as_i64().unwrap_or(0) as f64;
        y_max = y_max.max(count);
        let x = buckets.iter().position(|b| *b == bucket).unwrap_or(0) as f64;
        series.entry(appn).or_default().push((x, count));
    }
    let datasets: Vec<Dataset> = series
        .iter()
        .enumerate()
        .map(|(i, (name, points))| {
            Dataset::default()
                .name(name.to_string())
                .marker(symbols::Marker::Braille)
                .graph_type(GraphType::Line)
                .style(Style::default().fg(SERIES_COLORS[i % SERIES_COLORS.len()]))
                .data(points)
        })
        .collect();
    let x_max = (buckets.len().saturating_sub(1)).max(1) as f64;
    let chart = Chart::new(datasets)
        .block(block)
        .x_axis(Axis::default().bounds([0.0, x_max]))
        .y_axis(Axis::default().bounds([0.0, y_max * 1.1]).labels(vec![
            Span::raw("0"),
            Span::raw(format!("{}", y_max as i64)),
        ]));
    f.render_widget(chart, area);
}

fn render_totals(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default().borders(Borders::ALL).title(format!("Unique installs ({})", app.window.label()));
    let Some(snap) = &app.snapshot else {
        f.render_widget(block, area);
        return;
    };
    let lines: Vec<Line> = snap
        .totals
        .iter()
        .map(|r| {
            Line::from(format!(
                "{:<18} events {:>8}   installs {:>6}   sessions {:>6}",
                r["app"].as_str().unwrap_or("?"),
                r["count"].as_i64().unwrap_or(0),
                r["unique_installs"].as_i64().unwrap_or(0),
                r["unique_sessions"].as_i64().unwrap_or(0),
            ))
        })
        .collect();
    f.render_widget(Paragraph::new(lines).block(block), area);
}

fn render_top_events(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default().borders(Borders::ALL).title("Top events");
    let Some(snap) = &app.snapshot else {
        f.render_widget(block, area);
        return;
    };
    let bars: Vec<Bar> = snap
        .top_events
        .iter()
        .map(|r| {
            Bar::default()
                .label(r["event_name"].as_str().unwrap_or("?").to_string().into())
                .value(r["count"].as_i64().unwrap_or(0) as u64)
        })
        .collect();
    let chart = BarChart::default()
        .block(block)
        .direction(Direction::Horizontal)
        .bar_width(1)
        .data(BarGroup::default().bars(&bars));
    f.render_widget(chart, area);
}

fn render_apps_table(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default().borders(Borders::ALL).title("Apps");
    let Some(snap) = &app.snapshot else {
        f.render_widget(block, area);
        return;
    };
    let rows: Vec<Row> = snap
        .apps
        .iter()
        .map(|a| {
            Row::new(vec![
                a.app.clone(),
                a.total_events.to_string(),
                a.event_names.len().to_string(),
                a.last_event.clone().unwrap_or_else(|| "-".into()),
            ])
        })
        .collect();
    let table = Table::new(
        rows,
        [Constraint::Min(16), Constraint::Length(10), Constraint::Length(8), Constraint::Min(20)],
    )
    .header(Row::new(vec!["app", "events", "types", "last seen"]).style(Style::default().add_modifier(Modifier::BOLD)))
    .block(block);
    f.render_widget(table, area);
}

fn render_apps(f: &mut Frame, app: &App, area: Rect) {
    // App detail: event-name breakdown for the selected app (←/→ to switch)
    let block = Block::default().borders(Borders::ALL).title("App detail (←/→ to switch app)");
    let Some(snap) = &app.snapshot else {
        f.render_widget(block, area);
        return;
    };
    let Some(meta) = snap.apps.get(app.selected_app.min(snap.apps.len().saturating_sub(1))) else {
        f.render_widget(Paragraph::new("no apps yet").block(block), area);
        return;
    };
    let mut lines = vec![
        Line::from(Span::styled(meta.app.clone(), Style::default().add_modifier(Modifier::BOLD))),
        Line::from(format!("total events: {}", meta.total_events)),
        Line::from(format!("first: {}  last: {}",
            meta.first_event.as_deref().unwrap_or("-"),
            meta.last_event.as_deref().unwrap_or("-"))),
        Line::from(""),
        Line::from("event types:"),
    ];
    lines.extend(meta.event_names.iter().map(|n| Line::from(format!("  {n}"))));
    lines.push(Line::from(""));
    lines.push(Line::from(format!("attribute keys: {}", meta.attribute_keys.join(", "))));
    f.render_widget(Paragraph::new(lines).block(block), area);
}

fn render_explore(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(area);
    let picker = Paragraph::new(format!(
        "measure (↑): {}    dimension (↓): {}    enter: run",
        EXPLORE_MEASURES[app.explore.measure_idx],
        EXPLORE_DIMENSIONS[app.explore.dimension_idx],
    ))
    .block(Block::default().borders(Borders::ALL).title("Explore"));
    f.render_widget(picker, chunks[0]);

    let block = Block::default().borders(Borders::ALL).title("Result");
    match &app.explore.result {
        None => f.render_widget(Paragraph::new("press enter to run").block(block), chunks[1]),
        Some(resp) => {
            let lines: Vec<Line> = resp
                .rows
                .iter()
                .map(|r| Line::from(serde_json::to_string(r).unwrap_or_default()))
                .collect();
            f.render_widget(Paragraph::new(lines).block(block), chunks[1]);
        }
    }
}
```

Add `pub mod app; pub mod ui;` to `tui/mod.rs`.

- [ ] **Step 4: Run to verify pass**

Run: `cargo test -p gauge`
Expected: PASS. (ratatui widget APIs move between minors — if `Bar`/`BarGroup`/axis builders changed, adapt per docs.rs and note at the gate.)

- [ ] **Step 5: Commit**

```bash
git add crates/gauge
git commit -m "feat(client): TUI app state and dashboard rendering"
```

---

### Task 30: gauge — TUI event loop + wiring

**Files:**
- Create: `crates/gauge/src/tui/run.rs`
- Modify: `crates/gauge/src/tui/mod.rs`, `crates/gauge/src/main.rs`

- [ ] **Step 1: Implement the loop** (no headless test for the terminal loop itself — state and rendering are covered by Task 29; this is wiring)

`crates/gauge/src/tui/run.rs`:

```rust
use std::sync::Arc;
use std::time::Duration;

use crossterm::event::{Event, EventStream, KeyEventKind};
use futures::StreamExt as _;

use crate::api::ApiClient;
use crate::tui::app::App;
use crate::tui::data::{Snapshot, TimeWindow, fetch};
use crate::tui::ui;

enum Msg {
    Snapshot(Result<Snapshot, String>),
    Explore(Result<gauge_query::QueryResponse, String>),
}

fn spawn_fetch(api: Arc<ApiClient>, w: TimeWindow, tx: tokio::sync::mpsc::Sender<Msg>) {
    tokio::spawn(async move {
        let result = fetch(&api, w).await.map_err(|e| e.to_string());
        let _ = tx.send(Msg::Snapshot(result)).await;
    });
}

pub async fn run(api: ApiClient) -> Result<(), Box<dyn std::error::Error>> {
    let mut terminal = ratatui::init();
    let result = event_loop(&mut terminal, api).await;
    ratatui::restore();
    result
}

async fn event_loop(
    terminal: &mut ratatui::DefaultTerminal,
    api: ApiClient,
) -> Result<(), Box<dyn std::error::Error>> {
    let api = Arc::new(api);
    let mut app = App::new();
    let (tx, mut rx) = tokio::sync::mpsc::channel::<Msg>(8);
    let mut events = EventStream::new();
    let mut tick = tokio::time::interval(Duration::from_secs(30));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        if app.refresh_requested {
            app.refresh_requested = false;
            spawn_fetch(api.clone(), app.window, tx.clone());
        }
        if app.explore.run_requested {
            app.explore.run_requested = false;
            let req = app.explore_request();
            let api = api.clone();
            let tx = tx.clone();
            tokio::spawn(async move {
                let result = api.query(&req).await.map_err(|e| e.to_string());
                let _ = tx.send(Msg::Explore(result)).await;
            });
        }
        terminal.draw(|f| ui::render(f, &app))?;
        tokio::select! {
            maybe_ev = events.next() => {
                if let Some(Ok(Event::Key(k))) = maybe_ev
                    && k.kind == KeyEventKind::Press
                {
                    app.on_key(k.code);
                }
            }
            Some(msg) = rx.recv() => match msg {
                Msg::Snapshot(Ok(s)) => { app.snapshot = Some(s); app.stale = None; }
                Msg::Snapshot(Err(e)) => app.stale = Some(e),
                Msg::Explore(Ok(r)) => app.explore.result = Some(r),
                Msg::Explore(Err(e)) => app.stale = Some(e),
            },
            _ = tick.tick() => app.refresh_requested = true,
        }
        if app.should_quit {
            return Ok(());
        }
    }
}
```

Add `pub mod run;` to `tui/mod.rs`. Wire `Cmd::Tui` in `main.rs`:

```rust
        Cmd::Tui => {
            let cfg = gauge::config::ClientConfig::load()?;
            let api = gauge::api::ApiClient::from_config(&cfg);
            gauge::tui::run::run(api).await
        }
```

- [ ] **Step 2: Verify compile + full client suite**

Run: `cargo clippy -p gauge --all-targets -- -D warnings && cargo test -p gauge`
Expected: PASS, no warnings.

- [ ] **Step 3: Manual smoke**

Against a deployed or locally-running gauge-server with seeded data: `cargo run -p gauge -- tui` — verify: overview renders, `t` cycles ranges and refetches, killing the network shows the STALE banner while the last snapshot stays up, `q` exits cleanly restoring the terminal.

- [ ] **Step 4: Commit**

```bash
git add crates/gauge
git commit -m "feat(client): TUI event loop with background refresh and stale fallback"
```

---

## PHASE GATE 2 → 3

- [ ] `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace` — all green.
- [ ] `grep -rn "todo_stub" crates/gauge/src` returns nothing (all subcommands wired).
- [ ] Manual E2E pass: `gauge keys generate` → register in deployed users.toml → `gauge login` → `gauge query '{"measures":["count"],"time_range":{"last":"7d"}}'` → `gauge tui` → MCP smoke via `tools/list`.
- [ ] Re-read Phase 3 below against learnings (especially gauge-events API surface and reqwest feature handling). Edit affected steps.
- [ ] Update the Plan changelog; commit the revision.

---

# PHASE 3 — Sender batching client (`gauge_events::sender`)

> Library code for future Tome/Midnight Manual migrations: local disk queue + crash-safe OTLP drain. Feature-gated so server/client builds don't pull blocking reqwest.

### Task 31: gauge-events — sender feature + disk queue

**Files:**
- Create: `crates/gauge-events/src/sender/mod.rs`, `crates/gauge-events/src/sender/queue.rs`
- Modify: `crates/gauge-events/Cargo.toml`, `crates/gauge-events/src/lib.rs`

- [ ] **Step 1: Feature wiring**

`crates/gauge-events/Cargo.toml` — add:

```toml
[features]
sender = ["dep:reqwest"]

[dependencies]
reqwest = { workspace = true, optional = true, features = ["blocking"] }

[dev-dependencies]
tempfile.workspace = true
tokio.workspace = true
wiremock.workspace = true
```

`lib.rs`: add `#[cfg(feature = "sender")] pub mod sender;`

- [ ] **Step 2: Write the failing tests**

Tests module in `crates/gauge-events/src/sender/queue.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_then_read_round_trips() {
        let tmp = tempfile::tempdir().unwrap();
        let q = tmp.path().join("queue.jsonl");
        assert!(matches!(append_line(&q, "{\"a\":1}").unwrap(), AppendOutcome::Appended));
        assert!(matches!(append_line(&q, "{\"b\":2}").unwrap(), AppendOutcome::Appended));
        assert_eq!(read_lines(&q).unwrap(), vec!["{\"a\":1}", "{\"b\":2}"]);
    }

    #[test]
    fn oversized_line_is_dropped() {
        let tmp = tempfile::tempdir().unwrap();
        let q = tmp.path().join("queue.jsonl");
        let big = "x".repeat(MAX_LINE_BYTES + 1);
        assert!(matches!(append_line(&q, &big).unwrap(), AppendOutcome::DroppedTooLong));
        assert!(read_lines(&q).unwrap().is_empty());
    }

    #[test]
    fn full_queue_drops_new_lines() {
        let tmp = tempfile::tempdir().unwrap();
        let q = tmp.path().join("queue.jsonl");
        let line = "y".repeat(MAX_LINE_BYTES - 1);
        loop {
            match append_line(&q, &line).unwrap() {
                AppendOutcome::Appended => continue,
                AppendOutcome::DroppedQueueFull => break,
                other => panic!("unexpected {other:?}"),
            }
        }
        assert!(std::fs::metadata(&q).unwrap().len() <= MAX_QUEUE_BYTES);
    }

    #[test]
    fn rewrite_atomic_replaces_content() {
        let tmp = tempfile::tempdir().unwrap();
        let q = tmp.path().join("queue.jsonl");
        append_line(&q, "one").unwrap();
        append_line(&q, "two").unwrap();
        rewrite_atomic(&q, &["two".to_string()]).unwrap();
        assert_eq!(read_lines(&q).unwrap(), vec!["two"]);
        rewrite_atomic(&q, &[]).unwrap();
        assert!(read_lines(&q).unwrap().is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn queue_file_is_0600() {
        use std::os::unix::fs::PermissionsExt as _;
        let tmp = tempfile::tempdir().unwrap();
        let q = tmp.path().join("queue.jsonl");
        append_line(&q, "z").unwrap();
        assert_eq!(std::fs::metadata(&q).unwrap().permissions().mode() & 0o777, 0o600);
    }
}
```

- [ ] **Step 3: Run to verify failure**

Run: `cargo test -p gauge-events --features sender queue`
Expected: FAIL.

- [ ] **Step 4: Implement**

`crates/gauge-events/src/sender/queue.rs` (above tests):

```rust
//! Append-only JSONL disk queue, modeled on Tome's telemetry queue:
//! one O_APPEND write per event, hard caps, atomic rewrite after delivery.

use std::io::Write as _;
use std::path::Path;

pub const MAX_LINE_BYTES: usize = 4096;
pub const MAX_QUEUE_BYTES: u64 = 1_048_576;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppendOutcome {
    Appended,
    DroppedTooLong,
    DroppedQueueFull,
}

pub fn append_line(path: &Path, line: &str) -> std::io::Result<AppendOutcome> {
    if line.len() > MAX_LINE_BYTES {
        return Ok(AppendOutcome::DroppedTooLong);
    }
    let current = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    if current + line.len() as u64 + 1 > MAX_QUEUE_BYTES {
        return Ok(AppendOutcome::DroppedQueueFull);
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut opts = std::fs::OpenOptions::new();
    opts.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        opts.mode(0o600);
    }
    let mut f = opts.open(path)?;
    let mut buf = Vec::with_capacity(line.len() + 1);
    buf.extend_from_slice(line.as_bytes());
    buf.push(b'\n');
    f.write_all(&buf)?;
    Ok(AppendOutcome::Appended)
}

pub fn read_lines(path: &Path) -> std::io::Result<Vec<String>> {
    match std::fs::read_to_string(path) {
        Ok(s) => Ok(s.lines().map(str::to_string).filter(|l| !l.is_empty()).collect()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(vec![]),
        Err(e) => Err(e),
    }
}

/// Write remaining lines to a temp file, then rename over the queue.
/// Crash before rename → old queue intact (resend = at-least-once).
pub fn rewrite_atomic(path: &Path, remaining: &[String]) -> std::io::Result<()> {
    let tmp = path.with_extension("jsonl.tmp");
    let mut content = String::new();
    for l in remaining {
        content.push_str(l);
        content.push('\n');
    }
    std::fs::write(&tmp, content)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))?;
    }
    std::fs::rename(&tmp, path)
}
```

`crates/gauge-events/src/sender/mod.rs`:

```rust
pub mod queue;
```

- [ ] **Step 5: Run to verify pass**

Run: `cargo test -p gauge-events --features sender`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/gauge-events
git commit -m "feat(events): sender disk queue with caps and atomic rewrite"
```

---

### Task 32: gauge-events — sender config, enqueue, OTLP encoder

**Files:**
- Create: `crates/gauge-events/src/sender/encode.rs`
- Modify: `crates/gauge-events/src/sender/mod.rs`

- [ ] **Step 1: Write the failing tests**

Tests module in `crates/gauge-events/src/sender/encode.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::validate_batch;

    fn cfg(tmp: &std::path::Path) -> SenderConfig {
        SenderConfig {
            endpoint: "https://gauge-telemetry.fly.dev".into(),
            app: "tome".into(),
            app_version: "0.7.0".into(),
            install_id: uuid::Uuid::new_v4(),
            session_id: uuid::Uuid::new_v4(),
            os: "darwin".into(),
            arch: "arm64".into(),
            queue_path: tmp.join("queue.jsonl"),
        }
    }

    #[test]
    fn encoded_batch_passes_profile_validation() {
        let tmp = tempfile::tempdir().unwrap();
        let c = cfg(tmp.path());
        let mut attributes = serde_json::Map::new();
        attributes.insert("surface".into(), serde_json::json!("cli"));
        attributes.insert("reranker_used".into(), serde_json::json!(true));
        attributes.insert("candidates".into(), serde_json::json!(12));
        let ev = QueuedEvent {
            event_name: "tome.search".into(),
            time_unix_nano: 1_781_430_705_123_000_000,
            attributes,
        };
        let req = encode_batch(&c, &[ev]);
        let batch = validate_batch(&req, &["tome".to_string()]).unwrap();
        assert_eq!(batch.resource.app, "tome");
        assert_eq!(batch.events.len(), 1);
        assert!(batch.rejections.is_empty(), "{:?}", batch.rejections);
        // both event-name carriers present
        let rec = &req.resource_logs[0].scope_logs[0].log_records[0];
        assert_eq!(rec.event_name.as_deref(), Some("tome.search"));
        assert!(rec.attributes.iter().any(|kv| kv.key == "event.name"));
    }

    #[test]
    fn enqueue_writes_parseable_line() {
        let tmp = tempfile::tempdir().unwrap();
        let c = cfg(tmp.path());
        let mut attributes = serde_json::Map::new();
        attributes.insert("surface".into(), serde_json::json!("mcp"));
        enqueue(&c, "tome.search", attributes).unwrap();
        let lines = crate::sender::queue::read_lines(&c.queue_path).unwrap();
        assert_eq!(lines.len(), 1);
        let ev: QueuedEvent = serde_json::from_str(&lines[0]).unwrap();
        assert_eq!(ev.event_name, "tome.search");
        assert!(ev.time_unix_nano > 0);
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p gauge-events --features sender encode`
Expected: FAIL.

- [ ] **Step 3: Implement**

`crates/gauge-events/src/sender/encode.rs` (above tests):

```rust
use std::path::PathBuf;

use serde_json::{Map, Value};
use uuid::Uuid;

use crate::otlp::{
    AnyValue, ExportLogsServiceRequest, KeyValue, LogRecord, Resource, ResourceLogs, ScopeLogs,
};
use crate::sender::queue::{self, AppendOutcome};

#[derive(Debug, Clone)]
pub struct SenderConfig {
    /// Base server URL (no trailing slash), e.g. https://gauge-telemetry.fly.dev
    pub endpoint: String,
    pub app: String,
    pub app_version: String,
    pub install_id: Uuid,
    pub session_id: Uuid,
    pub os: String,
    pub arch: String,
    pub queue_path: PathBuf,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct QueuedEvent {
    pub event_name: String,
    pub time_unix_nano: u64,
    pub attributes: Map<String, Value>,
}

/// One append, no network — safe to call from any foreground path.
pub fn enqueue(
    cfg: &SenderConfig,
    event_name: &str,
    attributes: Map<String, Value>,
) -> std::io::Result<AppendOutcome> {
    let now = time::OffsetDateTime::now_utc();
    let ev = QueuedEvent {
        event_name: event_name.to_string(),
        time_unix_nano: now.unix_timestamp_nanos().max(0) as u64,
        attributes,
    };
    let line = serde_json::to_string(&ev).expect("QueuedEvent always serializes");
    queue::append_line(&cfg.queue_path, &line)
}

fn str_kv(key: &str, v: &str) -> KeyValue {
    KeyValue { key: key.into(), value: AnyValue { string_value: Some(v.into()), ..Default::default() } }
}

fn value_to_any(v: &Value) -> AnyValue {
    match v {
        Value::String(s) => AnyValue { string_value: Some(s.clone()), ..Default::default() },
        Value::Bool(b) => AnyValue { bool_value: Some(*b), ..Default::default() },
        Value::Number(n) if n.is_i64() => {
            AnyValue { int_value: Some(n.as_i64().unwrap_or(0).to_string()), ..Default::default() }
        }
        Value::Number(n) => AnyValue { double_value: n.as_f64(), ..Default::default() },
        // non-scalars never come from enqueue(); encode defensively as nothing
        _ => AnyValue::default(),
    }
}

pub fn encode_batch(cfg: &SenderConfig, events: &[QueuedEvent]) -> ExportLogsServiceRequest {
    let resource = Resource {
        attributes: vec![
            str_kv("service.name", &cfg.app),
            str_kv("service.version", &cfg.app_version),
            str_kv("service.instance.id", &cfg.install_id.to_string()),
            str_kv("session.id", &cfg.session_id.to_string()),
            str_kv("os.type", &cfg.os),
            str_kv("host.arch", &cfg.arch),
        ],
    };
    let log_records = events
        .iter()
        .map(|e| {
            let mut attributes = vec![str_kv("event.name", &e.event_name)];
            attributes.extend(
                e.attributes
                    .iter()
                    .map(|(k, v)| KeyValue { key: k.clone(), value: value_to_any(v) }),
            );
            LogRecord {
                time_unix_nano: Some(e.time_unix_nano),
                event_name: Some(e.event_name.clone()),
                attributes,
            }
        })
        .collect();
    ExportLogsServiceRequest {
        resource_logs: vec![ResourceLogs {
            resource: Some(resource),
            scope_logs: vec![ScopeLogs { log_records }],
        }],
    }
}
```

Update `sender/mod.rs`:

```rust
pub mod encode;
pub mod queue;

pub use encode::{QueuedEvent, SenderConfig, encode_batch, enqueue};
```

- [ ] **Step 4: Run to verify pass**

Run: `cargo test -p gauge-events --features sender`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/gauge-events
git commit -m "feat(events): sender enqueue and OTLP batch encoder"
```

---

### Task 33: gauge-events — sender transport + crash-safe drain

**Files:**
- Create: `crates/gauge-events/src/sender/transport.rs`, `crates/gauge-events/src/sender/drain.rs`, `crates/gauge-events/tests/sender_drain.rs`
- Modify: `crates/gauge-events/src/sender/mod.rs`

- [ ] **Step 1: Write the failing tests**

`crates/gauge-events/tests/sender_drain.rs`:

```rust
#![cfg(feature = "sender")]

use gauge_events::sender::{SenderConfig, drain, enqueue};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn cfg(tmp: &std::path::Path, endpoint: &str) -> SenderConfig {
    SenderConfig {
        endpoint: endpoint.trim_end_matches('/').to_string(),
        app: "tome".into(),
        app_version: "0.7.0".into(),
        install_id: uuid::Uuid::new_v4(),
        session_id: uuid::Uuid::new_v4(),
        os: "linux".into(),
        arch: "amd64".into(),
        queue_path: tmp.join("queue.jsonl"),
    }
}

fn attrs() -> serde_json::Map<String, serde_json::Value> {
    let mut m = serde_json::Map::new();
    m.insert("surface".into(), serde_json::json!("cli"));
    m
}

#[tokio::test]
async fn drain_posts_and_empties_queue() {
    let server = MockServer::start().await;
    Mock::given(method("POST")).and(path("/v1/logs"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
        .expect(1)
        .mount(&server).await;
    let tmp = tempfile::tempdir().unwrap();
    let c = cfg(tmp.path(), &server.uri());
    enqueue(&c, "tome.search", attrs()).unwrap();
    enqueue(&c, "tome.install", attrs()).unwrap();

    let report = tokio::task::spawn_blocking(move || drain(&c)).await.unwrap().unwrap();
    assert_eq!(report.sent, 2);
    assert_eq!(report.remaining, 0);

    // the body that arrived was a valid Gauge batch with 2 records
    let reqs = server.received_requests().await.unwrap();
    let body: gauge_events::otlp::ExportLogsServiceRequest =
        serde_json::from_slice(&reqs[0].body).unwrap();
    let batch = gauge_events::profile::validate_batch(&body, &["tome".to_string()]).unwrap();
    assert_eq!(batch.events.len(), 2);
}

#[tokio::test]
async fn server_error_keeps_queue_intact() {
    let server = MockServer::start().await;
    Mock::given(method("POST")).and(path("/v1/logs"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&server).await;
    let tmp = tempfile::tempdir().unwrap();
    let c = cfg(tmp.path(), &server.uri());
    enqueue(&c, "tome.search", attrs()).unwrap();
    let queue_path = c.queue_path.clone();

    let report = tokio::task::spawn_blocking(move || drain(&c)).await.unwrap().unwrap();
    assert_eq!(report.sent, 0);
    assert_eq!(report.remaining, 1); // at-least-once: nothing lost
    assert_eq!(gauge_events::sender::queue::read_lines(&queue_path).unwrap().len(), 1);
}

#[tokio::test]
async fn https_is_required_except_loopback() {
    let tmp = tempfile::tempdir().unwrap();
    let c = cfg(tmp.path(), "http://example.com");
    enqueue(&c, "tome.search", attrs()).unwrap();
    let err = tokio::task::spawn_blocking(move || drain(&c)).await.unwrap().unwrap_err();
    assert!(err.to_string().contains("https"));
}

#[tokio::test]
async fn concurrent_drain_is_skipped_by_lock() {
    let tmp = tempfile::tempdir().unwrap();
    let c = cfg(tmp.path(), "https://gauge-telemetry.fly.dev");
    enqueue(&c, "tome.search", attrs()).unwrap();
    std::fs::write(c.queue_path.with_extension("lock"), b"pid").unwrap(); // fresh lock held
    let report = tokio::task::spawn_blocking(move || drain(&c)).await.unwrap().unwrap();
    assert!(report.skipped_lock);
    assert_eq!(report.sent, 0);
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p gauge-events --features sender sender_drain`
Expected: FAIL.

- [ ] **Step 3: Implement**

`crates/gauge-events/src/sender/transport.rs`:

```rust
use thiserror::Error;

use crate::otlp::ExportLogsServiceRequest;

#[derive(Debug, Error)]
pub enum SenderError {
    #[error("endpoint must use https (plain http allowed only for loopback)")]
    InsecureEndpoint,
    #[error("http error: {0}")]
    Http(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub fn endpoint_allowed(endpoint: &str) -> bool {
    endpoint.starts_with("https://")
        || endpoint.starts_with("http://127.0.0.1")
        || endpoint.starts_with("http://localhost")
}

/// Blocking POST to {endpoint}/v1/logs. 5s timeout, no redirects (fail-closed).
pub fn post_batch(endpoint: &str, req: &ExportLogsServiceRequest) -> Result<u16, SenderError> {
    if !endpoint_allowed(endpoint) {
        return Err(SenderError::InsecureEndpoint);
    }
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| SenderError::Http(e.to_string()))?;
    let resp = client
        .post(format!("{endpoint}/v1/logs"))
        .json(req)
        .send()
        .map_err(|e| SenderError::Http(e.to_string()))?;
    Ok(resp.status().as_u16())
}
```

`crates/gauge-events/src/sender/drain.rs`:

```rust
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::profile::MAX_RECORDS_PER_BATCH;
use crate::sender::encode::{QueuedEvent, SenderConfig, encode_batch};
use crate::sender::queue;
use crate::sender::transport::{SenderError, post_batch};

const STALE_LOCK_AFTER: Duration = Duration::from_secs(600);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DrainReport {
    pub sent: usize,
    pub remaining: usize,
    pub skipped_lock: bool,
}

struct LockGuard(PathBuf);

impl Drop for LockGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn acquire_lock(path: &Path) -> std::io::Result<Option<LockGuard>> {
    match std::fs::OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(_) => Ok(Some(LockGuard(path.to_path_buf()))),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            let stale = std::fs::metadata(path)
                .and_then(|m| m.modified())
                .map(|t| SystemTime::now().duration_since(t).unwrap_or_default() > STALE_LOCK_AFTER)
                .unwrap_or(true);
            if stale {
                let _ = std::fs::remove_file(path);
                match std::fs::OpenOptions::new().write(true).create_new(true).open(path) {
                    Ok(_) => Ok(Some(LockGuard(path.to_path_buf()))),
                    Err(_) => Ok(None),
                }
            } else {
                Ok(None)
            }
        }
        Err(e) => Err(e),
    }
}

/// Drain the queue: parse, batch, POST, atomically rewrite survivors.
/// At-least-once: lines are removed ONLY after their batch got a 2xx;
/// a crash between POST and rewrite resends on the next drain.
pub fn drain(cfg: &SenderConfig) -> Result<DrainReport, SenderError> {
    let lock_path = cfg.queue_path.with_extension("lock");
    let Some(_guard) = acquire_lock(&lock_path)? else {
        return Ok(DrainReport { sent: 0, remaining: 0, skipped_lock: true });
    };

    let lines = queue::read_lines(&cfg.queue_path)?;
    if lines.is_empty() {
        return Ok(DrainReport { sent: 0, remaining: 0, skipped_lock: false });
    }
    // unparseable lines are dropped permanently (they can never send)
    let events: Vec<QueuedEvent> = lines
        .iter()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();

    let mut sent = 0usize;
    for chunk in events.chunks(MAX_RECORDS_PER_BATCH.min(100)) {
        let req = encode_batch(cfg, chunk);
        let status = post_batch(&cfg.endpoint, &req)?;
        if (200..300).contains(&status) {
            sent += chunk.len();
        } else {
            break; // keep this chunk and everything after it
        }
    }

    let remaining: Vec<String> = events[sent..]
        .iter()
        .map(|e| serde_json::to_string(e).expect("QueuedEvent serializes"))
        .collect();
    queue::rewrite_atomic(&cfg.queue_path, &remaining)?;
    Ok(DrainReport { sent, remaining: remaining.len(), skipped_lock: false })
}
```

Update `sender/mod.rs`:

```rust
pub mod drain;
pub mod encode;
pub mod queue;
pub mod transport;

pub use drain::{DrainReport, drain};
pub use encode::{QueuedEvent, SenderConfig, encode_batch, enqueue};
pub use transport::SenderError;
```

- [ ] **Step 4: Run to verify pass**

Run: `cargo test -p gauge-events --features sender`
Expected: PASS.

- [ ] **Step 5: Update CI to cover the feature**

In `.github/workflows/ci.yml`, change both `cargo test --workspace` and the clippy line to `--all-features`:

```yaml
      - run: cargo clippy --workspace --all-targets --all-features -- -D warnings
      - run: cargo test --workspace --all-features
```

- [ ] **Step 6: Commit**

```bash
git add crates/gauge-events .github
git commit -m "feat(events): crash-safe sender drain with lock and at-least-once delivery"
```

---

## PHASE GATE 3 — completion

- [ ] `cargo fmt --all --check && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo test --workspace --all-features` — all green.
- [ ] End-to-end against the deployed server: write a 5-line scratch program (or `cargo test`-style integration in a worktree) that uses `gauge_events::sender` to enqueue + drain real events, then confirm they appear in `gauge tui` and via `unique_users` through MCP.
- [ ] Update the Plan changelog (final state, drift notes useful to the Tome/Midnight Manual migration projects).
- [ ] Update `docs/superpowers/specs/2026-06-12-gauge-telemetry-platform-design.md` Future Work if anything learned here changes the migration guidance; commit.
- [ ] Use superpowers:finishing-a-development-branch to close out.






