//! Backward compatibility validation and migration generation tools.
//!
//! This module provides tools for validating schema compatibility between
//! different versions of event types and automatically generating migration
//! functions for common compatibility scenarios.

use super::evolution::{helpers, MigrationFn};
use crate::errors::EventStoreError;
use serde_json::{Map, Value};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// Schema compatibility level between two versions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompatibilityLevel {
    /// Fully compatible - no migration needed
    FullyCompatible,
    /// Forward compatible - old readers can read new data
    ForwardCompatible,
    /// Backward compatible - new readers can read old data  
    BackwardCompatible,
    /// Incompatible - requires manual migration
    Incompatible,
}

/// Describes a schema change between two versions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchemaChange {
    /// A field was added with an optional default value
    FieldAdded {
        field_name: String,
        field_type: SchemaFieldType,
        default_value: Option<Value>,
    },
    /// A field was removed
    FieldRemoved {
        field_name: String,
        field_type: SchemaFieldType,
    },
    /// A field was renamed
    FieldRenamed {
        old_name: String,
        new_name: String,
        field_type: SchemaFieldType,
    },
    /// A field's type was changed
    FieldTypeChanged {
        field_name: String,
        old_type: SchemaFieldType,
        new_type: SchemaFieldType,
    },
    /// A field was made optional or required
    FieldOptionalityChanged {
        field_name: String,
        field_type: SchemaFieldType,
        now_optional: bool,
    },
    /// A nested object structure was changed
    NestedStructureChanged {
        field_name: String,
        nested_changes: Vec<SchemaChange>,
    },
}

/// Simplified schema field type for compatibility analysis.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchemaFieldType {
    String,
    Number,
    Boolean,
    Array(Box<SchemaFieldType>),
    Object(HashMap<String, SchemaFieldType>),
    Null,
    Union(Vec<SchemaFieldType>),
}

/// Schema definition extracted from JSON data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonSchema {
    /// The fields and their types in this schema
    pub fields: HashMap<String, SchemaFieldType>,
    /// Required field names
    pub required_fields: HashSet<String>,
}

/// Compatibility analysis result.
#[derive(Clone)]
pub struct CompatibilityAnalysis {
    /// Overall compatibility level
    pub level: CompatibilityLevel,
    /// List of changes between the schemas
    pub changes: Vec<SchemaChange>,
    /// Whether an automatic migration can be generated
    pub auto_migration_possible: bool,
    /// Generated migration function (if possible)
    pub migration: Option<MigrationFn>,
    /// Issues that prevent compatibility or auto-migration
    pub issues: Vec<String>,
}

impl std::fmt::Debug for CompatibilityAnalysis {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CompatibilityAnalysis")
            .field("level", &self.level)
            .field("changes", &self.changes)
            .field("auto_migration_possible", &self.auto_migration_possible)
            .field("migration_present", &self.migration.is_some())
            .field("issues", &self.issues)
            .finish()
    }
}

/// Tools for analyzing and handling schema compatibility.
pub struct CompatibilityValidator;

impl CompatibilityValidator {
    /// Analyzes compatibility between two JSON schemas.
    pub fn analyze_compatibility(
        old_schema: &JsonSchema,
        new_schema: &JsonSchema,
    ) -> CompatibilityAnalysis {
        let changes = Self::detect_changes(old_schema, new_schema);
        let level = Self::determine_compatibility_level(&changes);
        let (auto_migration_possible, migration, issues) = Self::try_generate_migration(&changes);

        CompatibilityAnalysis {
            level,
            changes,
            auto_migration_possible,
            migration,
            issues,
        }
    }

    /// Extracts a schema from sample JSON data.
    pub fn extract_schema(json_data: &Value) -> Result<JsonSchema, EventStoreError> {
        match json_data {
            Value::Object(map) => {
                let mut fields = HashMap::new();
                let mut required_fields = HashSet::new();

                for (key, value) in map {
                    let field_type = Self::infer_field_type(value)?;
                    fields.insert(key.clone(), field_type);

                    // Consider non-null values as required for now
                    // In practice, this would be determined by schema annotations
                    if !matches!(value, Value::Null) {
                        required_fields.insert(key.clone());
                    }
                }

                Ok(JsonSchema {
                    fields,
                    required_fields,
                })
            }
            _ => Err(EventStoreError::DeserializationFailed(
                "Can only extract schema from JSON objects".to_string(),
            )),
        }
    }

