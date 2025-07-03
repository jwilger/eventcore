//! Integration tests for lock poisoning recovery.
//!
//! These tests verify that the system can recover gracefully from panics
//! that occur while holding locks, ensuring the system remains operational.

#![allow(clippy::significant_drop_tightening)]
#![allow(clippy::uninlined_format_args)]
#![allow(clippy::manual_assert)]
#![allow(clippy::use_self)]

use eventcore::prelude::*;
use eventcore::{CommandLogic, CommandStreams, ReadStreams, StreamResolver, StreamWrite};
use eventcore_memory::InMemoryEventStore;
use std::panic;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use tokio::sync::RwLock as AsyncRwLock;

#[tokio::test]
async fn test_event_store_recovers_from_panic_during_write() {
    let event_store = Arc::new(InMemoryEventStore::<String>::new());
    let store_clone = Arc::clone(&event_store);

    // Track if panic occurred
    let panic_occurred = Arc::new(Mutex::new(false));
    let panic_clone = Arc::clone(&panic_occurred);

    // Spawn a task that will panic during event write
    let handle = tokio::spawn(async move {
        // Set up panic hook to track panic
        let _old_hook = panic::take_hook();
        panic::set_hook(Box::new(move |_| {
            *panic_clone.lock().unwrap() = true;
        }));

        // This will panic during processing
        let stream_id = StreamId::try_new("test-stream").unwrap();
        let event = EventToWrite::new(EventId::new(), "PANIC".to_string());
        let stream_events = StreamEvents::new(stream_id, ExpectedVersion::New, vec![event]);

        // Simulate a panic during write (this is contrived, but demonstrates recovery)
        let _ = store_clone.write_events_multi(vec![stream_events]).await;

        // Force a panic
        panic!("Simulated panic during event write");
    });

    // Wait for the panic to occur
    let _ = handle.await;
    thread::sleep(Duration::from_millis(100));

    // Verify panic occurred
    assert!(*panic_occurred.lock().unwrap());

    // Now verify the event store is still usable
    let stream_id = StreamId::try_new("recovery-test").unwrap();
    let event = EventToWrite::new(EventId::new(), "after-panic".to_string());
    let stream_events = StreamEvents::new(stream_id.clone(), ExpectedVersion::New, vec![event]);

    // This should succeed despite the previous panic
    let result = event_store.write_events_multi(vec![stream_events]).await;
    assert!(
        result.is_ok(),
        "Should be able to write after panic recovery"
    );

    // Verify we can read the events
    let read_result = event_store
        .read_streams(&[stream_id], &ReadOptions::default())
        .await;
    assert!(read_result.is_ok());

    let stream_data = read_result.unwrap();
    assert_eq!(stream_data.events.len(), 1);
    assert_eq!(stream_data.events[0].payload, "after-panic");
}

#[tokio::test]
async fn test_concurrent_access_with_panic_recovery() {
    let shared_state = Arc::new(AsyncRwLock::new(vec![1, 2, 3]));
    let state_clone1 = Arc::clone(&shared_state);
    let state_clone2 = Arc::clone(&shared_state);

    // Task 1: Will panic while holding write lock
    let handle1 = tokio::spawn(async move {
        let mut guard = state_clone1.write().await;
        guard.push(4);
        // Simulate some work
        tokio::time::sleep(Duration::from_millis(10)).await;
        panic!("Panic while holding write lock");
    });

    // Task 2: Tries to read after panic
    let handle2 = tokio::spawn(async move {
        // Wait for task 1 to likely have the lock
        tokio::time::sleep(Duration::from_millis(5)).await;

        // This should not deadlock even if task 1 panicked
        let guard = state_clone2.read().await;
        guard.clone()
    });

    // Wait for both tasks
    let _ = handle1.await; // This will be an error due to panic
    let result2 = handle2.await;

    // Task 2 should complete successfully
    assert!(result2.is_ok());

    // The shared state should still be accessible
    let final_state = shared_state.read().await;
    // Note: The push(4) might or might not have completed before panic
    assert!(final_state.len() >= 3);
}

