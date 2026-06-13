# Changelog

All notable changes to this project will be documented in this file.

This project follows semantic versioning once releases are published. Until the first stable release, minor versions may include public API changes.

## [Unreleased]

## [0.1.3] - 2026-06-13

### Added

- Core budget broker API with `authorize`, `reserve`, `usage`, `policies`, and `report`.
- Budget policies with hard limits, calendar windows, pacing, emergency reserves, minimum remaining units, and priority-based reserved capacity.
- Type-safe request, key, reservation, usage, and error models.
- In-memory store for local development and tests.
- PostgreSQL, Redis, and SQLite store implementations behind feature flags.
- TOML policy parsing behind the `toml` feature.
- Tracing hooks behind the `tracing` feature.
- Shared store contract tests.
- CI workflow with formatting, pedantic clippy, tests, coverage, security checks, and documentation.
- Release workflow for crates.io publishing.

### Security

- Added `cargo audit` and `cargo deny` gates.
- Kept secret loading outside the library; host applications provide already-created stores or connection pools.
