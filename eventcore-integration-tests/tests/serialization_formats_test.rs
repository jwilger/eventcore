//! Integration test for multiple serialization formats

use eventcore::serialization::{
    EventSerializer, JsonEventSerializer, MessagePackEventSerializer, SerializationFormat,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct TestEvent {
    id: String,
    value: i32,
    data: Vec<u8>,
}

#[tokio::test]
async fn test_serialization_format_selection() {
    // Test that we can create serializers for each format
    let json_format = SerializationFormat::Json;
    let msgpack_format = SerializationFormat::MessagePack;
    let bincode_format = SerializationFormat::Bincode;

    let json_serializer = json_format.create_serializer();
    let msgpack_serializer = msgpack_format.create_serializer();
    let bincode_serializer = bincode_format.create_serializer();

    let test_event = TestEvent {
        id: "test-format".to_string(),
        value: 100,
        data: vec![10, 20, 30],
    };

    // Test JSON serialization
    let json_bytes = json_serializer
        .serialize(&test_event, "TestEvent")
        .await
        .unwrap();
    let json_decoded: TestEvent = json_serializer
        .deserialize(&json_bytes, "TestEvent")
        .await
        .unwrap();
    assert_eq!(test_event, json_decoded);

    // Test MessagePack serialization
    let msgpack_bytes = msgpack_serializer
        .serialize(&test_event, "TestEvent")
        .await
        .unwrap();
    let msgpack_decoded: TestEvent = msgpack_serializer
        .deserialize(&msgpack_bytes, "TestEvent")
        .await
        .unwrap();
    assert_eq!(test_event, msgpack_decoded);

    // Test Bincode serialization
    let bincode_bytes = bincode_serializer
        .serialize(&test_event, "TestEvent")
        .await
        .unwrap();
    let bincode_decoded: TestEvent = bincode_serializer
        .deserialize(&bincode_bytes, "TestEvent")
        .await
        .unwrap();
    assert_eq!(test_event, bincode_decoded);

    // Verify different formats produce different serialized data
    assert_ne!(json_bytes, msgpack_bytes);
    assert_ne!(json_bytes, bincode_bytes);
    assert_ne!(msgpack_bytes, bincode_bytes);

    // Verify sizes (typically: bincode < msgpack < json)
    println!("JSON size: {} bytes", json_bytes.len());
    println!("MessagePack size: {} bytes", msgpack_bytes.len());
    println!("Bincode size: {} bytes", bincode_bytes.len());
}

#[tokio::test]
async fn test_format_from_string() {
    assert_eq!(
        "json".parse::<SerializationFormat>().unwrap(),
        SerializationFormat::Json
    );
    assert_eq!(
        "messagepack".parse::<SerializationFormat>().unwrap(),
        SerializationFormat::MessagePack
    );
    assert_eq!(
        "msgpack".parse::<SerializationFormat>().unwrap(),
        SerializationFormat::MessagePack
    );
    assert_eq!(
        "bincode".parse::<SerializationFormat>().unwrap(),
        SerializationFormat::Bincode
    );

    assert!("invalid".parse::<SerializationFormat>().is_err());
}

#[tokio::test]
async fn test_cross_serializer_incompatibility() {
    // Events serialized with one format should not be deserializable with another
    let test_event = TestEvent {
        id: "cross-test".to_string(),
        value: 999,
        data: vec![255, 128, 0],
    };

    let json_serializer = JsonEventSerializer::new();
    let msgpack_serializer = MessagePackEventSerializer::new();

    // Serialize with JSON
    let json_bytes = json_serializer
        .serialize(&test_event, "TestEvent")
        .await
        .unwrap();

    // Try to deserialize with MessagePack (should fail)
    let result: Result<TestEvent, _> = msgpack_serializer
        .deserialize(&json_bytes, "TestEvent")
        .await;
    assert!(result.is_err());
}
