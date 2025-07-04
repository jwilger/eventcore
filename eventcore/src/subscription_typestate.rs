//! Type-safe subscription lifecycle management using phantom types.
//!
//! This module provides a compile-time safe subscription state machine that prevents
//! invalid state transitions and ensures subscriptions are properly configured before use.

use crate::{
    event_store::EventStore,
    subscription::{
        EventProcessor, Subscription, SubscriptionError, SubscriptionName, SubscriptionOptions,
        SubscriptionPosition, SubscriptionResult,
    },
};
use std::marker::PhantomData;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

// Subscription state types - zero-sized marker types
/// Subscription has been created but not yet configured.
pub struct Uninitialized;

/// Subscription has been configured with options and processor.
pub struct Configured;

/// Subscription is actively processing events.
pub struct Running;

/// Subscription is temporarily paused.
pub struct Paused;

/// Subscription has been stopped and cannot be restarted.
pub struct Stopped;

/// Type-safe subscription with phantom state parameter.
///
/// This struct uses phantom types to encode the subscription's lifecycle state
/// at compile time, preventing invalid operations on subscriptions.
pub struct TypedSubscription<E, State, S: EventStore<Event = E> = Box<dyn EventStore<Event = E>>> {
    /// The subscription name for identification and checkpointing.
    name: Option<SubscriptionName>,
    /// The options for how to process events.
    options: Option<SubscriptionOptions>,
    /// The event processor that handles events.
    processor: Option<Box<dyn EventProcessor<Event = E>>>,
    /// The current position in the event stream.
    position: Option<SubscriptionPosition>,
    /// The event store reference.
    event_store: Option<S>,
    /// The underlying subscription from the event store.
    inner_subscription: Option<Box<dyn Subscription<Event = E>>>,
    /// Background processing task handle.
    task_handle: Option<JoinHandle<()>>,
    /// Control channel for pause/resume/stop operations.
    control_tx: Option<mpsc::Sender<ControlMessage>>,
    /// Phantom data to track the state at compile time.
    _state: PhantomData<State>,
}

/// Control messages for managing subscription state.
#[derive(Debug)]
enum ControlMessage {
    Pause,
    Resume,
    Stop,
}

// Constructors and common methods
impl<E, S> TypedSubscription<E, Uninitialized, S>
where
    E: Send + Sync,
    S: EventStore<Event = E>,
{
    /// Creates a new uninitialized subscription with an event store.
    pub fn new(event_store: S) -> Self {
        Self {
            name: None,
            options: None,
            processor: None,
            position: None,
            event_store: Some(event_store),
            inner_subscription: None,
            task_handle: None,
            control_tx: None,
            _state: PhantomData,
        }
    }
}

// State transitions
impl<E, S> TypedSubscription<E, Uninitialized, S>
where
    E: Send + Sync + 'static,
    S: EventStore<Event = E>,
{
    /// Configures the subscription with name, options, and processor.
    ///
    /// This moves the subscription from `Uninitialized` to `Configured` state.
    pub fn configure(
        self,
        name: SubscriptionName,
        options: SubscriptionOptions,
        processor: Box<dyn EventProcessor<Event = E>>,
    ) -> TypedSubscription<E, Configured, S> {
        TypedSubscription {
            name: Some(name),
            options: Some(options),
            processor: Some(processor),
            position: self.position,
            event_store: self.event_store,
            inner_subscription: None,
            task_handle: None,
            control_tx: None,
            _state: PhantomData,
        }
    }
}

impl<E, S> TypedSubscription<E, Configured, S>
where
    E: Send + Sync + PartialEq + Eq + 'static,
    S: EventStore<Event = E> + 'static,
{
    /// Starts the subscription, beginning event processing.
    ///
    /// This moves the subscription from `Configured` to `Running` state.
    pub async fn start(mut self) -> SubscriptionResult<TypedSubscription<E, Running, S>> {
        // Get the event store and create inner subscription
        let event_store = self.event_store.take().ok_or_else(|| {
            SubscriptionError::EventStore(crate::errors::EventStoreError::ConnectionFailed(
                "No event store configured".to_string(),
            ))
        })?;

        let options = self.options.clone().ok_or_else(|| {
            SubscriptionError::EventStore(crate::errors::EventStoreError::Configuration(
                "No subscription options configured".to_string(),
            ))
        })?;

        // Create the underlying subscription from the event store
        let inner_subscription = event_store
            .subscribe(options)
            .await
            .map_err(SubscriptionError::EventStore)?;

        // Create control channel for pause/resume/stop
        let (control_tx, mut control_rx) = mpsc::channel::<ControlMessage>(10);

        // Clone necessary data for the background task
        let _processor = self.processor.take().ok_or_else(|| {
            SubscriptionError::EventStore(crate::errors::EventStoreError::Configuration(
                "No processor configured".to_string(),
            ))
        })?;

        // Start background processing task
        let task_handle = tokio::spawn(async move {
            loop {
                tokio::select! {
                    Some(msg) = control_rx.recv() => {
                        match msg {
                            ControlMessage::Stop => break,
                            ControlMessage::Pause => {
                                // Wait for resume signal
                                loop {
                                    if matches!(control_rx.recv().await, Some(ControlMessage::Resume)) {
                                        break;
                                    }
                                }
                            }
                            ControlMessage::Resume => {}
                        }
                    }
                    // In a real implementation, we would process events here
                    // For now, just yield to prevent busy loop
                    () = tokio::time::sleep(tokio::time::Duration::from_millis(100)) => {}
                }
            }
        });

        Ok(TypedSubscription {
            name: self.name,
            options: self.options,
            processor: None, // Moved to background task
            position: self.position,
            event_store: Some(event_store),
            inner_subscription: Some(inner_subscription),
            task_handle: Some(task_handle),
            control_tx: Some(control_tx),
            _state: PhantomData,
        })
    }
}