    /// Infers the field type from a JSON value.
    fn infer_field_type(value: &Value) -> Result<SchemaFieldType, EventStoreError> {
        match value {
            Value::String(_) => Ok(SchemaFieldType::String),
            Value::Number(_) => Ok(SchemaFieldType::Number),
            Value::Bool(_) => Ok(SchemaFieldType::Boolean),
            Value::Null => Ok(SchemaFieldType::Null),
            Value::Array(arr) => {
                if arr.is_empty() {
                    Ok(SchemaFieldType::Array(Box::new(SchemaFieldType::Null)))
                } else {
                    // Infer element type from first element
                    let element_type = Self::infer_field_type(&arr[0])?;
                    Ok(SchemaFieldType::Array(Box::new(element_type)))
                }
            }
            Value::Object(map) => {
                let mut object_fields = HashMap::new();
                for (key, val) in map {
                    object_fields.insert(key.clone(), Self::infer_field_type(val)?);
                }
                Ok(SchemaFieldType::Object(object_fields))
            }
        }
    }

    /// Detects changes between two schemas.
    fn detect_changes(old_schema: &JsonSchema, new_schema: &JsonSchema) -> Vec<SchemaChange> {
        let mut changes = Vec::new();

        // Find added fields
        for (field_name, field_type) in &new_schema.fields {
            if !old_schema.fields.contains_key(field_name) {
                changes.push(SchemaChange::FieldAdded {
                    field_name: field_name.clone(),
                    field_type: field_type.clone(),
                    default_value: Self::get_default_value_for_type(field_type),
                });
            }
        }

        // Find removed fields
        for (field_name, field_type) in &old_schema.fields {
            if !new_schema.fields.contains_key(field_name) {
                changes.push(SchemaChange::FieldRemoved {
                    field_name: field_name.clone(),
                    field_type: field_type.clone(),
                });
            }
        }

        // Find type changes and optionality changes
        for (field_name, old_type) in &old_schema.fields {
            if let Some(new_type) = new_schema.fields.get(field_name) {
                if old_type != new_type {
                    changes.push(SchemaChange::FieldTypeChanged {
                        field_name: field_name.clone(),
                        old_type: old_type.clone(),
                        new_type: new_type.clone(),
                    });
                }

                // Check optionality changes
                let old_required = old_schema.required_fields.contains(field_name);
                let new_required = new_schema.required_fields.contains(field_name);
                if old_required != new_required {
                    changes.push(SchemaChange::FieldOptionalityChanged {
                        field_name: field_name.clone(),
                        field_type: new_type.clone(),
                        now_optional: !new_required,
                    });
                }
            }
        }

        changes
    }

    /// Determines the overall compatibility level based on changes.
    fn determine_compatibility_level(changes: &[SchemaChange]) -> CompatibilityLevel {
        if changes.is_empty() {
            return CompatibilityLevel::FullyCompatible;
        }

        let mut has_breaking_changes = false;
        let mut has_forward_compatible_changes = false;
        let mut has_backward_compatible_changes = false;

        for change in changes {
            match change {
                SchemaChange::FieldAdded { default_value, .. } => {
                    if default_value.is_some() {
                        has_backward_compatible_changes = true;
                    } else {
                        has_breaking_changes = true;
                    }
                }
                SchemaChange::FieldRemoved { .. } => {
                    has_breaking_changes = true;
                }
                SchemaChange::FieldRenamed { .. } => {
                    has_breaking_changes = true;
                }
                SchemaChange::FieldTypeChanged {
                    old_type, new_type, ..
                } => {
                    if Self::is_type_compatible(old_type, new_type) {
                        has_forward_compatible_changes = true;
                    } else {
                        has_breaking_changes = true;
                    }
                }
                SchemaChange::FieldOptionalityChanged { now_optional, .. } => {
                    if *now_optional {
                        has_backward_compatible_changes = true;
                    } else {
                        has_breaking_changes = true;
                    }
                }
                SchemaChange::NestedStructureChanged { .. } => {
                    // For simplicity, consider nested changes as potentially breaking
                    has_breaking_changes = true;
                }
            }
        }

        if has_breaking_changes {
            CompatibilityLevel::Incompatible
        } else if has_forward_compatible_changes && has_backward_compatible_changes {
            CompatibilityLevel::FullyCompatible
        } else if has_forward_compatible_changes {
            CompatibilityLevel::ForwardCompatible
        } else {
            CompatibilityLevel::BackwardCompatible
        }
    }

