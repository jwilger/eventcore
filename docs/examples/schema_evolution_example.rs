//! Schema Evolution Example
//!
//! This example demonstrates how to evolve event schemas over time using EventCore's
//! schema evolution system. It shows:
//!
//! 1. Creating versioned events
//! 2. Registering migrations between versions
//! 3. Automatically migrating old event data
//! 4. Handling compatibility validation
//! 5. Using migration helpers for common patterns

use eventcore::serialization::{
    CompatibilityValidator, EnhancedJsonSchemaEvolution, EnhancedSchemaRegistry,
    EvolutionStrategy, ForwardCompatibilityMode, MigrationBuilder, VersionedEvent,
    evolution::helpers,
};
use eventcore::errors::EventStoreError;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::sync::RwLock;

// Version 1: Original user registration event
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UserRegisteredV1 {
    pub user_id: String,
    pub email: String,
    pub full_name: String,
    pub created_at: String,
}

// Version 2: Split name into first and last name
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UserRegisteredV2 {
    pub user_id: String,
    pub email: String,
    pub first_name: String,
    pub last_name: String,
    pub created_at: String,
}

// Version 3: Add structured user profile and status
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UserRegisteredV3 {
    pub user_id: String,
    pub email: String,
    pub profile: UserProfile,
    pub status: UserStatus,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UserProfile {
    pub first_name: String,
    pub last_name: String,
    pub display_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum UserStatus {
    Active,
    Pending,
    Suspended,
}

impl VersionedEvent for UserRegisteredV3 {
    const CURRENT_VERSION: u32 = 3;
    const TYPE_NAME: &'static str = "UserRegistered";

    fn validate_schema(&self) -> Result<(), EventStoreError> {
        if self.user_id.is_empty() {
            return Err(EventStoreError::DeserializationFailed("User ID cannot be empty".to_string()));
        }
        if self.email.is_empty() {
            return Err(EventStoreError::DeserializationFailed("Email cannot be empty".to_string()));
        }
        if self.profile.first_name.is_empty() {
            return Err(EventStoreError::DeserializationFailed("First name cannot be empty".to_string()));
        }
        Ok(())
    }
}

/// Sets up the schema evolution registry with all migrations
async fn setup_evolution_system() -> Arc<RwLock<EnhancedSchemaRegistry>> {
    let mut registry = EnhancedSchemaRegistry::with_strategy(EvolutionStrategy {
        forward_compatibility: ForwardCompatibilityMode::Preserve,
        validate_after_migration: true,
        max_migration_steps: 5,
        enable_migration_cache: true,
    });

    // Register the current event type
    registry.register_versioned_event::<UserRegisteredV3>();

    // Migration from V1 to V2: Split full_name into first_name and last_name
    let migration_v1_to_v2 = helpers::split_field(
        "full_name",
        &["first_name", "last_name"],
        helpers::splitters::split_full_name,
    );

    registry.register_migration(
        "UserRegistered".to_string(),
        1,
        2,
        migration_v1_to_v2,
    );

    // Migration from V2 to V3: Create nested profile structure and add status
    let migration_v2_to_v3 = MigrationBuilder::new()
        // First, create display_name from first and last name
        .add_custom_migration(|mut value| {
            if let Value::Object(ref mut map) = value {
                let first_name = map.get("first_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let last_name = map.get("last_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

                let display_name = if first_name.is_empty() && last_name.is_empty() {
                    "Unknown User".to_string()
                } else if last_name.is_empty() {
                    first_name
                } else if first_name.is_empty() {
                    last_name
                } else {
                    format!("{} {}", first_name, last_name)
                };

                map.insert("display_name".to_string(), json!(display_name));
            }
            Ok(value)
        })
        // Then nest the profile fields
        .add_custom_migration(|mut value| {
            if let Value::Object(ref mut map) = value {
                let profile = json!({
                    "first_name": map.remove("first_name").unwrap_or(json!("")),
                    "last_name": map.remove("last_name").unwrap_or(json!("")),
                    "display_name": map.remove("display_name").unwrap_or(json!("Unknown User"))
                });
                map.insert("profile".to_string(), profile);
            }
            Ok(value)
        })
        // Add default status
        .add_field("status", json!("Active"))
        .build();

    registry.register_migration(
        "UserRegistered".to_string(),
        2,
        3,
        migration_v2_to_v3,
    );

    Arc::new(RwLock::new(registry))
}

/// Demonstrates migrating from V1 to current version
async fn demonstrate_v1_migration(evolution: &EnhancedJsonSchemaEvolution) -> Result<(), EventStoreError> {
    println!("=== Migrating V1 Event ===");

    // Original V1 event data
    let v1_event = UserRegisteredV1 {
        user_id: "user-123".to_string(),
        email: "john.doe@example.com".to_string(),
        full_name: "John Doe".to_string(),
        created_at: "2024-01-15T10:30:00Z".to_string(),
    };

    println!("Original V1 event: {}", serde_json::to_string_pretty(&v1_event)?);

    // Serialize as V1 would have been stored
    let v1_data = serde_json::to_vec(&v1_event)?;

    // Migrate to current version (V3)
    let migrated_data = evolution.migrate(&v1_data, "UserRegistered", 1, 3).await?;

    // Deserialize as current version
    let v3_event: UserRegisteredV3 = serde_json::from_slice(&migrated_data)?;

    println!("Migrated to V3: {}", serde_json::to_string_pretty(&v3_event)?);

    // Validate the migrated event
    v3_event.validate_schema()?;
    println!("✅ Migrated event passes validation");

    Ok(())
}

/// Demonstrates migrating from V2 to current version
async fn demonstrate_v2_migration(evolution: &EnhancedJsonSchemaEvolution) -> Result<(), EventStoreError> {
    println!("\n=== Migrating V2 Event ===");

    // V2 event data
    let v2_event = UserRegisteredV2 {
        user_id: "user-456".to_string(),
        email: "jane.smith@example.com".to_string(),
        first_name: "Jane".to_string(),
        last_name: "Smith".to_string(),
        created_at: "2024-02-20T14:45:00Z".to_string(),
    };

    println!("Original V2 event: {}", serde_json::to_string_pretty(&v2_event)?);

    // Serialize as V2 would have been stored
    let v2_data = serde_json::to_vec(&v2_event)?;

    // Migrate to current version (V3)
    let migrated_data = evolution.migrate(&v2_data, "UserRegistered", 2, 3).await?;

    // Deserialize as current version
    let v3_event: UserRegisteredV3 = serde_json::from_slice(&migrated_data)?;

    println!("Migrated to V3: {}", serde_json::to_string_pretty(&v3_event)?);

    // Validate the migrated event
    v3_event.validate_schema()?;
    println!("✅ Migrated event passes validation");

    Ok(())
}

/// Demonstrates compatibility analysis between versions
fn demonstrate_compatibility_analysis() -> Result<(), EventStoreError> {
    println!("\n=== Compatibility Analysis ===");

    // Sample data for each version
    let v1_sample = json!({
        "user_id": "user-123",
        "email": "john@example.com",
        "full_name": "John Doe",
        "created_at": "2024-01-01T00:00:00Z"
    });

    let v2_sample = json!({
        "user_id": "user-123",
        "email": "john@example.com",
        "first_name": "John",
        "last_name": "Doe",
        "created_at": "2024-01-01T00:00:00Z"
    });

    let v3_sample = json!({
        "user_id": "user-123",
        "email": "john@example.com",
        "profile": {
            "first_name": "John",
            "last_name": "Doe",
            "display_name": "John Doe"
        },
        "status": "Active",
        "created_at": "2024-01-01T00:00:00Z"
    });

    // Extract schemas
    let v1_schema = CompatibilityValidator::extract_schema(&v1_sample)?;
    let v2_schema = CompatibilityValidator::extract_schema(&v2_sample)?;
    let v3_schema = CompatibilityValidator::extract_schema(&v3_sample)?;

    // Analyze V1 -> V2 compatibility
    let v1_to_v2_analysis = CompatibilityValidator::analyze_compatibility(&v1_schema, &v2_schema);
    println!("V1 -> V2 Compatibility: {:?}", v1_to_v2_analysis.level);
    println!("Changes: {} detected", v1_to_v2_analysis.changes.len());
    println!("Auto-migration possible: {}", v1_to_v2_analysis.auto_migration_possible);

    // Analyze V2 -> V3 compatibility
    let v2_to_v3_analysis = CompatibilityValidator::analyze_compatibility(&v2_schema, &v3_schema);
    println!("V2 -> V3 Compatibility: {:?}", v2_to_v3_analysis.level);
    println!("Changes: {} detected", v2_to_v3_analysis.changes.len());
    println!("Auto-migration possible: {}", v2_to_v3_analysis.auto_migration_possible);

    if !v2_to_v3_analysis.issues.is_empty() {
        println!("Issues preventing auto-migration:");
        for issue in &v2_to_v3_analysis.issues {
            println!("  - {}", issue);
        }
    }

    Ok(())
}

/// Demonstrates handling unknown/future events
async fn demonstrate_forward_compatibility(evolution: &EnhancedJsonSchemaEvolution) -> Result<(), EventStoreError> {
    println!("\n=== Forward Compatibility ===");

    // Simulated "future" event with unknown fields
    let future_event = json!({
        "user_id": "user-789",
        "email": "future@example.com",
        "profile": {
            "first_name": "Future",
            "last_name": "User",
            "display_name": "Future User"
        },
        "status": "Active",
        "created_at": "2024-03-01T12:00:00Z",
        "unknown_field": "some future data",
        "new_feature": {
            "setting1": true,
            "setting2": "advanced"
        }
    });

    println!("Future event with unknown fields: {}", serde_json::to_string_pretty(&future_event)?);

    let future_data = serde_json::to_vec(&future_event)?;

    // Try to handle the future event (version 5 -> version 3)
    match evolution.handle_forward_compatibility(&future_data, "UserRegistered", 5, 3).await {
        Ok(preserved_data) => {
            println!("✅ Future event preserved successfully");
            let preserved_event: Value = serde_json::from_slice(&preserved_data)?;
            println!("Preserved data: {}", serde_json::to_string_pretty(&preserved_event)?);
        }
        Err(e) => {
            println!("❌ Failed to handle future event: {}", e);
        }
    }

    Ok(())
}

/// Demonstrates performance characteristics
async fn demonstrate_performance(evolution: &EnhancedJsonSchemaEvolution) -> Result<(), EventStoreError> {
    println!("\n=== Performance Testing ===");

    let v1_event = json!({
        "user_id": "user-perf",
        "email": "perf@example.com",
        "full_name": "Performance Test",
        "created_at": "2024-01-01T00:00:00Z"
    });

    let v1_data = serde_json::to_vec(&v1_event)?;

    // First migration (cold cache)
    let start = std::time::Instant::now();
    let _result1 = evolution.migrate(&v1_data, "UserRegistered", 1, 3).await?;
    let first_duration = start.elapsed();

    // Second migration (warm cache)
    let start = std::time::Instant::now();
    let _result2 = evolution.migrate(&v1_data, "UserRegistered", 1, 3).await?;
    let second_duration = start.elapsed();

    println!("First migration (cold cache): {:?}", first_duration);
    println!("Second migration (warm cache): {:?}", second_duration);

    if second_duration < first_duration {
        println!("✅ Cache is working - second migration was faster");
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    env_logger::init();

    println!("EventCore Schema Evolution Example");
    println!("==================================");

    // Set up the evolution system
    let registry = setup_evolution_system().await;
    let evolution = EnhancedJsonSchemaEvolution::new(registry);

    // Run demonstrations
    demonstrate_v1_migration(&evolution).await?;
    demonstrate_v2_migration(&evolution).await?;
    demonstrate_compatibility_analysis()?;
    demonstrate_forward_compatibility(&evolution).await?;
    demonstrate_performance(&evolution).await?;

    println!("\n✅ All demonstrations completed successfully!");
    println!("\nKey takeaways:");
    println!("1. Events can be safely migrated between versions");
    println!("2. Compatibility analysis helps prevent breaking changes");
    println!("3. Forward compatibility preserves unknown data");
    println!("4. Migration caching improves performance");
    println!("5. Validation ensures data integrity after migration");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_complete_migration_chain() {
        let registry = setup_evolution_system().await;
        let evolution = EnhancedJsonSchemaEvolution::new(registry);

        // Test V1 -> V3 migration
        let v1_data = json!({
            "user_id": "test-user",
            "email": "test@example.com",
            "full_name": "Test User",
            "created_at": "2024-01-01T00:00:00Z"
        });

        let v1_bytes = serde_json::to_vec(&v1_data).unwrap();
        let migrated_bytes = evolution.migrate(&v1_bytes, "UserRegistered", 1, 3).await.unwrap();
        let migrated_event: UserRegisteredV3 = serde_json::from_slice(&migrated_bytes).unwrap();

        assert_eq!(migrated_event.user_id, "test-user");
        assert_eq!(migrated_event.email, "test@example.com");
        assert_eq!(migrated_event.profile.first_name, "Test");
        assert_eq!(migrated_event.profile.last_name, "User");
        assert_eq!(migrated_event.profile.display_name, "Test User");
        assert_eq!(migrated_event.status, UserStatus::Active);

        // Validate the migrated event
        migrated_event.validate_schema().unwrap();
    }

    #[tokio::test]
    async fn test_v2_to_v3_migration() {
        let registry = setup_evolution_system().await;
        let evolution = EnhancedJsonSchemaEvolution::new(registry);

        let v2_data = json!({
            "user_id": "test-user-2",
            "email": "test2@example.com",
            "first_name": "Jane",
            "last_name": "Doe",
            "created_at": "2024-02-01T00:00:00Z"
        });

        let v2_bytes = serde_json::to_vec(&v2_data).unwrap();
        let migrated_bytes = evolution.migrate(&v2_bytes, "UserRegistered", 2, 3).await.unwrap();
        let migrated_event: UserRegisteredV3 = serde_json::from_slice(&migrated_bytes).unwrap();

        assert_eq!(migrated_event.profile.first_name, "Jane");
        assert_eq!(migrated_event.profile.last_name, "Doe");
        assert_eq!(migrated_event.profile.display_name, "Jane Doe");
        assert_eq!(migrated_event.status, UserStatus::Active);
    }

    #[test]
    fn test_versioned_event_trait() {
        assert_eq!(UserRegisteredV3::CURRENT_VERSION, 3);
        assert_eq!(UserRegisteredV3::TYPE_NAME, "UserRegistered");

        let valid_event = UserRegisteredV3 {
            user_id: "valid-user".to_string(),
            email: "valid@example.com".to_string(),
            profile: UserProfile {
                first_name: "Valid".to_string(),
                last_name: "User".to_string(),
                display_name: "Valid User".to_string(),
            },
            status: UserStatus::Active,
            created_at: "2024-01-01T00:00:00Z".to_string(),
        };

        assert!(valid_event.validate_schema().is_ok());

        let invalid_event = UserRegisteredV3 {
            user_id: String::new(), // Invalid: empty user_id
            email: "valid@example.com".to_string(),
            profile: UserProfile {
                first_name: "Valid".to_string(),
                last_name: "User".to_string(),
                display_name: "Valid User".to_string(),
            },
            status: UserStatus::Active,
            created_at: "2024-01-01T00:00:00Z".to_string(),
        };

        assert!(invalid_event.validate_schema().is_err());
    }
}