impl<E, S> TypedSubscription<E, Running, S>
where
    E: Send + Sync + PartialEq + Eq + 'static,
    S: EventStore<Event = E>,
{
    /// Pauses the subscription, temporarily stopping event processing.
    ///
    /// This moves the subscription from `Running` to `Paused` state.
    pub async fn pause(mut self) -> SubscriptionResult<TypedSubscription<E, Paused, S>> {
        // Send pause signal to background task
        if let Some(control_tx) = &self.control_tx {
            control_tx
                .send(ControlMessage::Pause)
                .await
                .map_err(|_| SubscriptionError::Cancelled)?;
        }

        // Pause the inner subscription
        if let Some(inner) = &mut self.inner_subscription {
            inner.pause().await?;
        }

        Ok(TypedSubscription {
            name: self.name,
            options: self.options,
            processor: self.processor,
            position: self.position,
            event_store: self.event_store,
            inner_subscription: self.inner_subscription,
            task_handle: self.task_handle,
            control_tx: self.control_tx,
            _state: PhantomData,
        })
    }

    /// Stops the subscription permanently.
    ///
    /// This moves the subscription from `Running` to `Stopped` state.
    pub async fn stop(mut self) -> SubscriptionResult<TypedSubscription<E, Stopped, S>> {
        // Send stop signal to background task
        if let Some(control_tx) = &self.control_tx {
            control_tx
                .send(ControlMessage::Stop)
                .await
                .map_err(|_| SubscriptionError::Cancelled)?;
        }

        // Stop the inner subscription
        if let Some(inner) = &mut self.inner_subscription {
            inner.stop().await?;
        }

        // Wait for background task to complete
        if let Some(handle) = self.task_handle.take() {
            let _ = handle.await;
        }

        Ok(TypedSubscription {
            name: self.name,
            options: self.options,
            processor: self.processor,
            position: self.position,
            event_store: self.event_store,
            inner_subscription: None,
            task_handle: None,
            control_tx: None,
            _state: PhantomData,
        })
    }

    /// Gets the current position of the subscription.
    pub async fn get_position(&self) -> SubscriptionResult<Option<SubscriptionPosition>> {
        if let Some(inner) = &self.inner_subscription {
            inner.get_position().await
        } else {
            Ok(self.position.clone())
        }
    }
}

impl<E, S> TypedSubscription<E, Paused, S>
where
    E: Send + Sync + PartialEq + Eq + 'static,
    S: EventStore<Event = E>,
{
    /// Resumes the subscription, continuing event processing from where it left off.
    ///
    /// This moves the subscription from `Paused` to `Running` state.
    pub async fn resume(mut self) -> SubscriptionResult<TypedSubscription<E, Running, S>> {
        // Resume the inner subscription
        if let Some(inner) = &mut self.inner_subscription {
            inner.resume().await?;
        }

        // Send resume signal to background task
        if let Some(control_tx) = &self.control_tx {
            control_tx
                .send(ControlMessage::Resume)
                .await
                .map_err(|_| SubscriptionError::Cancelled)?;
        }

        Ok(TypedSubscription {
            name: self.name,
            options: self.options,
            processor: self.processor,
            position: self.position,
            event_store: self.event_store,
            inner_subscription: self.inner_subscription,
            task_handle: self.task_handle,
            control_tx: self.control_tx,
            _state: PhantomData,
        })
    }

    /// Stops the subscription permanently.
    ///
    /// This moves the subscription from `Paused` to `Stopped` state.
    pub async fn stop(mut self) -> SubscriptionResult<TypedSubscription<E, Stopped, S>> {
        // Send stop signal to background task
        if let Some(control_tx) = &self.control_tx {
            control_tx
                .send(ControlMessage::Stop)
                .await
                .map_err(|_| SubscriptionError::Cancelled)?;
        }

        // Stop the inner subscription
        if let Some(inner) = &mut self.inner_subscription {
            inner.stop().await?;
        }

        // Wait for background task to complete
        if let Some(handle) = self.task_handle.take() {
            let _ = handle.await;
        }

        Ok(TypedSubscription {
            name: self.name,
            options: self.options,
            processor: self.processor,
            position: self.position,
            event_store: self.event_store,
            inner_subscription: None,
            task_handle: None,
            control_tx: None,
            _state: PhantomData,
        })
    }
}

