# EventCore Development Commands
# Run `just --list` to see all available commands

# Default command - show available commands
default:
    @just --list

# Run all tests with nextest
test:
    cargo nextest run --workspace

# Run all tests with standard cargo test (fallback)
test-fallback:
    cargo test --workspace

# Run tests with coverage
test-coverage:
    cargo llvm-cov --workspace --html
    @echo "Coverage report generated at target/llvm-cov/html/index.html"

# Run a specific test
test-one TEST:
    cargo nextest run {{TEST}}

# Run tests continuously on file changes
test-watch:
    cargo watch -x "nextest run --workspace"

# Format code
fmt:
    cargo fmt --all

# Check formatting without modifying files
fmt-check:
    cargo fmt --all -- --check

# Run clippy linter
lint:
    cargo clippy --workspace --all-targets -- -D warnings

# Type check all targets
check:
    cargo check --all-targets

# Run all CI checks locally
ci: fmt-check lint test check
    @echo "All CI checks passed!"

# Build release version
build:
    cargo build --release

# Build and open documentation
docs:
    cargo doc --workspace --no-deps --open

# Clean build artifacts
clean:
    cargo clean

# Update dependencies
update:
    cargo update

# Check for outdated dependencies
outdated:
    cargo outdated --workspace

# Run security audit
audit:
    cargo audit

# Start PostgreSQL databases
db-up:
    docker-compose up -d

# Stop PostgreSQL databases
db-down:
    docker-compose down

# Connect to main database
db-connect:
    psql -h localhost -p 5432 -U postgres -d eventcore

# Connect to test database
db-connect-test:
    psql -h localhost -p 5433 -U postgres -d eventcore_test

# Install development tools
install-tools:
    cargo install cargo-nextest --locked
    cargo install cargo-llvm-cov --locked
    cargo install cargo-outdated --locked
    cargo install cargo-audit --locked
    cargo install cargo-watch --locked

# Run benchmarks
bench:
    cargo bench --workspace

# Generate a new migration (PostgreSQL)
migration-new NAME:
    cd eventcore-postgres && sqlx migrate add {{NAME}}

# Run migrations (PostgreSQL)
migration-run:
    cd eventcore-postgres && sqlx migrate run

# Revert last migration (PostgreSQL)
migration-revert:
    cd eventcore-postgres && sqlx migrate revert

# Pre-commit checks (mimics git pre-commit hook)
pre-commit: fmt check test lint
    @echo "Pre-commit checks passed!"

# Watch and run a specific command on file changes
watch CMD:
    cargo watch -x "{{CMD}}"