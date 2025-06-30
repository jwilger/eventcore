# Schema Evolution Guide

This guide covers best practices for evolving event schemas in EventCore while maintaining backward compatibility and data integrity.

## Overview

Schema evolution is the process of changing event structures over time while ensuring that existing data remains accessible and that applications can continue to function correctly. EventCore provides comprehensive tools to handle schema evolution safely and efficiently.

## Core Concepts

### Event Versioning

Every event type should implement the `VersionedEvent` trait to enable automatic version tracking:

```rust
use eventcore::serialization::VersionedEvent;

#[derive(Serialize, Deserialize)]
pub struct UserRegistered {
    pub user_id: String,
    pub email: String,
    pub first_name: String,
    pub last_name: String,
}

impl VersionedEvent for UserRegistered {
    const CURRENT_VERSION: u32 = 2;
    const TYPE_NAME: &'static str = "UserRegistered";
    
    fn validate_schema(&self) -> Result<(), EventStoreError> {
        if self.user_id.is_empty() {
            return Err(EventStoreError::ValidationFailed("User ID cannot be empty".to_string()));
        }
        if self.email.is_empty() {
            return Err(EventStoreError::ValidationFailed("Email cannot be empty".to_string()));
        }
        Ok(())
    }
}
```

### Migration Functions

Migration functions transform event data from one schema version to another:

```rust
use eventcore::serialization::evolution::helpers;
use serde_json::json;

// Migration from version 1 to version 2: split full_name into first_name and last_name
let migration_1_to_2 = helpers::split_field(
    "full_name",
    &["first_name", "last_name"],
    helpers::splitters::split_full_name,
);

// Register the migration
registry.register_migration(
    "UserRegistered".to_string(),
    1,
    2,
    migration_1_to_2,
);
```

### Compatibility Levels

EventCore classifies schema changes into four compatibility levels:

1. **Fully Compatible**: No migration needed, all versions can read all data
2. **Forward Compatible**: Old readers can read new data
3. **Backward Compatible**: New readers can read old data
4. **Incompatible**: Manual migration required

## Best Practices

### 1. Design for Evolution

#### Use Optional Fields
Add new fields as optional whenever possible:

```rust
// Good: New field is optional
#[derive(Serialize, Deserialize)]
pub struct UserRegisteredV2 {
    pub user_id: String,
    pub email: String,
    pub first_name: String,
    pub last_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phone: Option<String>, // New optional field
}
```

#### Provide Default Values
When adding required fields, always provide sensible defaults:

```rust
// Migration that adds a required field with default
let migration = helpers::add_field("status", json!("active"));
```

#### Avoid Breaking Changes
Never remove required fields or change field types in incompatible ways:

```rust
// Bad: Removing a required field
// pub struct UserRegistered {
//     // pub user_id: String, // Removed - breaks old readers!
//     pub email: String,
// }

// Good: Keep the field but mark as deprecated
#[derive(Serialize, Deserialize)]
pub struct UserRegistered {
    #[deprecated = "Use new_user_id instead"]
    pub user_id: String,
    pub new_user_id: UserId, // New typed ID
    pub email: String,
}
```

### 2. Migration Strategies

#### Simple Additive Changes
For backward-compatible changes, use simple field additions:

```rust
// Add multiple fields with defaults
let migration = helpers::add_fields(&[
    ("created_at", json!("2024-01-01T00:00:00Z")),
    ("status", json!("active")),
    ("preferences", json!({})),
]);
```

#### Field Restructuring
For complex changes, use multi-step migrations:

```rust
// Step 1: Rename fields
let step1 = helpers::rename_fields(&[
    ("user_name", "username"),
    ("user_email", "email"),
]);

// Step 2: Nest related fields
let step2 = helpers::nest_fields("profile", &[
    ("first_name", "first_name"),
    ("last_name", "last_name"),
    ("avatar_url", "avatar_url"),
]);

// Step 3: Add version field
let step3 = helpers::add_field("schema_version", json!(3));

// Register as separate migrations or combine
registry.register_migration("UserRegistered".to_string(), 2, 3, step1);
registry.register_migration("UserRegistered".to_string(), 3, 4, step2);
registry.register_migration("UserRegistered".to_string(), 4, 5, step3);
```

#### Type Conversions
Handle type changes with converters:

```rust
// Convert string IDs to structured IDs
let id_converter = |value: Value| -> Result<Value, EventStoreError> {
    if let Value::String(id_str) = value {
        Ok(json!({
            "value": id_str,
            "type": "user",
            "version": 1
        }))
    } else {
        Ok(value)
    }
};

let migration = helpers::convert_field_type("user_id", id_converter);
```

### 3. Forward Compatibility

Configure your evolution strategy to handle future versions:

```rust
use eventcore::serialization::{EvolutionStrategy, ForwardCompatibilityMode};

let strategy = EvolutionStrategy {
    forward_compatibility: ForwardCompatibilityMode::Preserve,
    validate_after_migration: true,
    max_migration_steps: 5,
    enable_migration_cache: true,
};

let registry = EnhancedSchemaRegistry::with_strategy(strategy);
```

