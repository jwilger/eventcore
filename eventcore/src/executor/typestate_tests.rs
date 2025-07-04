//! Tests for enhanced type-safe command execution

#[cfg(test)]
mod tests {
    use crate::command::{CommandLogic, CommandStreams, ReadStreams, StreamResolver, StreamWrite};
    use crate::errors::CommandError;
    use crate::event_store::{EventStore, StoredEvent, StreamData, StreamEvents};
    use crate::executor::typestate::{states, *};
    use crate::executor::ExecutionOptions;
    use crate::types::{EventId, EventVersion, StreamId};
    use async_trait::async_trait;
    use std::collections::HashMap;

    // Test command for state machine testing
    #[derive(Clone)]
    struct TestCommand {
        account_id: String,
        amount: u64,
    }

    impl CommandStreams for TestCommand {
        type StreamSet = ();

        fn read_streams(&self) -> Vec<StreamId> {
            vec![StreamId::try_new(format!("account-{}", self.account_id)).unwrap()]
        }
    }

    #[async_trait]
    impl CommandLogic for TestCommand {
        type State = TestState;
        type Event = TestEvent;

        fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
            match &event.payload {
                TestEvent::Deposited { amount } => state.balance += amount,
                TestEvent::Withdrawn { amount } => state.balance -= amount,
            }
        }

