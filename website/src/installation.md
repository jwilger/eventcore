# Installation

## Requirements

- Rust 1.70.0 or later
- PostgreSQL 13+ (for PostgreSQL backend)
- Tokio async runtime

## Adding EventCore to Your Project

Add the following to your `Cargo.toml`:

```toml
[dependencies]
# Core library
eventcore = "0.1"

# Choose your backend (one of these):
eventcore-postgres = "0.1"  # Production-ready PostgreSQL backend
eventcore-memory = "0.1"    # In-memory backend for development/testing

# Required dependencies
tokio = { version = "1", features = ["full"] }
async-trait = "0.1"
serde = { version = "1", features = ["derive"] }
uuid = { version = "1", features = ["v7", "serde"] }
thiserror = "1"

# Optional but recommended
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
```

## Backend Configuration

### PostgreSQL Backend

1. **Database Setup**

```bash
# Using Docker
docker run -d \
  --name eventcore-postgres \
  -e POSTGRES_PASSWORD=postgres \
  -e POSTGRES_DB=eventcore \
  -p 5432:5432 \
  postgres:15-alpine

# Or use the provided docker-compose.yml
docker-compose up -d
```

2. **Run Migrations**

EventCore will automatically create required tables on first use. For manual setup:

```sql
-- See eventcore-postgres/migrations/ for schema
```

3. **Connection Configuration**

```rust
use eventcore_postgres::PostgresEventStore;

let database_url = "postgres://postgres:postgres@localhost/eventcore";
let event_store = PostgresEventStore::new(database_url).await?;
```

### In-Memory Backend

Perfect for development and testing:

```rust
use eventcore_memory::InMemoryEventStore;

let event_store = InMemoryEventStore::new();
```

## Feature Flags

EventCore supports various feature flags:

```toml
[dependencies]
eventcore = { version = "0.1", features = ["full"] }

# Individual features:
# - "testing" - Testing utilities and fixtures
# - "chaos" - Chaos testing support
# - "monitoring" - OpenTelemetry integration
# - "cqrs" - CQRS pattern support
```

## Verification

Create a simple test to verify installation:

```rust
use eventcore::prelude::*;

#[tokio::test]
async fn test_eventcore_setup() {
    let event_store = eventcore_memory::InMemoryEventStore::new();
    let executor = CommandExecutor::new(event_store);
    
    // If this compiles, EventCore is properly installed!
    assert!(true);
}
```

## Next Steps

- Follow the [Quick Start Guide](./quickstart.html)
- Explore the [Examples](./examples/banking.html)
- Read about [Core Concepts](./manual/03-core-concepts/01-commands-and-macros.html)