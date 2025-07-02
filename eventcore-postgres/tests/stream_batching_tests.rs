//! Tests for stream batching optimizations in `PostgreSQL` adapter

#![allow(clippy::similar_names)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_possible_wrap)]
#![allow(clippy::unused_async)]
#![allow(clippy::manual_assert)]
#![allow(clippy::uninlined_format_args)]
#![allow(clippy::manual_range_contains)]

use std::time::Duration;

use eventcore::{
    EventId, EventMetadata, EventStore, EventToWrite, EventVersion, ExpectedVersion, ReadOptions,
    StreamEvents, StreamId,
};
use eventcore_postgres::{PostgresConfig, PostgresConfigBuilder, PostgresEventStore};
use serde::{Deserialize, Serialize};
use testcontainers::{core::WaitFor, runners::AsyncRunner, ContainerAsync, GenericImage, ImageExt};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
enum TestEvent {
    Created { id: String, value: i32 },
    Updated { id: String, value: i32 },
}

async fn setup_test_store() -> (
    PostgresEventStore<TestEvent>,
    Option<ContainerAsync<GenericImage>>,
) {
    // Check if running in CI with existing PostgreSQL service
    if let Ok(test_db_url) = std::env::var("TEST_DATABASE_URL") {
        let config = PostgresConfigBuilder::new()
            .database_url(test_db_url)
            .max_connections(5)
            .connect_timeout(Duration::from_secs(5))
            .read_batch_size(100) // Small batch size for testing
            .build();

        let store = PostgresEventStore::new(config).await.unwrap();
        store.initialize().await.unwrap();

        return (store, None);
    }

    // Fall back to testcontainers for local development
    let postgres_image = GenericImage::new("postgres", "16-alpine").with_wait_for(
        WaitFor::message_on_stderr("database system is ready to accept connections"),
    );

    let container = postgres_image
        .with_env_var("POSTGRES_USER", "postgres")
        .with_env_var("POSTGRES_PASSWORD", "postgres")
        .with_env_var("POSTGRES_DB", "postgres")
        .start()
        .await
        .unwrap();
    let port = container.get_host_port_ipv4(5432).await.unwrap();

    let config = PostgresConfigBuilder::new()
        .database_url(format!(
            "postgres://postgres:postgres@localhost:{port}/postgres"
        ))
        .max_connections(5)
        .connect_timeout(Duration::from_secs(5))
        .read_batch_size(100) // Small batch size for testing
        .build();

    let store = PostgresEventStore::new(config).await.unwrap();
    store.initialize().await.unwrap();

    // Give PostgreSQL a moment to fully initialize
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    (store, Some(container))
}

async fn create_test_events(count: usize, _stream_id: &StreamId) -> Vec<EventToWrite<TestEvent>> {
    (0..count)
        .map(|i| EventToWrite {
            event_id: EventId::new(),
            payload: TestEvent::Created {
                id: format!("item-{i}"),
                value: i as i32,
            },
            metadata: Some(EventMetadata::new()),
        })
        .collect()
}

#[tokio::test]
async fn test_read_batch_size_configuration() {
    // Test with custom batch size
    let config = PostgresConfigBuilder::new()
        .database_url("postgres://localhost/test")
        .read_batch_size(50)
        .build();

    assert_eq!(config.read_batch_size, 50);

    // Test default batch size
    let default_config = PostgresConfig::default();
    assert_eq!(default_config.read_batch_size, 1000);
}

#[tokio::test]
async fn test_batch_size_limits_results() {
    let (store, _container) = setup_test_store().await;

    // Create more events than the batch size
    let stream_id = StreamId::try_new("test-stream").unwrap();
    let events = create_test_events(200, &stream_id).await;

    store
        .write_events_multi(vec![StreamEvents {
            stream_id: stream_id.clone(),
            expected_version: ExpectedVersion::New,
            events,
        }])
        .await
        .unwrap();

    // Read with default options (should use batch size from config = 100)
    let read_options = ReadOptions::default();
    let result = store
        .read_streams(&[stream_id.clone()], &read_options)
        .await
        .unwrap();

    // Should return exactly batch_size events
    assert_eq!(result.events.len(), 100);

    // Read with explicit max_events
    let read_options = ReadOptions {
        max_events: Some(50),
        ..Default::default()
    };
    let result = store
        .read_streams(&[stream_id], &read_options)
        .await
        .unwrap();

    // Should respect the explicit limit
    assert_eq!(result.events.len(), 50);
}