        async fn handle(
            &self,
            _read_streams: ReadStreams<Self::StreamSet>,
            state: Self::State,
            _stream_resolver: &mut StreamResolver,
        ) -> Result<Vec<StreamWrite<Self::StreamSet, Self::Event>>, CommandError> {
            if state.balance < self.amount {
                return Err(CommandError::BusinessRuleViolation(
                    "Insufficient funds".to_string(),
                ));
            }

            Ok(vec![StreamWrite::new(
                &_read_streams,
                StreamId::try_new(format!("account-{}", self.account_id)).unwrap(),
                TestEvent::Withdrawn {
                    amount: self.amount,
                },
            )?])
        }
    }

    // Command trait is automatically implemented via blanket impl

    #[derive(Default)]
    struct TestState {
        balance: u64,
    }

    #[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
    enum TestEvent {
        Deposited { amount: u64 },
        Withdrawn { amount: u64 },
    }

    impl<'a> TryFrom<&'a Self> for TestEvent {
        type Error = String;

        fn try_from(value: &'a Self) -> Result<Self, Self::Error> {
            Ok(value.clone())
        }
    }

    // Mock event store for testing
    #[derive(Clone)]
    struct MockEventStore {
        events: HashMap<StreamId, Vec<StoredEvent<TestEvent>>>,
    }

    impl MockEventStore {
        fn new() -> Self {
            Self {
                events: HashMap::new(),
            }
        }

        fn with_events(mut self, stream_id: StreamId, events: Vec<TestEvent>) -> Self {
            let stored_events: Vec<StoredEvent<TestEvent>> = events
                .into_iter()
                .enumerate()
                .map(|(i, event)| {
                    StoredEvent::new(
                        EventId::new(),
                        stream_id.clone(),
                        EventVersion::try_new(i as u64).unwrap(),
                        crate::types::Timestamp::now(),
                        event,
                        Some(crate::metadata::EventMetadata::new()),
                    )
                })
                .collect();
            self.events.insert(stream_id, stored_events);
            self
        }
    }

    #[async_trait::async_trait]
    impl EventStore for MockEventStore {
        type Event = TestEvent;

        async fn read_streams(
            &self,
            stream_ids: &[StreamId],
            _options: &crate::event_store::ReadOptions,
        ) -> Result<StreamData<Self::Event>, crate::errors::EventStoreError> {
            let mut all_events = Vec::new();

            for stream_id in stream_ids {
                if let Some(events) = self.events.get(stream_id) {
                    all_events.extend(events.clone());
                }
            }

            let mut stream_versions = HashMap::new();
            for event in &all_events {
                stream_versions.insert(event.stream_id.clone(), event.event_version);
            }
            Ok(StreamData::new(all_events, stream_versions))
        }

        async fn write_events_multi(
            &self,
            _stream_events: Vec<StreamEvents<Self::Event>>,
        ) -> Result<HashMap<StreamId, EventVersion>, crate::errors::EventStoreError> {
            Ok(HashMap::new())
        }

        async fn stream_exists(
            &self,
            stream_id: &StreamId,
        ) -> Result<bool, crate::errors::EventStoreError> {
            Ok(self.events.contains_key(stream_id))
        }

        async fn get_stream_version(
            &self,
            stream_id: &StreamId,
        ) -> Result<Option<EventVersion>, crate::errors::EventStoreError> {
            Ok(self
                .events
                .get(stream_id)
                .and_then(|events| events.last())
                .map(|event| event.event_version))
        }

        async fn subscribe(
            &self,
            _options: crate::subscription::SubscriptionOptions,
        ) -> Result<
            Box<dyn crate::subscription::Subscription<Event = Self::Event>>,
            crate::errors::EventStoreError,
        > {
            unimplemented!("Subscriptions not needed for tests")
        }
    }

    // Compile-time tests to ensure state transitions are enforced

    #[test]
    fn test_valid_state_transitions_compile() {
        // This test verifies that valid state transitions compile

        fn _transition_initialized_to_validated(
            exec: CommandExecution<states::Initialized, TestCommand, MockEventStore>,
        ) -> Result<ValidatedExecution<TestCommand, MockEventStore>, CommandError> {
            exec.validate()
        }

        // Note: We can't test invalid transitions here because they won't compile,
        // which is exactly what we want!
    }

    #[tokio::test]
    async fn test_complete_execution_flow() {
        let command = TestCommand {
            account_id: "123".to_string(),
            amount: 50,
        };

        let stream_id = StreamId::try_new("account-123").unwrap();
        let event_store = MockEventStore::new().with_events(
            stream_id.clone(),
            vec![TestEvent::Deposited { amount: 100 }],
        );

        let options = ExecutionOptions::default();

        // Initialize
        let execution = CommandExecution::new(command, event_store, options);

        // Validate
        let validated = execution.validate().unwrap();

        // Read streams
        let streams_read = validated.read_streams().await.unwrap();

        // Reconstruct state
        let state_reconstructed = streams_read.reconstruct_state();

        // Execute command
        let mut stream_resolver = StreamResolver::new();
        let command_executed = state_reconstructed
            .execute_command(&mut stream_resolver)
            .await
            .unwrap();

        // Prepare events
        let events_prepared = command_executed.prepare_events();

        // Write events
        let events_written = events_prepared.write_events().await.unwrap();

        // Complete
        let completed = events_written.complete();

        assert!(completed.success);
        assert!(completed.error.is_none());
        assert_eq!(completed.metrics.streams_read, 1);
        assert_eq!(completed.metrics.events_processed, 1);
        assert_eq!(completed.metrics.events_written, 1);
    }

    #[tokio::test]
    async fn test_validation_failure() {
        // Test command that returns no streams
        #[derive(Clone)]
        struct InvalidCommand;

        impl CommandStreams for InvalidCommand {
            type StreamSet = ();

            fn read_streams(&self) -> Vec<StreamId> {
                vec![] // Invalid - no streams
            }
        }

        #[async_trait]
        impl CommandLogic for InvalidCommand {
            type State = ();
            type Event = TestEvent;

            fn apply(&self, _state: &mut Self::State, _event: &StoredEvent<Self::Event>) {}

            async fn handle(
                &self,
                _read_streams: ReadStreams<Self::StreamSet>,
                _state: Self::State,
                _stream_resolver: &mut StreamResolver,
            ) -> Result<Vec<StreamWrite<Self::StreamSet, Self::Event>>, CommandError> {
                Ok(vec![])
            }
        }

        // Command trait is automatically implemented via blanket impl

        let command = InvalidCommand;
        let event_store = MockEventStore::new();
        let options = ExecutionOptions::default();

        let execution = CommandExecution::new(command, event_store, options);
        let result = execution.validate();

        assert!(matches!(result, Err(CommandError::ValidationFailed(_))));
    }

    #[tokio::test]
    async fn test_business_rule_violation() {
        let command = TestCommand {
            account_id: "123".to_string(),
            amount: 150, // More than available balance
        };

        let stream_id = StreamId::try_new("account-123").unwrap();
        let event_store = MockEventStore::new().with_events(
            stream_id.clone(),
            vec![TestEvent::Deposited { amount: 100 }],
        );

        let options = ExecutionOptions::default();

        // Execute through to command execution
        let execution = CommandExecution::new(command, event_store, options);
        let validated = execution.validate().unwrap();
        let streams_read = validated.read_streams().await.unwrap();
        let state_reconstructed = streams_read.reconstruct_state();

        let mut stream_resolver = StreamResolver::new();
        let result = state_reconstructed
            .execute_command(&mut stream_resolver)
            .await;

        assert!(matches!(
            result,
            Err(CommandError::BusinessRuleViolation(_))
        ));
    }

    #[test]
    fn test_retry_state_transition() {
        let command = TestCommand {
            account_id: "123".to_string(),
            amount: 50,
        };

        let event_store = MockEventStore::new();
        let options = ExecutionOptions::default();
        let metrics = ExecutionMetrics::default();
        let error = CommandError::ValidationFailed("test error".to_string());

        // Create retryable state
        let retryable = CommandExecution::<states::Retryable, _, _>::from_error(
            command,
            event_store,
            options,
            metrics,
            error,
        );

        // Retry transitions back to Initialized
        let reinitialized = retryable.retry();

        // Should be able to validate again
        let _validated = reinitialized.validate().unwrap();
    }

    #[test]
    fn test_metrics_tracking() {
        let metrics = ExecutionMetrics::default();

        assert_eq!(metrics.streams_read, 0);
        assert_eq!(metrics.events_processed, 0);
        assert_eq!(metrics.events_written, 0);
        assert_eq!(metrics.retry_attempts, 0);
        assert_eq!(metrics.stream_read_duration, std::time::Duration::ZERO);
    }
}