    /// Checks if one type is compatible with another.
    fn is_type_compatible(old_type: &SchemaFieldType, new_type: &SchemaFieldType) -> bool {
        match (old_type, new_type) {
            // Same types are always compatible
            (a, b) if a == b => true,

            // Number and string are often interconvertible
            (SchemaFieldType::Number, SchemaFieldType::String) => true,
            (SchemaFieldType::String, SchemaFieldType::Number) => true,

            // Boolean and string are often interconvertible
            (SchemaFieldType::Boolean, SchemaFieldType::String) => true,
            (SchemaFieldType::String, SchemaFieldType::Boolean) => true,

            // Null can be compatible if the new type is optional
            (SchemaFieldType::Null, _) => true,

            // Arrays are compatible if element types are compatible
            (SchemaFieldType::Array(old_elem), SchemaFieldType::Array(new_elem)) => {
                Self::is_type_compatible(old_elem, new_elem)
            }

            _ => false,
        }
    }

    /// Attempts to generate an automatic migration for the given changes.
    fn try_generate_migration(
        changes: &[SchemaChange],
    ) -> (bool, Option<MigrationFn>, Vec<String>) {
        let mut migration_steps = Vec::new();
        let mut issues = Vec::new();

        for change in changes {
            match change {
                SchemaChange::FieldAdded {
                    field_name,
                    default_value,
                    ..
                } => {
                    if let Some(default) = default_value {
                        migration_steps.push(helpers::add_field(field_name, default.clone()));
                    } else {
                        issues.push(format!(
                            "Cannot auto-migrate added field '{field_name}' without default value"
                        ));
                    }
                }
                SchemaChange::FieldRemoved { field_name, .. } => {
                    // Field removal is often a breaking change
                    issues.push(format!(
                        "Cannot auto-migrate removed field '{field_name}' - may break old readers"
                    ));
                }
                SchemaChange::FieldRenamed {
                    old_name, new_name, ..
                } => {
                    migration_steps.push(helpers::rename_field(old_name, new_name));
                }
                SchemaChange::FieldTypeChanged {
                    field_name,
                    old_type,
                    new_type,
                    ..
                } => {
                    if let Some(converter) = Self::get_type_converter(old_type, new_type) {
                        let field_name = field_name.clone();
                        migration_steps.push(helpers::convert_field_type(&field_name, move |v| {
                            converter(v)
                        }));
                    } else {
                        issues.push(format!(
                            "Cannot auto-convert field '{field_name}' from {old_type:?} to {new_type:?}"
                        ));
                    }
                }
                SchemaChange::FieldOptionalityChanged { .. } => {
                    // Optionality changes usually don't require data migration
                    // They're handled at the schema level
                }
                SchemaChange::NestedStructureChanged { field_name, .. } => {
                    issues.push(format!(
                        "Cannot auto-migrate nested structure changes in field '{field_name}'"
                    ));
                }
            }
        }

        let auto_migration_possible = issues.is_empty();
        let migration = if auto_migration_possible && !migration_steps.is_empty() {
            Some(Self::combine_migrations(migration_steps))
        } else {
            None
        };

        (auto_migration_possible, migration, issues)
    }

    /// Gets a default value for a schema field type.
    fn get_default_value_for_type(field_type: &SchemaFieldType) -> Option<Value> {
        match field_type {
            SchemaFieldType::String => Some(Value::String(String::new())),
            SchemaFieldType::Number => Some(Value::Number(0.into())),
            SchemaFieldType::Boolean => Some(Value::Bool(false)),
            SchemaFieldType::Array(_) => Some(Value::Array(Vec::new())),
            SchemaFieldType::Object(_) => Some(Value::Object(Map::new())),
            SchemaFieldType::Null => Some(Value::Null),
            SchemaFieldType::Union(_) => Some(Value::Null),
        }
    }