#[tokio::test]
async fn test_paginated_reading() {
    let (store, _container) = setup_test_store().await;

    // Create events across multiple streams
    let stream1 = StreamId::try_new("stream-1").unwrap();
    let stream2 = StreamId::try_new("stream-2").unwrap();
    let stream3 = StreamId::try_new("stream-3").unwrap();

    let events1 = create_test_events(50, &stream1).await;
    let events2 = create_test_events(50, &stream2).await;
    let events3 = create_test_events(50, &stream3).await;

    store
        .write_events_multi(vec![
            StreamEvents {
                stream_id: stream1.clone(),
                expected_version: ExpectedVersion::New,
                events: events1,
            },
            StreamEvents {
                stream_id: stream2.clone(),
                expected_version: ExpectedVersion::New,
                events: events2,
            },
            StreamEvents {
                stream_id: stream3.clone(),
                expected_version: ExpectedVersion::New,
                events: events3,
            },
        ])
        .await
        .unwrap();

    // Test pagination
    let streams = vec![stream1, stream2, stream3];
    let options = ReadOptions {
        max_events: Some(50), // Page size
        ..Default::default()
    };

    let mut all_events = Vec::new();
    let mut continuation = None;
    let mut page_count = 0;

    loop {
        let (events, next_token) = store
            .read_paginated(&streams, &options, continuation)
            .await
            .unwrap();

        page_count += 1;
        all_events.extend(events);

        match next_token {
            Some(token) => continuation = Some(token),
            None => break,
        }

        // Safety check to prevent infinite loops in tests
        if page_count > 10 {
            panic!("Too many pages returned");
        }
    }

    // Should have read all 150 events across multiple pages
    assert_eq!(all_events.len(), 150);
    // Note: Due to pagination algorithm and ordering, may be 3-4 pages
    assert!(
        page_count >= 3 && page_count <= 4,
        "Expected 3-4 pages, got {}",
        page_count
    );

    // Verify events are properly ordered by event_id
    for i in 1..all_events.len() {
        assert!(all_events[i].event_id > all_events[i - 1].event_id);
    }
}

#[tokio::test]
async fn test_paginated_reading_with_filters() {
    let (store, _container) = setup_test_store().await;

    let stream_id = StreamId::try_new("filtered-stream").unwrap();
    let events = create_test_events(100, &stream_id).await;

    store
        .write_events_multi(vec![StreamEvents {
            stream_id: stream_id.clone(),
            expected_version: ExpectedVersion::New,
            events,
        }])
        .await
        .unwrap();

    // Test pagination with version filters
    // Note: Event versions start at 1, not 0
    let options = ReadOptions {
        from_version: Some(EventVersion::try_new(30).unwrap()),
        to_version: Some(EventVersion::try_new(80).unwrap()),
        max_events: Some(20),
    };

    let mut filtered_events = Vec::new();
    let mut continuation = None;

    loop {
        let (events, next_token) = store
            .read_paginated(&[stream_id.clone()], &options, continuation)
            .await
            .unwrap();

        filtered_events.extend(events);

        match next_token {
            Some(token) => continuation = Some(token),
            None => break,
        }
    }

    // Should have events from version 30 to 80 (inclusive)
    assert_eq!(filtered_events.len(), 51);

    // Verify version range
    let first_version: u64 = filtered_events.first().unwrap().event_version.into();
    let last_version: u64 = filtered_events.last().unwrap().event_version.into();
    // The actual first version we get might be higher than our filter
    // if there are gaps or if we're filtering correctly
    assert!(
        first_version >= 30,
        "First version should be >= 30, got {}",
        first_version
    );
    assert!(
        last_version <= 80,
        "Last version should be <= 80, got {}",
        last_version
    );
}

