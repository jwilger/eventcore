//! Schema evolution support for event versioning.

use super::SchemaEvolution;
use crate::errors::EventStoreError;
use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

/// Type alias for a migration function.
///
/// Migration functions take JSON data and transform it to a new schema version.
pub type MigrationFn = Arc<dyn Fn(Value) -> Result<Value, EventStoreError> + Send + Sync>;

/// A registry of schema migrations for different event types.
#[derive(Default)]
pub struct SchemaRegistry {
    /// Maps type names to their migration chains
    migrations: HashMap<String, MigrationChain>,
}

impl SchemaRegistry {
    /// Creates a new schema registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a migration for a specific event type.
    ///
    /// # Arguments
    ///
    /// * `type_name` - The fully qualified type name of the event
    /// * `from_version` - The source schema version
    /// * `to_version` - The target schema version
    /// * `migration` - The migration function
    pub fn register_migration(
        &mut self,
        type_name: String,
        from_version: u32,
        to_version: u32,
        migration: MigrationFn,
    ) {
        let chain = self.migrations.entry(type_name).or_default();
        chain.add_migration(from_version, to_version, migration);
    }

    /// Gets the migration chain for a type.
    pub fn get_chain(&self, type_name: &str) -> Option<&MigrationChain> {
        self.migrations.get(type_name)
    }
}

/// A chain of migrations for evolving an event through multiple schema versions.
#[derive(Default)]
pub struct MigrationChain {
    /// Maps from version to the migration that upgrades to the next version
    migrations: HashMap<u32, (u32, MigrationFn)>,
}

impl MigrationChain {
    /// Creates a new migration chain.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a migration to the chain.
    pub fn add_migration(&mut self, from_version: u32, to_version: u32, migration: MigrationFn) {
        self.migrations
            .insert(from_version, (to_version, migration));
    }

    /// Finds a path of migrations from one version to another.
    pub fn find_migration_path(&self, from: u32, to: u32) -> Option<Vec<(u32, u32, &MigrationFn)>> {
        if from == to {
            return Some(vec![]);
        }

        let mut path = Vec::new();
        let mut current = from;

        while current < to {
            if let Some((next_version, migration)) = self.migrations.get(&current) {
                if *next_version > to {
                    // This migration goes beyond our target version
                    return None;
                }
                path.push((current, *next_version, migration));
                current = *next_version;
            } else {
                // No migration found from current version
                return None;
            }
        }

        if current == to {
            Some(path)
        } else {
            None
        }
    }
}

/// JSON-based schema evolution implementation.
pub struct JsonSchemaEvolution {
    registry: Arc<tokio::sync::RwLock<SchemaRegistry>>,
}

impl JsonSchemaEvolution {
    /// Creates a new JSON schema evolution handler.
    pub const fn new(registry: Arc<tokio::sync::RwLock<SchemaRegistry>>) -> Self {
        Self { registry }
    }
}

#[async_trait]
impl SchemaEvolution for JsonSchemaEvolution {
    #[allow(clippy::significant_drop_tightening)]
    async fn migrate(
        &self,
        data: &[u8],
        type_name: &str,
        from_version: u32,
        to_version: u32,
    ) -> Result<Vec<u8>, EventStoreError> {
        // Parse the JSON data
        let mut value: Value = serde_json::from_slice(data).map_err(|e| {
            EventStoreError::DeserializationError(format!(
                "Failed to parse JSON for migration: {e}"
            ))
        })?;

        // Get the migration chain
        let registry = self.registry.read().await;
        let chain = registry.get_chain(type_name).ok_or_else(|| {
            EventStoreError::SchemaEvolutionError(format!(
                "No migrations registered for type: {type_name}"
            ))
        })?;

        // Find the migration path
        let path = chain
            .find_migration_path(from_version, to_version)
            .ok_or_else(|| {
                EventStoreError::SchemaEvolutionError(format!(
                    "No migration path found from version {from_version} to {to_version} for type: {type_name}"
                ))
            })?;

        // Apply each migration in sequence
        for (from, to, migration) in path {
            value = migration(value).map_err(|e| {
                EventStoreError::SchemaEvolutionError(format!(
                    "Migration failed from version {from} to {to}: {e}"
                ))
            })?;
        }

        // Serialize back to bytes
        serde_json::to_vec(&value).map_err(|e| {
            EventStoreError::SerializationError(format!("Failed to serialize migrated data: {e}"))
        })
    }
}

/// Helper functions for common migration patterns.
pub mod helpers {
    use super::{Arc, EventStoreError, MigrationFn, Value};

