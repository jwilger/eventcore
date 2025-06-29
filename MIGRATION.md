# EventCore Migration Guide

This document provides comprehensive migration guidance for upgrading between major versions of EventCore.

## Table of Contents

- [General Migration Strategy](#general-migration-strategy)
- [Version 0.1.x to 1.0.0](#version-01x-to-100)
- [Future Version Migrations](#future-version-migrations)
- [Database Migrations](#database-migrations)
- [Testing Your Migration](#testing-your-migration)
- [Common Issues and Solutions](#common-issues-and-solutions)

## General Migration Strategy

### Before You Start

1. **Review the CHANGELOG**: Understand all breaking changes
2. **Check MSRV**: Ensure your Rust version is compatible
3. **Backup Data**: Create backups of event stores and projections
4. **Test Environment**: Perform migration in test environment first
5. **Read Dependencies**: Check if your dependencies support the new version

### Migration Process

1. **Update Dependencies**: Update EventCore versions in Cargo.toml
2. **Fix Compilation Errors**: Address API changes
3. **Update Tests**: Ensure tests work with new APIs
4. **Migrate Data**: Run any required data migrations
5. **Update Documentation**: Update your code documentation
6. **Performance Testing**: Verify performance characteristics

### Version Compatibility Table

| Your Version | Can Upgrade To | Migration Complexity |
|--------------|----------------|---------------------|
| 0.1.x | 1.0.0 | Major - API changes |
| 1.x.x | 1.y.z (y > x) | Minor - mostly compatible |
| 1.x.x | 2.0.0 | Major - breaking changes |

## Version 0.1.x to 1.0.0

> **Note**: As this is the initial release (0.1.0), this section serves as a template for future migrations and documents potential breaking changes that might occur.

### Overview

The migration from 0.1.x to 1.0.0 focuses on API stabilization and may include:

- Command trait signature changes
- Event serialization format improvements
- Database schema optimizations
- Enhanced type safety

### Breaking Changes

#### 1. Command Trait Changes

**What Changed**: The Command trait may gain additional parameters for better stream management.

**Before (0.1.x)**:
```rust
#[async_trait]
impl Command for TransferMoney {
    type Input = TransferMoneyInput;
    type State = AccountState;
    type Event = BankEvent;
    type StreamSet = ();

    fn read_streams(&self, input: &Self::Input) -> Vec<StreamId> {
        vec![input.from_account.clone(), input.to_account.clone()]
    }

    fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
        // Apply logic
    }

    async fn handle(
        &self,
        read_streams: ReadStreams<Self::StreamSet>,
        state: Self::State,
        input: Self::Input,
        stream_resolver: &mut StreamResolver,
    ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
        // Handle logic
    }
}
```

**After (1.0.0)** (hypothetical):
```rust
#[async_trait]
impl Command for TransferMoney {
    type Input = TransferMoneyInput;
    type State = AccountState;
    type Event = BankEvent;
    type StreamSet = TransferStreams; // More specific type

    fn read_streams(&self, input: &Self::Input) -> Vec<StreamId> {
        vec![input.from_account.clone(), input.to_account.clone()]
    }

    fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
        // Apply logic
    }

    async fn handle(
        &self,
        context: CommandContext<Self>, // New context parameter
        read_streams: ReadStreams<Self::StreamSet>,
        state: Self::State,
        input: Self::Input,
        stream_resolver: &mut StreamResolver,
    ) -> CommandResult<CommandOutput<Self::Event>> { // New output type
        // Handle logic with new context
    }
}
```

**Migration Steps**:
1. Update trait implementation signatures
2. Use new `CommandContext` parameter for enhanced functionality
3. Return `CommandOutput` instead of raw event vectors
4. Define specific `StreamSet` types instead of `()`

#### 2. Event Store Configuration

**What Changed**: Configuration structure may be enhanced for better ergonomics.

**Before (0.1.x)**:
```rust
let config = PostgresConfig {
    connection_string: "postgresql://...".to_string(),
    max_connections: 10,
    timeout: Duration::from_secs(30),
};
let store = PostgresEventStore::new(config).await?;
```

**After (1.0.0)** (hypothetical):
```rust
let store = PostgresEventStore::builder()
    .connection_string("postgresql://...")
    .max_connections(10)
    .timeout(Duration::from_secs(30))
    .enable_ssl(true) // New option
    .build()
    .await?;
```

**Migration Steps**:
1. Replace direct config struct with builder pattern
2. Review new configuration options
3. Update SSL and connection settings

#### 3. Error Handling Improvements

**What Changed**: Enhanced error types with better diagnostics.

**Before (0.1.x)**:
```rust
match result {
    Err(CommandError::BusinessRuleViolation(msg)) => {
        eprintln!("Business rule violation: {}", msg);
    }
    // Other error handling
}
```

**After (1.0.0)** (hypothetical):
```rust
match result {
    Err(CommandError::BusinessRuleViolation { rule, context, .. }) => {
        eprintln!("Business rule '{}' violated: {}", rule, context);
        // More structured error information
    }
    // Enhanced error variants
}
```

**Migration Steps**:
1. Update error handling patterns
2. Use structured error information
3. Leverage enhanced diagnostics

### Non-Breaking Changes

These changes are backward compatible but recommended:

#### 1. New Macro Syntax

**0.1.x Approach**:
```rust
struct MyCommand;

#[async_trait]
impl Command for MyCommand {
    // Full implementation
}
```

**1.0.0 Enhancement**:
```rust
#[derive(Command)]
struct MyCommand {
    #[stream]
    account_id: StreamId,
    amount: Money,
}

// Or use declarative macro
command! {
    name: MyCommand,
    reads: [account_stream],
    state: AccountState,
    event: AccountEvent,
    
    apply: |state, event| {
        // Event application logic
    },
    
    handle: |state, input| {
        // Command logic
        emit!(AccountEvent::MoneyDeposited { amount: input.amount })
    }
}
```

**Migration Benefits**:
- Reduced boilerplate code
- Better compile-time validation
- Cleaner code structure

#### 2. Enhanced Builder APIs

**0.1.x**:
```rust
let executor = CommandExecutor::new(event_store);
```

**1.0.0**:
```rust
let executor = CommandExecutor::builder()
    .with_store(event_store)
    .with_tracing(true)
    .with_retry_config(RetryConfig::fault_tolerant())
    .build();
```

### Database Migrations

#### Schema Changes

If database schema changes are required:

1. **Backup your database**:
```bash
pg_dump eventcore > backup.sql
```

2. **Run migration scripts**:
```bash
# Provided migration scripts
psql eventcore < migrations/0.1_to_1.0.sql
```

3. **Verify migration**:
```sql
-- Check schema version
SELECT version FROM schema_migrations ORDER BY version DESC LIMIT 1;
```

#### Data Format Changes

If event serialization format changes:

1. **Check compatibility**:
```rust
// Test reading existing events
let events = store.read_streams(stream_ids, ReadOptions::default()).await?;
for event in events {
    // Verify deserialization works
    let _: MyEvent = serde_json::from_value(event.data)?;
}
```

2. **Migrate incompatible events**:
```rust
// Migration tool (provided)
cargo run --bin migrate-events -- --dry-run
cargo run --bin migrate-events -- --execute
```

### Testing Your Migration

#### Automated Testing

1. **Create test with old data**:
```rust
#[test]
async fn test_migration_compatibility() {
    // Load events created with 0.1.x
    let old_events = load_test_events("v0.1_test_data.json");
    
    // Verify they work with 1.0.0
    let store = InMemoryEventStore::new();
    store.write_events_multi(old_events).await?;
    
    // Execute commands against migrated data
    let result = executor.execute(&MyCommand, input, ExecutionOptions::default()).await;
    assert!(result.is_ok());
}
```

2. **Performance regression tests**:
```rust
#[bench]
fn bench_command_execution_v1(b: &mut Bencher) {
    // Ensure performance isn't degraded
    b.iter(|| {
        // Command execution benchmark
    });
}
```

#### Manual Testing

1. **Test critical workflows**: Execute your most important business operations
2. **Verify projections**: Ensure read models rebuild correctly
3. **Check monitoring**: Verify metrics and health checks work
4. **Load testing**: Ensure performance meets requirements

### Rollback Plan

If issues are discovered after migration:

1. **Code rollback**:
```toml
[dependencies]
eventcore = "0.1.x"  # Revert to previous version
```

2. **Database rollback**:
```bash
# Restore from backup
psql eventcore < backup.sql
```

3. **Data rollback**:
```bash
# Revert data migrations if needed
psql eventcore < rollback/1.0_to_0.1.sql
```

## Future Version Migrations

This section will be updated as new versions are released.

### Planned Breaking Changes

Future versions may include:

- **2.0.0**: Enhanced projection system
- **3.0.0**: Event store plugin architecture
- **4.0.0**: Distributed event sourcing

## Database Migrations

### PostgreSQL Migrations

EventCore includes database migration tools:

#### Running Migrations

```bash
# Check current schema version
cargo run --bin eventcore-migrate -- --status

# Run all pending migrations
cargo run --bin eventcore-migrate -- --up

# Rollback specific migration
cargo run --bin eventcore-migrate -- --down <version>
```

#### Custom Migrations

For application-specific migrations:

```sql
-- migrations/custom/001_add_user_metadata.sql
ALTER TABLE events ADD COLUMN user_metadata JSONB;
CREATE INDEX idx_events_user_metadata ON events USING GIN (user_metadata);
```

#### Migration Safety

- **Transactional**: All migrations run in transactions
- **Rollback support**: Every migration has a rollback script
- **Validation**: Schema validation after each migration
- **Backup integration**: Automatic backups before major changes

## Testing Your Migration

### Pre-Migration Checklist

- [ ] Full database backup created
- [ ] Test environment migration successful
- [ ] All dependencies updated and compatible
- [ ] Performance benchmarks established
- [ ] Rollback plan documented and tested

### Post-Migration Validation

- [ ] All tests pass
- [ ] Critical workflows function correctly
- [ ] Performance meets requirements
- [ ] Monitoring and alerting operational
- [ ] Documentation updated

### Validation Tools

```rust
// Provided validation tools
cargo run --bin validate-migration

// Custom validation
cargo test migration_validation
```

## Common Issues and Solutions

### Issue: Compilation Errors After Upgrade

**Symptoms**: Code doesn't compile after version update

**Solutions**:
1. Check CHANGELOG for breaking changes
2. Update trait implementations
3. Fix deprecated API usage
4. Update macro syntax

### Issue: Performance Degradation

**Symptoms**: Commands execute slower after migration

**Solutions**:
1. Run performance benchmarks
2. Check for new configuration options
3. Verify database indices
4. Profile hot paths

### Issue: Event Deserialization Failures

**Symptoms**: Cannot read existing events

**Solutions**:
1. Run event format validation tool
2. Check for schema evolution configuration
3. Use migration tools for format changes
4. Verify type registry setup

### Issue: Database Connection Problems

**Symptoms**: Cannot connect to database after migration

**Solutions**:
1. Check connection string format
2. Verify database version compatibility
3. Review SSL/TLS settings
4. Check firewall and network configuration

## Getting Help

### Community Support

- **GitHub Issues**: Report bugs and ask questions
- **Discussions**: General help and best practices
- **Discord**: Real-time community support

### Professional Support

- **Consulting**: Migration assistance for complex scenarios
- **Training**: EventCore best practices and migration strategies
- **Custom Development**: Tailored solutions for specific needs

### Documentation

- **API Documentation**: Comprehensive rustdoc
- **Examples**: Working code examples
- **Tutorials**: Step-by-step guides
- **Best Practices**: Recommended patterns

---

Remember: When in doubt, test thoroughly in a non-production environment first!