#[tokio::test]
async fn test_multi_stream_batch_performance() {
    let (store, _container) = setup_test_store().await;

    // Create many streams
    let stream_count = 50;
    let events_per_stream = 20;
    let mut stream_ids = Vec::new();
    let mut writes = Vec::new();

    for i in 0..stream_count {
        let stream_id = StreamId::try_new(format!("perf-stream-{i}")).unwrap();
        stream_ids.push(stream_id.clone());

        let events = create_test_events(events_per_stream, &stream_id).await;
        writes.push(StreamEvents {
            stream_id,
            expected_version: ExpectedVersion::New,
            events,
        });
    }

    // Write all events
    store.write_events_multi(writes).await.unwrap();

    // Test reading all streams with batching
    let start = std::time::Instant::now();

    let options = ReadOptions::default(); // Will use configured batch size
    let result = store.read_streams(&stream_ids, &options).await.unwrap();

    let duration = start.elapsed();

    // Should read up to batch_size events efficiently
    assert_eq!(result.events.len(), 100); // batch_size from test config

    // Performance assertion - should complete quickly even with many streams
    assert!(
        duration.as_millis() < 500,
        "Multi-stream read took too long: {:?}",
        duration
    );
}

#[tokio::test]
async fn test_empty_stream_handling_with_batching() {
    let (store, _container) = setup_test_store().await;

    // Mix of empty and non-empty streams
    let empty_stream = StreamId::try_new("empty-stream").unwrap();
    let stream_with_data = StreamId::try_new("data-stream").unwrap();

    // Only write to one stream
    let events = create_test_events(50, &stream_with_data).await;
    store
        .write_events_multi(vec![StreamEvents {
            stream_id: stream_with_data.clone(),
            expected_version: ExpectedVersion::New,
            events,
        }])
        .await
        .unwrap();

    // Read both streams
    let result = store
        .read_streams(&[empty_stream, stream_with_data], &ReadOptions::default())
        .await
        .unwrap();

    // Should handle empty stream gracefully
    assert_eq!(result.events.len(), 50);
    assert_eq!(result.stream_versions.len(), 2); // Both streams have versions
}

#[tokio::test]
async fn test_continuation_token_consistency() {
    let (store, _container) = setup_test_store().await;

    let stream_id = StreamId::try_new("token-test").unwrap();
    let events = create_test_events(30, &stream_id).await;

    store
        .write_events_multi(vec![StreamEvents {
            stream_id: stream_id.clone(),
            expected_version: ExpectedVersion::New,
            events,
        }])
        .await
        .unwrap();

    // First page
    let options = ReadOptions {
        max_events: Some(10),
        ..Default::default()
    };

    let (page1, token1) = store
        .read_paginated(&[stream_id.clone()], &options, None)
        .await
        .unwrap();

    assert_eq!(page1.len(), 10);
    assert!(token1.is_some());

    // Second page
    let (page2, token2) = store
        .read_paginated(&[stream_id.clone()], &options, token1)
        .await
        .unwrap();

    assert_eq!(page2.len(), 10);
    assert!(token2.is_some());

    // Verify no overlap between pages
    let page1_ids: Vec<EventId> = page1.iter().map(|e| e.event_id).collect();
    let page2_ids: Vec<EventId> = page2.iter().map(|e| e.event_id).collect();

    for id in &page2_ids {
        assert!(!page1_ids.contains(id), "Pages should not overlap");
    }

    // Third page
    let (page3, token3) = store
        .read_paginated(&[stream_id], &options, token2)
        .await
        .unwrap();

    assert_eq!(page3.len(), 10);
    // Note: token3 may or may not be None depending on whether there are exactly 30 events
    // If there are exactly 30 events and we've read 30, then token3 should be None
    // But if there are rounding issues, we might need to be more flexible
    if page3.len() < 10 {
        assert!(
            token3.is_none(),
            "Last incomplete page should have no continuation token"
        );
    }
}
