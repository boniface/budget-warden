# budget-warden

[![CI](https://github.com/boniface/budget-warden/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/boniface/budget-warden/actions/workflows/ci.yml)
[![Scheduled Security](https://github.com/boniface/budget-warden/actions/workflows/scheduled-security.yml/badge.svg?branch=main)](https://github.com/boniface/budget-warden/actions/workflows/scheduled-security.yml)
[![Release](https://github.com/boniface/budget-warden/actions/workflows/release.yml/badge.svg)](https://github.com/boniface/budget-warden/actions/workflows/release.yml)
[![Crates.io](https://img.shields.io/crates/v/budget-warden.svg)](https://crates.io/crates/budget-warden)
[![Docs.rs](https://docs.rs/budget-warden/badge.svg)](https://docs.rs/budget-warden)
[![License](https://img.shields.io/crates/l/budget-warden.svg)](https://github.com/boniface/budget-warden#license)
[![Rust Version](https://img.shields.io/badge/rust-1.95%2B-blue.svg)](https://github.com/boniface/budget-warden/blob/main/Cargo.toml)
[![Rust Edition](https://img.shields.io/badge/edition-2024-blue.svg)](https://doc.rust-lang.org/edition-guide/rust-2024/)
[![Coverage Gate](https://img.shields.io/badge/coverage%20gate-90%25-brightgreen.svg)](https://github.com/boniface/budget-warden/actions/workflows/ci.yml)
[![Dependencies](https://deps.rs/repo/github/boniface/budget-warden/status.svg)](https://deps.rs/repo/github/boniface/budget-warden)

`budget-warden` is a Rust library for protecting applications from exhausting scarce third-party API budgets. It answers a simple question before a live call is made: should this request spend budget now, use a fallback, wait, downgrade, or be rejected?

It is useful for APIs with monthly or daily free tiers, paid request quotas, token budgets, credit balances, and other provider limits where a normal rate limiter is not enough. `budget-warden` tracks committed usage, short-lived reservations, pacing rules, and priority reserves so applications can avoid burning through a full provider allowance too early.

## Status

This crate is early v0.1 library code. The core broker, policy model, memory store, PostgreSQL store, Redis store, SQLite store, TOML policy loading, tracing hooks, CI, security checks, and crates.io release workflow are implemented.

## Core Ideas

- A `BudgetPolicy` describes a provider budget, such as 250 SerpApi requests per calendar month.
- A `BudgetRequest` describes a live operation that wants to spend budget.
- A `BudgetWarden` evaluates the request against matching policies and current usage.
- `authorize` checks whether a request is allowed without reserving budget.
- `reserve` atomically reserves budget and returns a `BudgetReservation`.
- A reservation should be committed after the live call is sent, or refunded if the call is not sent.
- Denials are normal `BudgetDecision::DenyLive` values, not errors.
- Internal failures, invalid requests, and store failures are returned as errors.

## Install

```toml
[dependencies]
budget-warden = "0.1"
```

Default features include the memory store, serde support, and chrono-based windows.

Feature flags:

- `memory`: in-process store for tests and local development.
- `postgres`: PostgreSQL durable store.
- `redis`: Redis distributed counter store.
- `sqlite`: SQLite embedded store.
- `toml`: TOML policy parsing.
- `tracing`: tracing events for decisions, reservations, and store failures.

## Quick Start

```rust
use budget_warden::{
    BudgetBroker, BudgetDecision, BudgetPolicy, BudgetRequest, BudgetStrategy, BudgetUnit,
    BudgetWarden, FallbackAction, MemoryStore, PreserveForWindow, Priority,
};

# async fn run() -> Result<(), Box<dyn std::error::Error>> {
let policy = BudgetPolicy::builder("serpapi-monthly-free-plan")
    .provider("serpapi")
    .domain("search")
    .resource("google-search")
    .subject("global")
    .unit(BudgetUnit::Requests)
    .hard_limit(250)
    .calendar_month("America/Toronto")
    .strategy(BudgetStrategy::PreserveForWindow(PreserveForWindow::new(
        10,
        20,
        Some(10),
    )))
    .exhausted_action(FallbackAction::UseStaleCache)
    .build()?;

let warden = BudgetWarden::builder()
    .store(MemoryStore::new())
    .policy(policy)
    .build()?;

let request = BudgetRequest::builder("serpapi", "search", "google-search")
    .subject("global")
    .unit(BudgetUnit::Requests)
    .amount(1)
    .priority(Priority::Normal)
    .build()?;

match warden.reserve(request).await? {
    BudgetDecision::AllowLive { reservation, .. } => {
        // Send the live provider request here.
        reservation.commit().await?;
    }
    BudgetDecision::DenyLive {
        recommended_action, ..
    } => {
        // Use cache, queue, downgrade, reject, or another application fallback.
        let _ = recommended_action;
    }
}
# Ok(())
# }
```

## Production Stores

The library accepts connection pools created by the host application. This is deliberate: the host app owns deployment-specific secret loading, TLS configuration, pool sizing, timeouts, and observability.

PostgreSQL:

```rust
# async fn run() -> Result<(), Box<dyn std::error::Error>> {
let database_url = std::env::var("DATABASE_URL")?;
let pool = sqlx_postgres::PgPoolOptions::new()
    .max_connections(10)
    .connect(&database_url)
    .await?;

let store = budget_warden::PostgresStore::new(pool);
store.setup_schema().await?;
# Ok(())
# }
```

Redis:

```rust
# async fn run() -> Result<(), Box<dyn std::error::Error>> {
let redis_url = std::env::var("REDIS_URL")?;
let config = deadpool_redis::Config::from_url(redis_url);
let pool = config.create_pool(Some(deadpool_redis::Runtime::Tokio1))?;

let store = budget_warden::RedisStore::new(pool);
# Ok(())
# }
```

SQLite:

```rust
# async fn run() -> Result<(), Box<dyn std::error::Error>> {
let pool = sqlx_sqlite::SqlitePoolOptions::new()
    .max_connections(1)
    .connect("sqlite://budget-warden.db")
    .await?;

let store = budget_warden::SqliteStore::new(pool);
store.setup_schema().await?;
# Ok(())
# }
```

## Secrets and `.env` Files

`budget-warden` does not read `.env.local`, `.env.prod`, Kubernetes Secrets, Docker secrets, or cloud secret managers directly. The host application should load secrets, read environment variables, create the database or Redis pool, and pass that pool into `budget-warden`.

Typical local development flow:

```rust
dotenvy::from_filename(".env.local")?;
let database_url = std::env::var("DATABASE_URL")?;
```

Typical production flow:

```text
Kubernetes Secret / Docker secret / cloud secret manager
        -> host application environment or mounted file
        -> host application creates DB or Redis pool
        -> budget-warden receives the ready-to-use pool
```

This keeps raw credentials out of budget policy files, avoids logging secrets inside the library, and lets each application use its own secret-management standard.

## TOML Policy Config

The `toml` feature parses budget policies, not infrastructure credentials. Backend credentials should remain in the host application environment.

```toml
[[policies]]
name = "serpapi-monthly-free-plan"
provider = "serpapi"
domain = "search"
resource = "google-search"
subject = "global"
unit = "requests"
hard_limit = 250
exhausted_action = "use_stale_cache"
fail_mode = "closed"

[policies.window]
type = "calendar_month"
timezone = "America/Toronto"

[policies.strategy]
type = "preserve_for_window"
max_spend_ahead_percent = 10
emergency_reserve_percent = 20
minimum_remaining_units = 10
```

Memory-backed config loading:

```rust
let warden = budget_warden::BudgetWarden::from_toml_file(
    "examples/config/serpapi_free_plan.toml",
)?;
```

Production applications can parse policies from TOML and provide their own store through the builder.

## Failure Modes

Each policy has a `fail_mode`:

- `closed`: store failures deny live spending by returning an error.
- `open`: store failures allow live access with a preview reservation.

Use `closed` when overspending is worse than temporary unavailability. Use `open` when availability is more important and the provider budget can tolerate risk during store outages.

## Quality Gates

Development and CI use the same baseline commands:

```sh
make dev
make fmt
make clippy
make test
make live-integration
make coverage
make security
```

Coverage must stay above 90% line coverage. Security checks use `cargo audit` and `cargo deny check`.

The CI coverage gate compiles all features, uploads an LCOV artifact, and fails below 90% line coverage. PostgreSQL and Redis stores are covered by the live store-contract job against CI service containers; SQLite is covered directly by the default all-feature test and coverage pass. Local `make live-integration` requires `BUDGET_WARDEN_POSTGRES_URL` and `BUDGET_WARDEN_REDIS_URL`; ordinary `cargo test --all-features` skips those live service checks when the URLs are not set.

## Examples

Runnable examples live in `examples/`:

- `examples/serpapi_free_plan.rs`
- `examples/weather_daily_budget.rs`
- `examples/news_cache_fallback.rs`
- `examples/toml_config.rs`

## Project Documents

- [CHANGELOG.md](CHANGELOG.md): release history.
- [CONTRIBUTING.md](CONTRIBUTING.md): contribution workflow and quality requirements.
- [SECURITY.md](SECURITY.md): vulnerability reporting and secret-handling policy.

## Release

Releases are published to crates.io through the manual `Release` GitHub Actions workflow only. The crates.io token must be stored as the `CARGO_REGISTRY_TOKEN` repository or `crates-io` environment secret and never committed to the repository.

Release checklist:

1. Update `Cargo.toml` version.
2. Update `CHANGELOG.md`.
3. Run `make dev`.
4. Run `cargo publish --dry-run`.
5. Open the `Release` workflow from the `main` branch.
6. Enter the plain semver version, for example `0.1.0`.
7. Run `mode=dry-run` first and review the validation result.
8. Run `mode=publish` to publish `budget-warden` to crates.io and create the GitHub release/tag.
9. Yank a bad release with `cargo yank --vers <version>` and publish a corrected patch release.
