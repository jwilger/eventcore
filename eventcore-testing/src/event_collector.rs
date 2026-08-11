//! Test utility for collecting events during projection for assertions.
//!
//! `EventCollector` implements the `Projector` trait and accumulates events
//! in an `Arc<Mutex<Vec<E>>>` for shared access during testing. This allows
//! test code to verify that commands produced expected events by running
//! a projection and inspecting the collected results.
//!
//! # Example
//!
//! ```ignore
//! use eventcore::{execute, run_projection, ProjectionConfig, RetryPolicy};
//! use std::sync::{Arc, Mutex};
//!
//! // `store` and `backend` are your `EventStore` (e.g. `InMemoryEventStore`);
//! // `command` implements `CommandLogic`; `MyEvent` is the event type.
//! execute(&store, command, RetryPolicy::new()).await?;
//!
//! let storage = Arc::new(Mutex::new(Vec::new()));
//! let collector = EventCollector::<MyEvent>::new(storage.clone());
//! run_projection(collector, &backend, ProjectionConfig::default()).await?;
//!
//! // Events accessible through the original storage handle
//! assert_eq!(storage.lock().unwrap().len(), expected_count);
//! ```

use eventcore_types::{Projector, StreamPosition};
use std::convert::Infallible;
use std::sync::{Arc, Mutex};

/// A projector that collects events for testing assertions.
///
/// `EventCollector` stores events in shared, thread-safe storage (`Arc<Mutex<Vec<E>>>`)
/// so that events can be inspected after projection completes. This is the primary
/// mechanism for black-box integration testing in EventCore.
///
/// # Type Parameters
///
/// - `E`: The event type to collect. Must be `Clone` so that `events()` can return
///   owned copies without consuming the collector.
///
/// # Thread Safety
///
/// The internal storage uses `Arc<Mutex<_>>` to allow the collector to be shared
/// across threads (e.g., between the projection runner and test assertions).
#[derive(Debug)]
pub struct EventCollector<E> {
    events: Arc<Mutex<Vec<E>>>,
}

impl<E> EventCollector<E> {
    /// Creates a new `EventCollector` with the provided shared storage.
    ///
    /// # Arguments
    ///
    /// * `storage` - An `Arc<Mutex<Vec<E>>>` that will hold collected events.
    ///   The same storage can be cloned before passing to enable access to
    ///   collected events after the collector is moved.
    pub fn new(storage: Arc<Mutex<Vec<E>>>) -> Self {
        Self { events: storage }
    }

    /// Returns a clone of all collected events.
    ///
    /// This method clones the internal vector, allowing inspection without
    /// consuming the collector. The `Clone` bound on `E` enables this behavior.
    pub fn events(&self) -> Vec<E>
    where
        E: Clone,
    {
        self.events
            .lock()
            .expect("EventCollector mutex poisoned - a test panicked while holding the lock")
            .clone()
    }
}

impl<E: Send + 'static> Projector for EventCollector<E> {
    type Event = E;
    type Error = Infallible;
    type Context = ();

    fn apply(
        &mut self,
        event: Self::Event,
        _position: StreamPosition,
        _ctx: &mut Self::Context,
    ) -> Result<(), Self::Error> {
        self.events
            .lock()
            .expect("EventCollector mutex poisoned - a test panicked while holding the lock")
            .push(event);
        Ok(())
    }

    fn name(&self) -> &str {
        "event-collector"
    }
}

#[cfg(test)]
#[path = "event_collector.test.rs"]
mod tests;