    /// Creates a migration that adds a new field with a default value.
    pub fn add_field(field_name: &str, default_value: Value) -> MigrationFn {
        let field_name = field_name.to_string();
        Arc::new(move |mut value: Value| {
            if let Value::Object(ref mut map) = value {
                map.insert(field_name.clone(), default_value.clone());
            }
            Ok(value)
        })
    }

    /// Creates a migration that removes a field.
    pub fn remove_field(field_name: &str) -> MigrationFn {
        let field_name = field_name.to_string();
        Arc::new(move |mut value: Value| {
            if let Value::Object(ref mut map) = value {
                map.remove(&field_name);
            }
            Ok(value)
        })
    }

    /// Creates a migration that renames a field.
    pub fn rename_field(old_name: &str, new_name: &str) -> MigrationFn {
        let old_name = old_name.to_string();
        let new_name = new_name.to_string();
        Arc::new(move |mut value: Value| {
            if let Value::Object(ref mut map) = value {
                if let Some(field_value) = map.remove(&old_name) {
                    map.insert(new_name.clone(), field_value);
                }
            }
            Ok(value)
        })
    }

    /// Creates a migration that transforms a field value.
    pub fn transform_field<F>(field_name: &str, transform_fn: F) -> MigrationFn
    where
        F: Fn(Value) -> Result<Value, EventStoreError> + Send + Sync + 'static,
    {
        let field_name = field_name.to_string();
        let transform_fn = Arc::new(transform_fn);
        Arc::new(move |value: Value| {
            if let Value::Object(mut map) = value {
                if let Some(field_value) = map.get(&field_name).cloned() {
                    let transformed = transform_fn(field_value)?;
                    map.insert(field_name.clone(), transformed);
                }
                Ok(Value::Object(map))
            } else {
                Ok(value)
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_migration_chain_path_finding() {
        let mut chain = MigrationChain::new();

        // Add migrations 1->2, 2->3, 3->4
        chain.add_migration(1, 2, Arc::new(Ok));
        chain.add_migration(2, 3, Arc::new(Ok));
        chain.add_migration(3, 4, Arc::new(Ok));

        // Test finding paths
        let path = chain.find_migration_path(1, 4).unwrap();
        assert_eq!(path.len(), 3);

        let path = chain.find_migration_path(2, 4).unwrap();
        assert_eq!(path.len(), 2);

        let path = chain.find_migration_path(1, 1).unwrap();
        assert_eq!(path.len(), 0);

        // Test non-existent path
        assert!(chain.find_migration_path(1, 5).is_none());
    }

    #[tokio::test]
    async fn test_json_schema_evolution() {
        let mut registry = SchemaRegistry::new();

        // Register a migration that adds a "version" field
        registry.register_migration(
            "TestEvent".to_string(),
            1,
            2,
            helpers::add_field("version", json!(2)),
        );

        let registry = Arc::new(tokio::sync::RwLock::new(registry));
        let evolution = JsonSchemaEvolution::new(registry);

        let original_data = json!({
            "id": "test-123",
            "value": 42
        });

        let serialized = serde_json::to_vec(&original_data).unwrap();
        let migrated = evolution
            .migrate(&serialized, "TestEvent", 1, 2)
            .await
            .unwrap();

        let migrated_value: Value = serde_json::from_slice(&migrated).unwrap();
        assert_eq!(migrated_value["id"], "test-123");
        assert_eq!(migrated_value["value"], 42);
        assert_eq!(migrated_value["version"], 2);
    }

    #[test]
    fn test_helper_functions() {
        // Test add_field
        let migration = helpers::add_field("new_field", json!("default"));
        let result = migration(json!({"existing": "value"})).unwrap();
        assert_eq!(result["new_field"], "default");

        // Test remove_field
        let migration = helpers::remove_field("to_remove");
        let result = migration(json!({"to_remove": "value", "keep": "this"})).unwrap();
        assert!(result.get("to_remove").is_none());
        assert_eq!(result["keep"], "this");

        // Test rename_field
        let migration = helpers::rename_field("old_name", "new_name");
        let result = migration(json!({"old_name": "value"})).unwrap();
        assert!(result.get("old_name").is_none());
        assert_eq!(result["new_name"], "value");

        // Test transform_field
        let migration = helpers::transform_field("number", |v| {
            if let Value::Number(ref n) = v {
                n.as_i64().map_or(Ok(v), |i| Ok(json!(i * 2)))
            } else {
                Ok(v)
            }
        });
        let result = migration(json!({"number": 21})).unwrap();
        assert_eq!(result["number"], 42);
    }
}