    /// Gets a type converter function for converting between compatible types.
    fn get_type_converter(
        old_type: &SchemaFieldType,
        new_type: &SchemaFieldType,
    ) -> Option<Arc<dyn Fn(Value) -> Result<Value, EventStoreError> + Send + Sync>> {
        match (old_type, new_type) {
            (SchemaFieldType::String, SchemaFieldType::Number) => {
                Some(Arc::new(helpers::converters::string_to_number))
            }
            (SchemaFieldType::Number, SchemaFieldType::String) => {
                Some(Arc::new(helpers::converters::number_to_string))
            }
            (SchemaFieldType::String, SchemaFieldType::Boolean) => {
                Some(Arc::new(helpers::converters::string_to_boolean))
            }
            (SchemaFieldType::Boolean, SchemaFieldType::String) => {
                Some(Arc::new(helpers::converters::boolean_to_string))
            }
            _ => None,
        }
    }

    /// Combines multiple migration functions into a single function.
    fn combine_migrations(migrations: Vec<MigrationFn>) -> MigrationFn {
        Arc::new(move |mut value: Value| {
            for migration in &migrations {
                value = migration(value)?;
            }
            Ok(value)
        })
    }
}

/// Builder for creating migration strategies for specific compatibility scenarios.
pub struct MigrationBuilder {
    migrations: Vec<MigrationFn>,
}

impl MigrationBuilder {
    /// Creates a new migration builder.
    pub fn new() -> Self {
        Self {
            migrations: Vec::new(),
        }
    }

    /// Adds a migration step to handle field addition.
    pub fn add_field(mut self, field_name: &str, default_value: Value) -> Self {
        self.migrations
            .push(helpers::add_field(field_name, default_value));
        self
    }

    /// Adds a migration step to handle field removal.
    pub fn remove_field(mut self, field_name: &str) -> Self {
        self.migrations.push(helpers::remove_field(field_name));
        self
    }

    /// Adds a migration step to handle field renaming.
    pub fn rename_field(mut self, old_name: &str, new_name: &str) -> Self {
        self.migrations
            .push(helpers::rename_field(old_name, new_name));
        self
    }

    /// Adds a migration step to handle type conversion.
    pub fn convert_field_type<F>(mut self, field_name: &str, converter: F) -> Self
    where
        F: Fn(Value) -> Result<Value, EventStoreError> + Send + Sync + 'static,
    {
        self.migrations
            .push(helpers::convert_field_type(field_name, converter));
        self
    }

    /// Adds a custom migration step.
    pub fn add_custom_migration<F>(mut self, migration: F) -> Self
    where
        F: Fn(Value) -> Result<Value, EventStoreError> + Send + Sync + 'static,
    {
        self.migrations.push(Arc::new(migration));
        self
    }

    /// Builds the final migration function.
    pub fn build(self) -> MigrationFn {
        CompatibilityValidator::combine_migrations(self.migrations)
    }
}

impl Default for MigrationBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_schema_extraction() {
        let json_data = json!({
            "id": "test-123",
            "name": "John Doe",
            "age": 30,
            "active": true,
            "tags": ["user", "admin"],
            "profile": {
                "email": "john@example.com",
                "phone": null
            }
        });

        let schema = CompatibilityValidator::extract_schema(&json_data).unwrap();

        assert!(schema.fields.contains_key("id"));
        assert!(schema.fields.contains_key("name"));
        assert!(schema.fields.contains_key("age"));
        assert!(schema.fields.contains_key("active"));
        assert!(schema.fields.contains_key("tags"));
        assert!(schema.fields.contains_key("profile"));

