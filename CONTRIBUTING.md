# Contributing

Thanks for helping improve `budget-warden`. This crate protects application budgets, so correctness, security, and predictable behavior matter more than adding broad surface area quickly.

## Development Setup

Install the Rust stable toolchain and the local quality tools:

```sh
cargo install cargo-llvm-cov
cargo install cargo-audit
cargo install cargo-deny
```

Run the default gate before opening a pull request:

```sh
make dev
```

Useful individual commands:

```sh
make fmt
make clippy
make test
make coverage
make security
make doc
```

Live store integration tests require PostgreSQL and Redis URLs:

```sh
export BUDGET_WARDEN_POSTGRES_URL=postgres://postgres:postgres@localhost:5432/budget_warden
export BUDGET_WARDEN_REDIS_URL=redis://localhost:6379
make live-integration
```

## Code Standards

- Keep modules organized with minimal `mod.rs` files containing declarations and re-exports only.
- Prefer small, focused types with private fields and accessors.
- Prefer borrowing over cloning. Clone only when ownership is required or the value is small and clarity wins.
- Do not use `unsafe` unless it is unavoidable and the safety invariant is documented.
- Do not use `.unwrap()` in production code paths.
- Use `thiserror` for library error types.
- Treat clippy diagnostics as design feedback. Do not silence lints unless the exception is narrow, documented, and justified.
- Keep functions focused and use a config struct when a function would otherwise need many parameters.
- Maintain deterministic cleanup for reservations and counters.

## Testing Expectations

New behavior should include tests at the smallest useful level:

- Unit tests for constructors, validation, and calculations.
- Broker tests for decisions and failure modes.
- Store contract tests for reservation, commit, refund, expiry, idempotency, and usage.
- Concurrency tests for atomic reservation behavior.
- Config parsing tests for valid and invalid policy files.

Coverage must remain above 90% line coverage.

## Security Expectations

- Never commit secrets, API keys, passwords, tokens, or real connection strings.
- Keep `.env` files out of git.
- Do not log secrets, credentials, tokens, or sensitive request metadata.
- Host applications own secret loading. This crate should receive already-created pools or non-sensitive policy data.
- Run `make security` before publishing or merging dependency changes.

## Documentation Expectations

Update documentation when public behavior changes:

- `README.md` for user-facing setup and examples.
- `src/lib.rs` for crate-level rustdoc.
- `CHANGELOG.md` for release-visible changes.
- `dev-docs/implementation-plan.md` when phase scope or accepted deferrals change.

## Pull Request Checklist

- `make dev` passes.
- `cargo doc --all-features --no-deps` passes.
- `cargo publish --dry-run --allow-dirty` passes for release-related changes.
- Public API changes are documented.
- `CHANGELOG.md` is updated.
- No unrelated formatting churn or refactors are included.