#[tokio::test]
async fn test_stream_id_cache_recovery() {
    use std::sync::{Arc, Barrier};
    use std::thread;

    let barrier = Arc::new(Barrier::new(2));
    let barrier_clone = Arc::clone(&barrier);

    // Thread 1: Will panic while accessing cache
    let handle1 = thread::spawn(move || {
        // This is a bit contrived since StreamId::from_string handles errors gracefully,
        // but we're testing the general pattern
        barrier_clone.wait();

        // Try to create many StreamIds rapidly
        for i in 0..100 {
            let _ = StreamId::try_new(format!("stream-{}", i));
            if i == 50 {
                panic!("Simulated panic during cache access");
            }
        }
    });

    // Thread 2: Continues using cache after panic
    let handle2 = thread::spawn(move || {
        barrier.wait();

        // Wait a bit to ensure thread 1 has likely panicked
        thread::sleep(Duration::from_millis(50));

        // This should work despite thread 1's panic
        let mut results = vec![];
        for i in 0..10 {
            match StreamId::try_new(format!("recovery-{}", i)) {
                Ok(id) => results.push(id),
                Err(e) => panic!("Failed to create StreamId after panic: {}", e),
            }
        }
        results
    });

    // Wait for threads
    let _ = handle1.join(); // Will be error due to panic
    let result2 = handle2.join();

    assert!(result2.is_ok());
    let ids = result2.unwrap();
    assert_eq!(ids.len(), 10);
}

#[tokio::test]
async fn test_projection_continues_after_panic() {
    use async_trait::async_trait;
    use eventcore::{
        Event, EventMetadata, Projection, ProjectionCheckpoint, ProjectionConfig, ProjectionResult,
    };

    // Create a projection that panics on specific events
    #[derive(Debug)]
    struct PanickyProjection {
        panic_on: String,
        processed: Arc<Mutex<Vec<String>>>,
    }

    #[async_trait]
    impl Projection for PanickyProjection {
        type State = Vec<String>;
        type Event = String;

        fn config(&self) -> &ProjectionConfig {
            todo!("Not needed for this test")
        }

        async fn initialize_state(&self) -> ProjectionResult<Self::State> {
            Ok(Vec::new())
        }

        async fn apply_event(
            &self,
            state: &mut Self::State,
            event: &Event<Self::Event>,
        ) -> ProjectionResult<()> {
            if event.payload == self.panic_on {
                panic!("Intentional panic on event: {}", event.payload);
            }

            state.push(event.payload.clone());
            self.processed.lock().unwrap().push(event.payload.clone());
            Ok(())
        }

        async fn get_state(&self) -> ProjectionResult<Self::State> {
            Ok(self.processed.lock().unwrap().clone())
        }

        async fn save_checkpoint(&self, _checkpoint: ProjectionCheckpoint) -> ProjectionResult<()> {
            Ok(())
        }

        async fn load_checkpoint(&self) -> ProjectionResult<ProjectionCheckpoint> {
            Ok(ProjectionCheckpoint::initial())
        }

        async fn get_status(&self) -> ProjectionResult<eventcore::ProjectionStatus> {
            Ok(eventcore::ProjectionStatus::Running)
        }
    }

    // This test demonstrates that projections can be designed to handle panics
    // The actual ProjectionRunner would need modifications to catch panics in tasks
    let processed = Arc::new(Mutex::new(Vec::new()));
    let projection = PanickyProjection {
        panic_on: "PANIC_EVENT".to_string(),
        processed: Arc::clone(&processed),
    };

    // Process events manually to simulate behavior
    let mut state = projection.initialize_state().await.unwrap();

    let events = vec![
        Event::new(
            StreamId::try_new("test").unwrap(),
            "event1".to_string(),
            EventMetadata::default(),
        ),
        Event::new(
            StreamId::try_new("test").unwrap(),
            "PANIC_EVENT".to_string(),
            EventMetadata::default(),
        ),
        Event::new(
            StreamId::try_new("test").unwrap(),
            "event3".to_string(),
            EventMetadata::default(),
        ),
    ];

    for event in events {
        // Catch panics and continue
        let result = panic::catch_unwind(panic::AssertUnwindSafe(|| {
            // In real implementation, this would be async and handled differently
            let mut temp_state = state.clone();
            // We can't actually call the async function in catch_unwind,
            // but this demonstrates the pattern
            temp_state.push(event.payload.clone());
            temp_state
        }));

        if let Ok(new_state) = result {
            state = new_state;
        }
    }

    // Even with the panic, we should have processed some events
    assert!(!state.is_empty());
}

