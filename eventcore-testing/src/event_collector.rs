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
//! use std::sync::{Arc, Mutex};
//!
//! execute(&mut store, &command).await?;
//!
//! let storage = Arc::new(Mutex::new(Vec::new()));
//! let collector = EventCollector::<MyEvent>::new(storage.clone());
//! run_projection(collector, &backend).await?;
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
pub struct EventCollector<E>
where
    E: Clone,
{
    events: Arc<Mutex<Vec<E>>>,
}

impl<E> EventCollector<E>
where
    E: Clone,
{
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
    pub fn events(&self) -> Vec<E> {
        self.events.lock().unwrap().clone()
    }
}

impl<E> Projector for EventCollector<E>
where
    E: Clone,
{
    type Event = E;
    type Error = Infallible;
    type Context = ();

    fn apply(
        &mut self,
        event: Self::Event,
        _position: StreamPosition,
        _ctx: &mut Self::Context,
    ) -> Result<(), Self::Error> {
        self.events.lock().unwrap().push(event);
        Ok(())
    }

    fn name(&self) -> &str {
        "event-collector"
    }
}

#[cfg(test)]
mod tests {
    use crate::event_collector::EventCollector;

    // Simple test event for unit tests
    #[derive(Debug, Clone, PartialEq)]
    struct TestEvent {
        id: u32,
    }

    #[test]
    fn new_collector_has_empty_events() {
        use std::sync::{Arc, Mutex};

        // Given: A newly created EventCollector
        let storage: Arc<Mutex<Vec<TestEvent>>> = Arc::new(Mutex::new(Vec::new()));
        let collector = EventCollector::new(storage);

        // When: We retrieve the events
        let events = collector.events();

        // Then: The events vector is empty
        assert!(events.is_empty());
    }

    #[test]
    fn collects_event_via_projector_apply() {
        use eventcore_types::{Projector, StreamPosition};
        use std::sync::{Arc, Mutex};
        use uuid::Uuid;

        // Given: An EventCollector
        let storage: Arc<Mutex<Vec<TestEvent>>> = Arc::new(Mutex::new(Vec::new()));
        let mut collector = EventCollector::new(storage);
        let event = TestEvent { id: 42 };
        let position = StreamPosition::new(Uuid::nil());

        // When: We apply an event via the Projector trait
        let result = collector.apply(event.clone(), position, &mut ());

        // Then: The apply succeeded and the event is collected
        assert!(result.is_ok());
        assert_eq!(collector.events(), vec![event]);
    }

    #[test]
    fn events_accessible_after_collector_moved() {
        use eventcore_types::{Projector, StreamPosition};
        use std::sync::{Arc, Mutex};
        use uuid::Uuid;

        // Given: Shared storage and a collector using that storage
        let storage: Arc<Mutex<Vec<TestEvent>>> = Arc::new(Mutex::new(Vec::new()));
        let collector = EventCollector::new(storage.clone());

        // When: Collector is moved (simulates move into run_projection) and events are applied
        let mut moved_collector = collector;
        let event = TestEvent { id: 99 };
        let position = StreamPosition::new(Uuid::nil());
        let _ = moved_collector.apply(event.clone(), position, &mut ());

        // Then: Events are accessible through the original storage handle
        let events = storage.lock().unwrap();
        assert_eq!(*events, vec![event]);
    }
}
