//! Given-When-Then testing helpers for eventcore commands.

use eventcore_memory::InMemoryEventStore;
use eventcore_types::{CommandLogic, Event, EventStore, StreamId, StreamVersion, StreamWrites};

/// Builder for Given-When-Then command tests.
pub struct TestScenario {
    store: InMemoryEventStore,
}

impl Default for TestScenario {
    fn default() -> Self {
        Self::new()
    }
}

impl TestScenario {
    /// Create a new test scenario with an empty event store.
    pub fn new() -> Self {
        Self {
            store: InMemoryEventStore::new(),
        }
    }

    /// Seed events into a stream as preconditions (the "Given" step).
    pub async fn given_events<E: Event>(self, stream_id: StreamId, events: Vec<E>) -> Self {
        if events.is_empty() {
            return self;
        }

        let reader = self
            .store
            .read_stream::<E>(stream_id.clone())
            .await
            .expect("reading stream for test setup should not fail");
        let current_version = StreamVersion::new(reader.len());

        let mut writes = StreamWrites::new()
            .register_stream(stream_id, current_version)
            .expect("registering stream for test setup should not fail");

        for event in events {
            writes = writes
                .append(event)
                .expect("appending event for test setup should not fail");
        }

        let _ = self
            .store
            .append_events(writes)
            .await
            .expect("appending events for test setup should not fail");

        self
    }

    /// Execute a command (the "When" step). Returns a result for assertions.
    pub async fn when<C>(self, command: C) -> ScenarioResult<C::Event>
    where
        C: CommandLogic,
        C::Event: Clone + PartialEq + std::fmt::Debug,
    {
        let result = eventcore::execute(&self.store, command, eventcore::RetryPolicy::new()).await;

        let storage: std::sync::Arc<std::sync::Mutex<Vec<C::Event>>> =
            std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let collector = crate::EventCollector::new(storage.clone());
        let _ = eventcore::run_projection(collector, &self.store).await;

        let all_events = storage.lock().unwrap().clone();

        ScenarioResult {
            result: result.map(|_| ()),
            all_events,
        }
    }
}

/// Result of executing a command in a test scenario (the "Then" step).
pub struct ScenarioResult<E> {
    result: Result<(), eventcore_types::CommandError>,
    all_events: Vec<E>,
}

impl<E: PartialEq + std::fmt::Debug> ScenarioResult<E> {
    /// Assert the command succeeded.
    pub fn succeeded(&self) -> &Self {
        assert!(
            self.result.is_ok(),
            "expected command to succeed, got: {:?}",
            self.result.as_ref().err()
        );
        self
    }

    /// Assert the command failed with a specific error.
    ///
    /// Accepts any error type that implements `Into<CommandError>`, matching
    /// the same pattern used with the `require!` macro. The error is converted
    /// to `CommandError` via `Into` and compared against the actual result.
    pub fn failed_with<Err: Into<eventcore_types::CommandError>>(&self, expected: Err) -> &Self {
        let expected_error = expected.into();
        match &self.result {
            Err(actual) => {
                assert_eq!(
                    actual.to_string(),
                    expected_error.to_string(),
                    "command error mismatch"
                );
            }
            Ok(()) => panic!(
                "expected command to fail with {}, but it succeeded",
                expected_error
            ),
        }
        self
    }

    /// Assert the events in the store match the expected list.
    pub fn then_events(&self, expected: Vec<E>) -> &Self {
        assert_eq!(
            self.all_events, expected,
            "events in store should match expected"
        );
        self
    }

    /// Assert the number of events in the store.
    pub fn then_event_count(&self, expected: usize) -> &Self {
        assert_eq!(
            self.all_events.len(),
            expected,
            "expected {} events, found {}",
            expected,
            self.all_events.len()
        );
        self
    }
}