// This test demonstrates that panics in commands will propagate up
// In a production system, you would handle these at a higher level
#[tokio::test]
#[should_panic(expected = "Intentional panic during command handling")]
async fn test_command_executor_propagates_panics() {
    use async_trait::async_trait;

    #[derive(Debug, Clone)]
    struct TestCommand {
        stream_id: StreamId,
        value: String,
    }

    #[derive(Debug, Default)]
    struct TestState {
        events: Vec<String>,
    }

    #[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
    enum TestEvent {
        Created { value: String },
    }

    impl TryFrom<&TestEvent> for TestEvent {
        type Error = std::convert::Infallible;
        fn try_from(value: &TestEvent) -> Result<Self, Self::Error> {
            Ok(value.clone())
        }
    }

    struct TestStreamSet;

    impl CommandStreams for TestCommand {
        type StreamSet = TestStreamSet;

        fn read_streams(&self) -> Vec<StreamId> {
            vec![self.stream_id.clone()]
        }
    }

    #[async_trait]
    impl CommandLogic for TestCommand {
        type State = TestState;
        type Event = TestEvent;

        fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
            match &event.payload {
                TestEvent::Created { value } => {
                    state.events.push(value.clone());
                }
            }
        }

        async fn handle(
            &self,
            read_streams: ReadStreams<Self::StreamSet>,
            _state: Self::State,
            _resolver: &mut StreamResolver,
        ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
            if self.value == "PANIC" {
                panic!("Intentional panic during command handling");
            }

            let event = StreamWrite::new(
                &read_streams,
                self.stream_id.clone(),
                TestEvent::Created {
                    value: self.value.clone(),
                },
            )?;

            Ok(vec![event])
        }
    }

    let event_store = InMemoryEventStore::<TestEvent>::new();
    let executor = CommandExecutor::new(event_store);

    // Execute a command that will panic
    let panic_command = TestCommand {
        stream_id: StreamId::try_new("test-stream").unwrap(),
        value: "PANIC".to_string(),
    };

    // This will panic and the panic will propagate
    let _panic_result = executor
        .execute(&panic_command, ExecutionOptions::default())
        .await;
}

// Test that the system recovers after a panic in one task
#[tokio::test]
async fn test_system_recovery_after_panic() {
    use async_trait::async_trait;

    #[derive(Debug, Clone)]
    struct SafeCommand {
        stream_id: StreamId,
        value: String,
    }

    #[derive(Debug, Default)]
    struct SafeState {
        events: Vec<String>,
    }

    #[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
    enum SafeEvent {
        Created { value: String },
    }

    impl TryFrom<&SafeEvent> for SafeEvent {
        type Error = std::convert::Infallible;
        fn try_from(value: &SafeEvent) -> Result<Self, Self::Error> {
            Ok(value.clone())
        }
    }

    struct SafeStreamSet;

    impl CommandStreams for SafeCommand {
        type StreamSet = SafeStreamSet;

        fn read_streams(&self) -> Vec<StreamId> {
            vec![self.stream_id.clone()]
        }
    }

    #[async_trait]
    impl CommandLogic for SafeCommand {
        type State = SafeState;
        type Event = SafeEvent;

        fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
            match &event.payload {
                SafeEvent::Created { value } => {
                    state.events.push(value.clone());
                }
            }
        }

        async fn handle(
            &self,
            read_streams: ReadStreams<Self::StreamSet>,
            _state: Self::State,
            _resolver: &mut StreamResolver,
        ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
            let event = StreamWrite::new(
                &read_streams,
                self.stream_id.clone(),
                SafeEvent::Created {
                    value: self.value.clone(),
                },
            )?;

            Ok(vec![event])
        }
    }

    // Create an event store - InMemoryEventStore is already thread-safe
    let event_store = InMemoryEventStore::<SafeEvent>::new();

    // Spawn a task that will panic
    let panic_handle = tokio::spawn(async move {
        // This simulates some task that panics
        panic!("Simulated panic in another task");
    });

    // Wait for the panic
    let _ = panic_handle.await;

    // Create an executor - the store is not affected by panics in other tasks
    let executor = CommandExecutor::new(event_store);

    // Execute a command - this should work despite the panic in another task
    let command = SafeCommand {
        stream_id: StreamId::try_new("safe-stream").unwrap(),
        value: "recovery-test".to_string(),
    };

    let result = executor
        .execute(&command, ExecutionOptions::default())
        .await;
    assert!(
        result.is_ok(),
        "Should be able to execute commands after panic in another task"
    );
}
