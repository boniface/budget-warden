LLVM_COV ?= $(shell if command -v llvm-cov >/dev/null 2>&1; then command -v llvm-cov; elif [ -x /opt/homebrew/opt/llvm/bin/llvm-cov ]; then echo /opt/homebrew/opt/llvm/bin/llvm-cov; fi)
LLVM_PROFDATA ?= $(shell if command -v llvm-profdata >/dev/null 2>&1; then command -v llvm-profdata; elif [ -x /opt/homebrew/opt/llvm/bin/llvm-profdata ]; then echo /opt/homebrew/opt/llvm/bin/llvm-profdata; fi)
CARGO_AUDIT_DB ?= $(shell if [ -d "$$HOME/.cargo/advisory-db/.git" ]; then echo "$$HOME/.cargo/advisory-db"; else find "$$HOME/.cargo/advisory-db" -maxdepth 1 -mindepth 1 -type d -name 'advisory-db-*' 2>/dev/null | head -n 1; fi)
COVERAGE_ARGS ?= --all-features --ignore-filename-regex 'src/store/(postgres|redis_store)\.rs' --fail-under-lines 90

.PHONY: dev fmt clippy test live-integration coverage security doc

dev: fmt clippy test coverage security

fmt:
	cargo fmt --all

clippy:
	cargo clippy --all-targets --all-features -- -D warnings -W clippy::pedantic

test:
	cargo test --all-features

live-integration:
	BUDGET_WARDEN_REQUIRE_LIVE=1 cargo test --all-features --test store_contract

coverage:
	LLVM_COV="$(LLVM_COV)" LLVM_PROFDATA="$(LLVM_PROFDATA)" cargo llvm-cov $(COVERAGE_ARGS)

security:
	cargo audit $(if $(CARGO_AUDIT_DB),-d "$(CARGO_AUDIT_DB)",)
	cargo deny check

doc:
	cargo doc --all-features --no-deps