        assert_eq!(schema.fields["id"], SchemaFieldType::String);
        assert_eq!(schema.fields["name"], SchemaFieldType::String);
        assert_eq!(schema.fields["age"], SchemaFieldType::Number);
        assert_eq!(schema.fields["active"], SchemaFieldType::Boolean);
        assert!(matches!(schema.fields["tags"], SchemaFieldType::Array(_)));
        assert!(matches!(
            schema.fields["profile"],
            SchemaFieldType::Object(_)
        ));
    }

    #[test]
    fn test_compatibility_analysis_backward_compatible() {
        let old_schema = JsonSchema {
            fields: [
                ("id".to_string(), SchemaFieldType::String),
                ("name".to_string(), SchemaFieldType::String),
            ]
            .into_iter()
            .collect(),
            required_fields: ["id", "name"].iter().map(|s| (*s).to_string()).collect(),
        };

        let new_schema = JsonSchema {
            fields: [
                ("id".to_string(), SchemaFieldType::String),
                ("name".to_string(), SchemaFieldType::String),
                ("email".to_string(), SchemaFieldType::String),
            ]
            .into_iter()
            .collect(),
            required_fields: ["id", "name"].iter().map(|s| (*s).to_string()).collect(),
        };

        let analysis = CompatibilityValidator::analyze_compatibility(&old_schema, &new_schema);

        assert_eq!(analysis.level, CompatibilityLevel::BackwardCompatible);
        assert_eq!(analysis.changes.len(), 1);
        assert!(matches!(
            analysis.changes[0],
            SchemaChange::FieldAdded { .. }
        ));
        assert!(analysis.auto_migration_possible);
        assert!(analysis.migration.is_some());
    }

    #[test]
    fn test_compatibility_analysis_incompatible() {
        let old_schema = JsonSchema {
            fields: [
                ("id".to_string(), SchemaFieldType::String),
                ("name".to_string(), SchemaFieldType::String),
                ("age".to_string(), SchemaFieldType::Number),
            ]
            .into_iter()
            .collect(),
            required_fields: ["id", "name", "age"]
                .iter()
                .map(|s| (*s).to_string())
                .collect(),
        };

        let new_schema = JsonSchema {
            fields: [
                ("id".to_string(), SchemaFieldType::String),
                ("full_name".to_string(), SchemaFieldType::String),
            ]
            .into_iter()
            .collect(),
            required_fields: ["id", "full_name"]
                .iter()
                .map(|s| (*s).to_string())
                .collect(),
        };

        let analysis = CompatibilityValidator::analyze_compatibility(&old_schema, &new_schema);

        assert_eq!(analysis.level, CompatibilityLevel::Incompatible);
        assert!(analysis.changes.len() > 1);
        assert!(!analysis.auto_migration_possible);
        assert!(!analysis.issues.is_empty());
    }

    #[test]
    fn test_migration_builder() {
        let migration = MigrationBuilder::new()
            .add_field("version", json!(2))
            .rename_field("old_name", "new_name")
            .convert_field_type("age", helpers::converters::number_to_string)
            .build();

        let test_data = json!({
            "old_name": "test value",
            "age": 25
        });

        let result = migration(test_data).unwrap();

        assert_eq!(result["version"], 2);
        assert_eq!(result["new_name"], "test value");
        assert_eq!(result["age"], "25");
        assert!(result.get("old_name").is_none());
    }

    #[test]
    fn test_type_compatibility() {
        assert!(CompatibilityValidator::is_type_compatible(
            &SchemaFieldType::String,
            &SchemaFieldType::Number
        ));

        assert!(CompatibilityValidator::is_type_compatible(
            &SchemaFieldType::Number,
            &SchemaFieldType::String
        ));

        assert!(CompatibilityValidator::is_type_compatible(
            &SchemaFieldType::Boolean,
            &SchemaFieldType::String
        ));

        assert!(!CompatibilityValidator::is_type_compatible(
            &SchemaFieldType::Array(Box::new(SchemaFieldType::String)),
            &SchemaFieldType::Object(HashMap::new())
        ));
    }

    #[test]
    fn test_automatic_migration_generation() {
        let old_data = json!({
            "user_id": "123",
            "user_name": "John Doe",
            "user_age": "30"
        });

        let new_data = json!({
            "id": "123",
            "name": "John Doe",
            "age": 30,
            "status": "active"
        });

        let old_schema = CompatibilityValidator::extract_schema(&old_data).unwrap();
        let new_schema = CompatibilityValidator::extract_schema(&new_data).unwrap();

        // This would be incompatible for auto-migration due to multiple renames
        let analysis = CompatibilityValidator::analyze_compatibility(&old_schema, &new_schema);
        assert_eq!(analysis.level, CompatibilityLevel::Incompatible);

        // But we can manually build a migration
        let migration = MigrationBuilder::new()
            .rename_field("user_id", "id")
            .rename_field("user_name", "name")
            .rename_field("user_age", "age") // First rename the field
            .convert_field_type("age", helpers::converters::string_to_number) // Then convert its type
            .add_field("status", json!("active"))
            .build();

        let result = migration(old_data).unwrap();
        assert_eq!(result["id"], "123");
        assert_eq!(result["name"], "John Doe");
        assert_eq!(result["age"], json!(30.0)); // JSON numbers are f64
        assert_eq!(result["status"], "active");
    }
}
