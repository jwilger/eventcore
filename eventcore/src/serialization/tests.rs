//! Property-based tests for event serialization.

#![allow(clippy::similar_names)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::nonminimal_bool)]
#![allow(clippy::option_if_let_else)]

use super::*;
use crate::event::Event;
use crate::metadata::EventMetadata;
use crate::testing::generators::{arb_event_version, arb_stream_id};
use crate::types::{EventVersion, StreamId};
use proptest::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;
use tokio::sync::RwLock;

// Test event types with different complexity levels
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct SimpleEvent {
    id: String,
    value: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ComplexEvent {
    id: String,
    items: Vec<String>,
    metadata: std::collections::HashMap<String, String>,
    nested: NestedData,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct NestedData {
    level: u32,
    data: Option<Box<NestedData>>,
}

// Property test generators
prop_compose! {
    fn simple_event()(
        id in "[a-zA-Z0-9]{8,16}",
        value in any::<i32>()
    ) -> SimpleEvent {
        SimpleEvent { id, value }
    }
}

prop_compose! {
    fn nested_data(max_depth: u32)(
        level in 0u32..max_depth,
        has_data in any::<bool>()
    ) -> NestedData {
        if level == 0 || !has_data {
            NestedData { level, data: None }
        } else {
            NestedData {
                level,
                data: Some(Box::new(NestedData {
                    level: level - 1,
                    data: None,
                })),
            }
        }
    }
}

prop_compose! {
    fn complex_event()(
        id in "[a-zA-Z0-9]{8,16}",
        items in prop::collection::vec("[a-zA-Z0-9]{4,8}", 0..10),
        keys in prop::collection::vec("[a-zA-Z]{4,8}", 0..5),
        values in prop::collection::vec("[a-zA-Z0-9]{4,12}", 0..5),
        nested in nested_data(3)
    ) -> ComplexEvent {
        let metadata = keys.into_iter()
            .zip(values.into_iter())
            .collect();
        ComplexEvent { id, items, metadata, nested }
    }
}

prop_compose! {
    fn stored_event_with_simple_payload()(
        payload in simple_event(),
        stream_id in arb_stream_id(),
        version in arb_event_version()
    ) -> StoredEvent<SimpleEvent> {
        let event = Event::new(stream_id, payload, EventMetadata::default());
        StoredEvent::new(event, version)
    }
}

mod json_serializer_properties {
    use super::*;

    proptest! {
        #[test]
        fn simple_event_round_trip(event in simple_event()) {
            tokio::runtime::Runtime::new().unwrap().block_on(async {
                let serializer = JsonEventSerializer::new();

                let serialized = serializer
                    .serialize(&event, "SimpleEvent")
                    .await
                    .expect("Serialization should succeed");

                let deserialized: SimpleEvent = serializer
                    .deserialize(&serialized, "SimpleEvent")
                    .await
                    .expect("Deserialization should succeed");

                prop_assert_eq!(event, deserialized);
                Ok(())
            })?;
        }

        #[test]
        fn complex_event_round_trip(event in complex_event()) {
            tokio::runtime::Runtime::new().unwrap().block_on(async {
                let serializer = JsonEventSerializer::new();

                let serialized = serializer
                    .serialize(&event, "ComplexEvent")
                    .await
                    .expect("Serialization should succeed");

                let deserialized: ComplexEvent = serializer
                    .deserialize(&serialized, "ComplexEvent")
                    .await
                    .expect("Deserialization should succeed");

                prop_assert_eq!(event, deserialized);
                Ok(())
            })?;
        }

        #[test]
        fn stored_event_round_trip(event in stored_event_with_simple_payload()) {
            tokio::runtime::Runtime::new().unwrap().block_on(async {
                let serializer = JsonEventSerializer::new();

                let serialized = serializer
                    .serialize_stored_event(&event, "SimpleEvent")
                    .await
                    .expect("Serialization should succeed");

                let deserialized: StoredEvent<SimpleEvent> = serializer
                    .deserialize_stored_event(&serialized, "SimpleEvent")
                    .await
                    .expect("Deserialization should succeed");

                prop_assert_eq!(event.event.id, deserialized.event.id);
                prop_assert_eq!(event.event.stream_id, deserialized.event.stream_id);
                prop_assert_eq!(event.event.payload, deserialized.event.payload);
                prop_assert_eq!(event.version, deserialized.version);
                // Timestamps might need tolerance for serialization
                prop_assert_eq!(event.event.created_at, deserialized.event.created_at);
                prop_assert_eq!(event.stored_at, deserialized.stored_at);
                Ok(())
            })?;
        }

        #[test]
        fn serialized_data_is_valid_json(event in simple_event()) {
            tokio::runtime::Runtime::new().unwrap().block_on(async {
                let serializer = JsonEventSerializer::new();

                let serialized = serializer
                    .serialize(&event, "SimpleEvent")
                    .await
                    .expect("Serialization should succeed");

                // Verify it's valid JSON
                let parsed: serde_json::Value = serde_json::from_slice(&serialized)
                    .expect("Should be valid JSON");

                prop_assert!(parsed.is_object());
                Ok(())
            })?;
        }

        #[test]
        fn schema_version_consistency(
            type_name in "[a-zA-Z][a-zA-Z0-9_]{4,20}",
            version in 1u32..100
        ) {
            tokio::runtime::Runtime::new().unwrap().block_on(async {
                let serializer = JsonEventSerializer::new();

                // Register a version
                serializer.register_schema_version(type_name.clone(), version).await;

                // Verify it's stored correctly
                prop_assert_eq!(serializer.get_schema_version(&type_name), version);

                // Verify version support
                prop_assert!(serializer.supports_schema_version(&type_name, version));
                prop_assert!(serializer.supports_schema_version(&type_name, version - 1));
                prop_assert!(!serializer.supports_schema_version(&type_name, version + 1));
                Ok(())
            })?;
        }
    }
}

mod schema_evolution_properties {
    use super::*;
    use crate::serialization::evolution::{helpers, JsonSchemaEvolution, SchemaRegistry};

    proptest! {
        #[test]
        fn field_addition_preserves_existing_data(event in simple_event()) {
            tokio::runtime::Runtime::new().unwrap().block_on(async {
                let mut registry = SchemaRegistry::new();

                // Register migration that adds a "timestamp" field
                registry.register_migration(
                    "SimpleEvent".to_string(),
                    1,
                    2,
                    helpers::add_field("timestamp", json!("2024-01-01T00:00:00Z")),
                );

                let registry = Arc::new(RwLock::new(registry));
                let evolution = JsonSchemaEvolution::new(registry.clone());
                let serializer = JsonEventSerializer::with_evolution(Arc::new(evolution));

                // Serialize with version 1
                let v1_data = serde_json::to_vec(&event).unwrap();

                // Register version 2
                serializer.register_schema_version("SimpleEvent".to_string(), 2).await;

                // Create stored event with v1 data
                let stream_id = StreamId::try_new("test").unwrap();
                let event_obj = Event::new(stream_id, event.clone(), EventMetadata::default());
                let stored = StoredEvent::new(event_obj, EventVersion::try_new(1).unwrap());

                // Create the envelope manually with v1 schema version
                let envelope = SerializedEventEnvelope {
                    schema_version: 1, // Old version
                    type_name: "SimpleEvent".to_string(),
                    payload_data: v1_data,
                    event_id: stored.event.id.clone(),
                    stream_id: stored.event.stream_id.clone(),
                    event_metadata: stored.event.metadata.clone(),
                    created_at: stored.event.created_at.clone(),
                    version: stored.version,
                    stored_at: stored.stored_at.clone(),
                };

                let serialized = serde_json::to_vec(&envelope).unwrap();

                // Deserialize, which should trigger migration
                let deserialized: StoredEvent<serde_json::Value> = serializer
                    .deserialize_stored_event(&serialized, "SimpleEvent")
                    .await
                    .unwrap();

                // Verify original data is preserved and new field added
                let payload = &deserialized.event.payload;
                prop_assert_eq!(&payload["id"], &json!(event.id));
                prop_assert_eq!(&payload["value"], &json!(event.value));
                prop_assert_eq!(&payload["timestamp"], &json!("2024-01-01T00:00:00Z"));
                Ok(())
            })?;
        }

        #[test]
        fn field_removal_removes_only_specified_field(event in complex_event()) {
            tokio::runtime::Runtime::new().unwrap().block_on(async {
                let mut registry = SchemaRegistry::new();

                // Register migration that removes "metadata" field
                registry.register_migration(
                    "ComplexEvent".to_string(),
                    1,
                    2,
                    helpers::remove_field("metadata"),
                );

                let registry = Arc::new(RwLock::new(registry));
                let evolution = JsonSchemaEvolution::new(registry);

                let v1_data = serde_json::to_vec(&event).unwrap();
                let migrated = evolution
                    .migrate(&v1_data, "ComplexEvent", 1, 2)
                    .await
                    .unwrap();

                let migrated_value: serde_json::Value = serde_json::from_slice(&migrated).unwrap();

                // Verify metadata is removed but other fields remain
                prop_assert!(!migrated_value.get("metadata").is_some());
                prop_assert_eq!(&migrated_value["id"], &json!(event.id));
                prop_assert_eq!(&migrated_value["items"], &json!(event.items));
                prop_assert!(migrated_value.get("nested").is_some());
                Ok(())
            })?;
        }

        #[test]
        fn field_rename_preserves_value(event in simple_event()) {
            tokio::runtime::Runtime::new().unwrap().block_on(async {
                let mut registry = SchemaRegistry::new();

                // Register migration that renames "value" to "amount"
                registry.register_migration(
                    "SimpleEvent".to_string(),
                    1,
                    2,
                    helpers::rename_field("value", "amount"),
                );

                let registry = Arc::new(RwLock::new(registry));
                let evolution = JsonSchemaEvolution::new(registry);

                let v1_data = serde_json::to_vec(&event).unwrap();
                let migrated = evolution
                    .migrate(&v1_data, "SimpleEvent", 1, 2)
                    .await
                    .unwrap();

                let migrated_value: serde_json::Value = serde_json::from_slice(&migrated).unwrap();

                // Verify field is renamed with value preserved
                prop_assert!(!migrated_value.get("value").is_some());
                prop_assert_eq!(&migrated_value["amount"], &json!(event.value));
                prop_assert_eq!(&migrated_value["id"], &json!(event.id));
                Ok(())
            })?;
        }

        #[test]
        fn multi_version_migration_chain(initial_value in 0i32..1000) {
            tokio::runtime::Runtime::new().unwrap().block_on(async {
                let mut registry = SchemaRegistry::new();

                // Create a chain of migrations: v1 -> v2 -> v3 -> v4
                // v1->v2: add "version" field
                registry.register_migration(
                    "ChainEvent".to_string(),
                    1,
                    2,
                    helpers::add_field("version", json!(2)),
                );

                // v2->v3: rename "value" to "amount"
                registry.register_migration(
                    "ChainEvent".to_string(),
                    2,
                    3,
                    helpers::rename_field("value", "amount"),
                );

                // v3->v4: transform amount (double it)
                registry.register_migration(
                    "ChainEvent".to_string(),
                    3,
                    4,
                    helpers::transform_field("amount", move |v| {
                        if let serde_json::Value::Number(ref n) = v {
                            if let Some(i) = n.as_i64() {
                                Ok(json!(i * 2))
                            } else {
                                Ok(v)
                            }
                        } else {
                            Ok(v)
                        }
                    }),
                );

                let registry = Arc::new(RwLock::new(registry));
                let evolution = JsonSchemaEvolution::new(registry);

                let v1_event = json!({
                    "id": "test",
                    "value": initial_value
                });

                let v1_data = serde_json::to_vec(&v1_event).unwrap();
                let migrated = evolution
                    .migrate(&v1_data, "ChainEvent", 1, 4)
                    .await
                    .unwrap();

                let migrated_value: serde_json::Value = serde_json::from_slice(&migrated).unwrap();

                // Verify all migrations were applied
                prop_assert_eq!(&migrated_value["version"], &json!(2));
                prop_assert!(!migrated_value.get("value").is_some());
                prop_assert_eq!(&migrated_value["amount"], &json!(initial_value * 2));
                prop_assert_eq!(&migrated_value["id"], &json!("test"));
                Ok(())
            })?;
        }

        #[test]
        fn invalid_migration_path_returns_error(
            from_version in 1u32..10,
            to_version in 11u32..20
        ) {
            tokio::runtime::Runtime::new().unwrap().block_on(async {
                let registry = SchemaRegistry::new();
                let registry = Arc::new(RwLock::new(registry));
                let evolution = JsonSchemaEvolution::new(registry);

                let data = json!({"test": "data"});
                let serialized = serde_json::to_vec(&data).unwrap();

                let result = evolution
                    .migrate(&serialized, "UnknownEvent", from_version, to_version)
                    .await;

                prop_assert!(result.is_err());
                Ok(())
            })?;
        }
    }
}

#[cfg(test)]
mod integration_tests {
    use super::*;
    use crate::serialization::evolution::{helpers, JsonSchemaEvolution, SchemaRegistry};

    #[tokio::test]
    async fn test_full_evolution_workflow() {
        // Create a registry with migrations
        let mut registry = SchemaRegistry::new();

        // V1 -> V2: Add status field
        registry.register_migration(
            "Order".to_string(),
            1,
            2,
            helpers::add_field("status", json!("pending")),
        );

        // V2 -> V3: Rename customerId to customer_id
        registry.register_migration(
            "Order".to_string(),
            2,
            3,
            helpers::rename_field("customerId", "customer_id"),
        );

        let registry = Arc::new(RwLock::new(registry));
        let evolution = Arc::new(JsonSchemaEvolution::new(registry));
        let serializer = JsonEventSerializer::with_evolution(evolution);

        // Register current version as 3
        serializer
            .register_schema_version("Order".to_string(), 3)
            .await;

        // Create a V1 order event
        let v1_order = json!({
            "orderId": "ORD-123",
            "customerId": "CUST-456",
            "amount": 99.99
        });

        // Create stored event with V1 data
        let stream_id = StreamId::try_new("order-123").unwrap();
        let event = Event::new(
            stream_id.clone(),
            v1_order.clone(),
            EventMetadata::default(),
        );
        let stored_event = StoredEvent::new(event, EventVersion::try_new(1).unwrap());

        // Serialize (with version 1 in envelope)
        let envelope = SerializedEventEnvelope {
            schema_version: 1, // Old version
            type_name: "Order".to_string(),
            payload_data: serde_json::to_vec(&v1_order).unwrap(),
            event_id: stored_event.event.id.clone(),
            stream_id: stored_event.event.stream_id.clone(),
            event_metadata: stored_event.event.metadata.clone(),
            created_at: stored_event.event.created_at.clone(),
            version: stored_event.version,
            stored_at: stored_event.stored_at.clone(),
        };

        let serialized = serde_json::to_vec(&envelope).unwrap();

        // Deserialize (should trigger migration from v1 to v3)
        let deserialized: StoredEvent<serde_json::Value> = serializer
            .deserialize_stored_event(&serialized, "Order")
            .await
            .unwrap();

        let migrated_data = &deserialized.event.payload;

        // Verify all migrations were applied
        assert_eq!(migrated_data["orderId"], "ORD-123");
        assert_eq!(migrated_data["customer_id"], "CUST-456"); // Renamed
        assert_eq!(migrated_data["amount"], 99.99);
        assert_eq!(migrated_data["status"], "pending"); // Added
        assert!(migrated_data.get("customerId").is_none()); // Old name removed
    }
}