#### Forward Compatibility Modes

1. **Strict**: Reject events with future versions (default)
2. **Preserve**: Keep unknown events as `UnknownEvent` for later processing
3. **Downgrade**: Attempt reverse migration if available

### 4. Testing Migration Strategies

Always test your migrations thoroughly:

```rust
#[cfg(test)]
mod migration_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_user_registered_v1_to_v2_migration() {
        // Old version 1 data
        let v1_data = json!({
            "user_id": "user-123",
            "email": "john@example.com",
            "full_name": "John Doe"
        });

        // Apply migration
        let migration = get_user_registered_migration(1, 2);
        let v2_data = migration(v1_data).unwrap();

        // Verify new structure
        assert_eq!(v2_data["user_id"], "user-123");
        assert_eq!(v2_data["email"], "john@example.com");
        assert_eq!(v2_data["first_name"], "John");
        assert_eq!(v2_data["last_name"], "Doe");
        assert!(v2_data.get("full_name").is_none());
    }

    #[test]
    fn test_migration_chain() {
        let v1_data = json!({"old_field": "value"});
        
        // Test full migration chain from v1 to v3
        let mut registry = EnhancedSchemaRegistry::new();
        register_all_migrations(&mut registry);
        
        let path = registry.find_migration_path("UserRegistered", 1, 3)
            .await
            .unwrap()
            .unwrap();
        
        let result = registry.apply_migration_path(
            &serde_json::to_vec(&v1_data).unwrap(),
            &path
        ).await.unwrap();
        
        let final_data: Value = serde_json::from_slice(&result).unwrap();
        // Verify final structure matches v3 expectations
    }
}
```

## Common Migration Patterns

### 1. Adding Fields

```rust
// Add single field
let migration = helpers::add_field("new_field", json!("default_value"));

// Add multiple fields
let migration = helpers::add_fields(&[
    ("field1", json!("default1")),
    ("field2", json!(42)),
    ("field3", json!([])),
]);
```

### 2. Removing Fields

```rust
// Remove single field
let migration = helpers::remove_field("deprecated_field");

// Remove multiple fields
let migration = helpers::remove_fields(&["old_field1", "old_field2"]);
```

### 3. Renaming Fields

```rust
// Rename single field
let migration = helpers::rename_field("old_name", "new_name");

// Rename multiple fields
let migration = helpers::rename_fields(&[
    ("old_name1", "new_name1"),
    ("old_name2", "new_name2"),
]);
```

### 4. Restructuring Data

```rust
// Flatten nested structure
let migration = helpers::flatten_fields("user_profile", "user");
// {"user_profile": {"name": "John"}} -> {"user_name": "John"}

// Create nested structure
let migration = helpers::nest_fields("profile", &[
    ("first_name", "first_name"),
    ("last_name", "last_name"),
]);
// {"first_name": "John", "last_name": "Doe"} -> {"profile": {"first_name": "John", "last_name": "Doe"}}
```

### 5. Type Conversions

```rust
// String to number
let migration = helpers::convert_field_type("age", helpers::converters::string_to_number);

// Number to string
let migration = helpers::convert_field_type("id", helpers::converters::number_to_string);

// Boolean to string
let migration = helpers::convert_field_type("active", helpers::converters::boolean_to_string);

// Custom conversion
let custom_converter = |value: Value| -> Result<Value, EventStoreError> {
    match value {
        Value::String(s) => Ok(json!({"value": s, "type": "legacy"})),
        _ => Ok(value),
    }
};
let migration = helpers::convert_field_type("legacy_field", custom_converter);
```

### 6. Array Transformations

```rust
// Transform array elements
let transform_items = |items: Vec<Value>| -> Result<Vec<Value>, EventStoreError> {
    items.into_iter().map(|item| {
        if let Value::String(s) = item {
            Ok(json!({"name": s, "active": true}))
        } else {
            Ok(item)
        }
    }).collect()
};

let migration = helpers::transform_array_field("items", transform_items);
```

## Compatibility Validation

Use the compatibility validator to analyze schema changes:

```rust
use eventcore::serialization::{CompatibilityValidator, CompatibilityLevel};

// Extract schemas from sample data
let old_data = json!({"id": "123", "name": "John"});
let new_data = json!({"id": "123", "full_name": "John Doe", "email": "john@example.com"});

let old_schema = CompatibilityValidator::extract_schema(&old_data)?;
let new_schema = CompatibilityValidator::extract_schema(&new_data)?;

// Analyze compatibility
let analysis = CompatibilityValidator::analyze_compatibility(&old_schema, &new_schema);

match analysis.level {
    CompatibilityLevel::FullyCompatible => {
        println!("✅ Changes are fully compatible");
    }
    CompatibilityLevel::BackwardCompatible => {
        println!("⬅️ Changes are backward compatible");
        if let Some(migration) = analysis.migration {
            // Use generated migration
        }
    }
    CompatibilityLevel::ForwardCompatible => {
        println!("➡️ Changes are forward compatible");
    }
    CompatibilityLevel::Incompatible => {
        println!("❌ Breaking changes detected:");
        for issue in &analysis.issues {
            println!("  - {}", issue);
        }
        // Manual migration required
    }
}
```

