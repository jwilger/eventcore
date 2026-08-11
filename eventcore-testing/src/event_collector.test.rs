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

#[test]
fn projector_name_is_event_collector() {
    use eventcore_types::Projector;
    use std::sync::{Arc, Mutex};

    // Given: An EventCollector
    let storage: Arc<Mutex<Vec<TestEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let collector = EventCollector::new(storage);

    // When/Then: The projector name is "event-collector"
    assert_eq!(collector.name(), "event-collector");
}