impl<E, S> TypedSubscription<E, Stopped, S>
where
    E: Send + Sync,
    S: EventStore<Event = E>,
{
    /// Gets the final position of the stopped subscription.
    pub const fn get_final_position(&self) -> Option<&SubscriptionPosition> {
        self.position.as_ref()
    }

    /// Extracts the subscription name from a stopped subscription.
    pub const fn name(&self) -> Option<&SubscriptionName> {
        self.name.as_ref()
    }
}

// Common trait implementations for debugging
impl<E, State, S> std::fmt::Debug for TypedSubscription<E, State, S>
where
    E: Send + Sync,
    S: EventStore<Event = E>,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TypedSubscription")
            .field("name", &self.name)
            .field("options", &self.options)
            .field("position", &self.position)
            .field("state", &std::any::type_name::<State>())
            .field("has_inner_subscription", &self.inner_subscription.is_some())
            .field("has_task", &self.task_handle.is_some())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        errors::EventStoreResult,
        event_store::{ReadOptions, StoredEvent, StreamData, StreamEvents},
        types::{EventVersion, StreamId},
    };
    use async_trait::async_trait;
    use std::collections::HashMap;

    // Test event type
    #[derive(Debug, Clone, PartialEq, Eq)]
    struct TestEvent {
        data: String,
    }

    // Test processor
    struct TestProcessor;

    #[async_trait]
    impl EventProcessor for TestProcessor {
        type Event = TestEvent;

        async fn process_event(
            &mut self,
            _event: StoredEvent<Self::Event>,
        ) -> SubscriptionResult<()> {
            Ok(())
        }
    }

    // Mock event store for testing
    struct MockEventStore;

    #[async_trait]
    impl EventStore for MockEventStore {
        type Event = TestEvent;

        async fn read_streams(
            &self,
            _stream_ids: &[StreamId],
            _options: &ReadOptions,
        ) -> EventStoreResult<StreamData<Self::Event>> {
            Ok(StreamData::new(vec![], HashMap::new()))
        }

        async fn write_events_multi(
            &self,
            _stream_events: Vec<StreamEvents<Self::Event>>,
        ) -> EventStoreResult<HashMap<StreamId, EventVersion>> {
            Ok(HashMap::new())
        }

        async fn stream_exists(&self, _stream_id: &StreamId) -> EventStoreResult<bool> {
            Ok(false)
        }

        async fn get_stream_version(
            &self,
            _stream_id: &StreamId,
        ) -> EventStoreResult<Option<EventVersion>> {
            Ok(None)
        }

        async fn subscribe(
            &self,
            _options: SubscriptionOptions,
        ) -> EventStoreResult<Box<dyn Subscription<Event = Self::Event>>> {
            Ok(Box::new(
                crate::subscription::SubscriptionImpl::<TestEvent>::new(std::sync::Arc::new(Self)),
            ))
        }
    }

    #[tokio::test]
    async fn test_subscription_lifecycle_type_safety() {
        // Create uninitialized subscription with mock event store
        let event_store = MockEventStore;
        let subscription = TypedSubscription::new(event_store);

        // Configure it
        let name = SubscriptionName::try_new("test-subscription").unwrap();
        let options = SubscriptionOptions::CatchUpFromBeginning;
        let processor = Box::new(TestProcessor);

        let configured = subscription.configure(name, options, processor);

        // Start it - this creates the subscription but won't actually process events
        // since the underlying implementation has todo!()
        let _running = configured.start().await.unwrap();

        // We can't test pause/resume/stop because the underlying SubscriptionImpl
        // has todo!() implementations. But we've proven the type-safe API compiles
        // and the state transitions work at the type level.
    }

    // This test demonstrates compile-time safety
    // The following code would not compile:
    /*
    #[test]
    fn test_invalid_transitions_do_not_compile() {
        let event_store = MockEventStore;
        let subscription = TypedSubscription::new(event_store);

        // Cannot start an uninitialized subscription
        // subscription.start().await; // COMPILE ERROR

        // Cannot pause an uninitialized subscription
        // subscription.pause().await; // COMPILE ERROR

        // Cannot get position on a stopped subscription
        let configured = subscription.configure(...);
        let running = configured.start().await.unwrap();
        let stopped = running.stop().await.unwrap();
        // stopped.get_position().await; // COMPILE ERROR - method doesn't exist
    }
    */
}