## Migration Builder

For complex scenarios, use the migration builder:

```rust
use eventcore::serialization::MigrationBuilder;

let migration = MigrationBuilder::new()
    .rename_field("user_name", "username")
    .add_field("created_at", json!("2024-01-01T00:00:00Z"))
    .convert_field_type("age", helpers::converters::string_to_number)
    .add_custom_migration(|mut value| {
        // Custom logic for complex transformations
        if let Value::Object(ref mut map) = value {
            if let Some(username) = map.get("username").cloned() {
                map.insert("display_name".to_string(), username);
            }
        }
        Ok(value)
    })
    .build();
```

## Error Handling

Handle migration errors gracefully:

```rust
match evolution.migrate(&data, "UserRegistered", 1, 3).await {
    Ok(migrated_data) => {
        // Migration successful
        let event: UserRegistered = serde_json::from_slice(&migrated_data)?;
        // Validate the migrated event
        event.validate_schema()?;
    }
    Err(EventStoreError::SchemaEvolutionError(msg)) => {
        log::error!("Migration failed: {}", msg);
        // Handle migration failure - perhaps store as UnknownEvent
    }
    Err(e) => {
        log::error!("Unexpected error during migration: {}", e);
        return Err(e);
    }
}
```

## Performance Considerations

### Migration Caching

Enable migration caching for better performance:

```rust
let strategy = EvolutionStrategy {
    enable_migration_cache: true,
    ..Default::default()
};
```

### Batch Migrations

For large datasets, consider batch processing:

```rust
async fn migrate_events_batch(
    events: Vec<StoredEvent<Value>>,
    evolution: &EnhancedJsonSchemaEvolution,
) -> Result<Vec<StoredEvent<Value>>, EventStoreError> {
    let mut migrated = Vec::new();
    
    for event in events {
        let migrated_data = evolution.migrate(
            &serde_json::to_vec(&event.payload())?,
            event.type_name(),
            event.schema_version(),
            CURRENT_VERSION,
        ).await?;
        
        let migrated_payload = serde_json::from_slice(&migrated_data)?;
        migrated.push(StoredEvent::new(
            Event::new(event.stream_id().clone(), migrated_payload, event.metadata().clone()),
            event.version
        ));
    }
    
    Ok(migrated)
}
```

## Monitoring and Alerting

Track migration metrics:

```rust
use eventcore::monitoring::metrics;

// Record migration events
metrics::counter!("schema_migration.applied")
    .increment(&[
        ("type", event_type),
        ("from_version", &from_version.to_string()),
        ("to_version", &to_version.to_string()),
    ]);

// Track migration performance
let start = std::time::Instant::now();
let result = evolution.migrate(data, type_name, from_version, to_version).await;
let duration = start.elapsed();

metrics::histogram!("schema_migration.duration_ms")
    .record(duration.as_millis() as f64, &[
        ("type", event_type),
        ("success", &result.is_ok().to_string()),
    ]);
```

## Troubleshooting

### Common Issues

1. **Migration Path Not Found**
   ```
   Error: No migration path found from version 1 to 4
   ```
   **Solution**: Register intermediate migrations (1→2, 2→3, 3→4)

2. **Type Conversion Failures**
   ```
   Error: Cannot convert string 'invalid' to number
   ```
   **Solution**: Add validation in migration functions or use fallback values

3. **Exceeds Maximum Migration Steps**
   ```
   Error: Migration path exceeds maximum steps (10)
   ```
   **Solution**: Increase `max_migration_steps` or create more direct migration paths

### Debugging Migrations

Enable detailed logging:

```rust
// Set log level to debug
env::set_var("RUST_LOG", "eventcore::serialization=debug");

// Log migration steps
log::debug!("Applying migration from {} to {} for type {}", 
    from_version, to_version, type_name);
```

## Deployment Strategies

### Rolling Deployments

1. **Deploy new code with migration support** (can read old and new formats)
2. **Run background migration process** to update stored events
3. **Update producers** to emit new format
4. **Clean up old migration code** after verification

### Blue-Green Deployments

1. **Deploy to green environment** with migration support
2. **Test with production data copy** 
3. **Switch traffic** after validation
4. **Keep blue environment** as fallback during migration period

## Conclusion

Schema evolution is critical for long-term maintainability of event-sourced systems. By following these best practices and using EventCore's evolution tools, you can evolve your event schemas safely while maintaining data integrity and system compatibility.

Key takeaways:
- Always version your events using the `VersionedEvent` trait
- Prefer additive changes with optional fields and defaults
- Test migrations thoroughly with real data
- Use compatibility validation to catch breaking changes early
- Monitor migration performance and failures in production
- Plan deployment strategies that account for schema evolution