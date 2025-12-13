use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use eventcore::{
    CommandError, CommandLogic, CommandStreams, Event, EventStore, EventStoreError,
    EventStreamReader, EventStreamSlice, EventTypeName, InMemoryEventStore, NewEvents, RetryPolicy,
    StreamDeclarations, StreamId, StreamResolver, StreamVersion, StreamWrites, execute,
};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex as AsyncMutex;

#[tokio::test]
async fn process_payment_discovers_related_payment_stream_before_handling() {
    // Given: An order stream referencing an external payment method stream plus seeded payment events.
    let store = InMemoryEventStore::new();
    let order_stream = test_stream_id("orders/order-123");
    let payment_stream = test_stream_id("payment-methods/payment-789");

    seed_order_payment_link(&store, &order_stream, &payment_stream).await;
    seed_payment_method_history(&store, &payment_stream).await;

    let captured_state = Arc::new(Mutex::new(None));
    let command = ProcessPaymentCommand::new(order_stream.clone(), Arc::clone(&captured_state));

    // When: The developer executes the ProcessPayment command that should discover and load the payment stream via StreamResolver.
    let result = execute(&store, command, RetryPolicy::new()).await;

    // Then: The captured state should prove that dynamic stream discovery loaded payment events before handle().
    let final_state = captured_state
        .lock()
        .expect("capture mutex should not be poisoned")
        .clone()
        .expect("handle() should capture reconstructed checkout state");

    let actual = (
        result.is_ok(),
        final_state.payment_stream_loaded,
        final_state.payment_events_observed > 0,
    );
    let expected = (true, true, true);

    assert_eq!(
        actual, expected,
        "ProcessPayment should succeed and record payment events discovered via resolver",
    );
}

#[tokio::test]
async fn executor_registers_discovered_streams_for_optimistic_concurrency() {
    // Given: Same checkout fixture with linked order/payment streams and seeded history.
    let store = InMemoryEventStore::new();
    let order_stream = test_stream_id("orders/order-123");
    let payment_stream = test_stream_id("payment-methods/payment-789");

    seed_order_payment_link(&store, &order_stream, &payment_stream).await;
    seed_payment_method_history(&store, &payment_stream).await;

    let command = CaptureAcrossStreamsCommand::new(order_stream.clone());

    // When: The executor runs a command that emits events to both the declared and resolver-discovered streams.
    let result = execute(&store, command, RetryPolicy::new()).await;

    // Then: Both streams should contain the original seed plus the captured event without undeclared stream errors.
    let order_events = store
        .read_stream::<CheckoutEvent>(order_stream.clone())
        .await
        .expect("order stream read should succeed");
    let payment_events = store
        .read_stream::<CheckoutEvent>(payment_stream.clone())
        .await
        .expect("payment stream read should succeed");

    let actual = (result.is_ok(), order_events.len(), payment_events.len());
    let expected = (true, 2, 2);

    assert_eq!(
        actual, expected,
        "executor should register resolver-discovered streams before appending cross-stream events",
    );
}

#[tokio::test]
async fn executor_retries_when_discovered_stream_conflicts() {
    // Given: A conflict-injecting store with seeded order and payment history.
    let store = ConflictOnceStore::new();
    let order_stream = test_stream_id("orders/order-123");
    let payment_stream = test_stream_id("payment-methods/payment-789");

    seed_order_payment_link(store.inner_store(), &order_stream, &payment_stream).await;
    seed_payment_method_history(store.inner_store(), &payment_stream).await;

    let command = CaptureAcrossStreamsCommand::new(order_stream.clone());

    // When: The executor retries after a synthetic version conflict touching discovered streams.
    let result = execute(&store, command, RetryPolicy::new()).await;

    // Then: Execution should succeed after one retry, proving append_events was attempted twice.
    let actual = (result.is_ok(), store.append_attempts());
    let expected = (true, 2);

    assert_eq!(
        actual, expected,
        "executor should retry once after injected conflict affecting discovered streams",
    );
}

#[tokio::test]
async fn executor_reads_each_stream_once_during_discovery() {
    // Given: A counting store wrapping the in-memory store plus seeded order and payment streams.
    let store = CountingEventStore::new();
    let order_stream = test_stream_id("orders/order-456");
    let payment_stream = test_stream_id("payment-methods/payment-321");

    seed_order_payment_link(&store, &order_stream, &payment_stream).await;
    seed_payment_method_history(&store, &payment_stream).await;

    let captured_state = Arc::new(Mutex::new(None));
    let command = ProcessPaymentCommand::new(order_stream.clone(), Arc::clone(&captured_state));

    // When: The executor loads streams via StreamResolver and processes the command.
    let result = execute(&store, command, RetryPolicy::new()).await;

    // Then: Each stream should be read exactly once even when discovered dynamically.
    let actual = (
        result.is_ok(),
        store.read_count(&order_stream),
        store.read_count(&payment_stream),
    );
    let expected = (true, 1, 1);

    assert_eq!(
        actual, expected,
        "executor should only read declared and resolver-discovered streams once",
    );
}

struct ConflictOnceStore {
    inner: InMemoryEventStore,
    conflict_injected: Arc<AsyncMutex<bool>>,
    append_attempts: Arc<Mutex<usize>>,
}

impl ConflictOnceStore {
    fn new() -> Self {
        Self {
            inner: InMemoryEventStore::new(),
            conflict_injected: Arc::new(AsyncMutex::new(false)),
            append_attempts: Arc::new(Mutex::new(0)),
        }
    }

    fn inner_store(&self) -> &InMemoryEventStore {
        &self.inner
    }

    fn append_attempts(&self) -> usize {
        *self
            .append_attempts
            .lock()
            .expect("attempt counter mutex should not be poisoned")
    }
}

impl EventStore for ConflictOnceStore {
    async fn read_stream<E: Event>(
        &self,
        stream_id: StreamId,
    ) -> Result<EventStreamReader<E>, EventStoreError> {
        self.inner.read_stream(stream_id).await
    }

    async fn append_events(
        &self,
        writes: StreamWrites,
    ) -> Result<EventStreamSlice, EventStoreError> {
        {
            let mut attempts = self
                .append_attempts
                .lock()
                .expect("attempt counter mutex should not be poisoned");
            *attempts += 1;
        }

        let should_conflict = {
            let mut flag = self.conflict_injected.lock().await;
            if !*flag {
                *flag = true;
                true
            } else {
                false
            }
        };

        if should_conflict {
            return Err(EventStoreError::VersionConflict);
        }

        self.inner.append_events(writes).await
    }
}

struct CountingEventStore {
    inner: InMemoryEventStore,
    read_counts: Arc<Mutex<HashMap<String, usize>>>,
}

impl CountingEventStore {
    fn new() -> Self {
        Self {
            inner: InMemoryEventStore::new(),
            read_counts: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn read_count(&self, stream_id: &StreamId) -> usize {
        let counts = self
            .read_counts
            .lock()
            .expect("read count mutex should not be poisoned");
        counts.get(stream_id.as_ref()).copied().unwrap_or(0)
    }
}

impl EventStore for CountingEventStore {
    async fn read_stream<E: Event>(
        &self,
        stream_id: StreamId,
    ) -> Result<EventStreamReader<E>, EventStoreError> {
        let key = stream_id.to_string();
        let reader = self.inner.read_stream::<E>(stream_id).await;

        let mut counts = self
            .read_counts
            .lock()
            .expect("read count mutex should not be poisoned");
        *counts.entry(key).or_insert(0) += 1;

        reader
    }

    async fn append_events(
        &self,
        writes: StreamWrites,
    ) -> Result<EventStreamSlice, EventStoreError> {
        self.inner.append_events(writes).await
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
enum CheckoutEvent {
    OrderPaymentMethodLinked {
        order_stream: StreamId,
        payment_stream: StreamId,
    },
    PaymentMethodAuthorized {
        payment_stream: StreamId,
    },
    PaymentCaptured {
        order_stream: StreamId,
    },
    PaymentMethodCaptured {
        payment_stream: StreamId,
    },
}

impl Event for CheckoutEvent {
    fn stream_id(&self) -> &StreamId {
        match self {
            CheckoutEvent::OrderPaymentMethodLinked { order_stream, .. }
            | CheckoutEvent::PaymentCaptured { order_stream } => order_stream,
            CheckoutEvent::PaymentMethodAuthorized { payment_stream }
            | CheckoutEvent::PaymentMethodCaptured { payment_stream } => payment_stream,
        }
    }

    fn event_type_name(&self) -> EventTypeName {
        match self {
            CheckoutEvent::OrderPaymentMethodLinked { .. } => "OrderPaymentMethodLinked"
                .try_into()
                .expect("valid event type name"),
            CheckoutEvent::PaymentMethodAuthorized { .. } => "PaymentMethodAuthorized"
                .try_into()
                .expect("valid event type name"),
            CheckoutEvent::PaymentCaptured { .. } => {
                "PaymentCaptured".try_into().expect("valid event type name")
            }
            CheckoutEvent::PaymentMethodCaptured { .. } => "PaymentMethodCaptured"
                .try_into()
                .expect("valid event type name"),
        }
    }

    fn all_type_names() -> Vec<EventTypeName> {
        vec![
            "OrderPaymentMethodLinked"
                .try_into()
                .expect("valid event type name"),
            "PaymentMethodAuthorized"
                .try_into()
                .expect("valid event type name"),
            "PaymentCaptured".try_into().expect("valid event type name"),
            "PaymentMethodCaptured"
                .try_into()
                .expect("valid event type name"),
        ]
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct CheckoutState {
    order_events_observed: usize,
    discovered_payment_stream: Option<StreamId>,
    payment_stream_loaded: bool,
    payment_events_observed: usize,
}

impl CheckoutState {
    fn record(&mut self, event: &CheckoutEvent) {
        match event {
            CheckoutEvent::OrderPaymentMethodLinked { payment_stream, .. } => {
                self.order_events_observed += 1;
                self.discovered_payment_stream = Some(payment_stream.clone());
            }
            CheckoutEvent::PaymentMethodAuthorized { payment_stream }
            | CheckoutEvent::PaymentMethodCaptured { payment_stream } => {
                self.payment_stream_loaded = true;
                self.payment_events_observed += 1;
                self.discovered_payment_stream
                    .get_or_insert_with(|| payment_stream.clone());
            }
            CheckoutEvent::PaymentCaptured { .. } => {}
        }
    }
}

struct ProcessPaymentCommand {
    order_stream: StreamId,
    captured_state: Arc<Mutex<Option<CheckoutState>>>,
}

impl ProcessPaymentCommand {
    fn new(order_stream: StreamId, captured_state: Arc<Mutex<Option<CheckoutState>>>) -> Self {
        Self {
            order_stream,
            captured_state,
        }
    }
}

impl CommandStreams for ProcessPaymentCommand {
    fn stream_declarations(&self) -> StreamDeclarations {
        StreamDeclarations::try_from_streams(vec![self.order_stream.clone()])
            .expect("process payment declares the order stream statically")
    }
}

impl CommandLogic for ProcessPaymentCommand {
    type Event = CheckoutEvent;
    type State = CheckoutState;

    fn apply(&self, mut state: Self::State, event: &Self::Event) -> Self::State {
        state.record(event);
        state
    }

    fn handle(&self, state: Self::State) -> Result<NewEvents<Self::Event>, CommandError> {
        self.captured_state
            .lock()
            .expect("capture mutex should not be poisoned")
            .replace(state.clone());

        Ok(vec![CheckoutEvent::PaymentCaptured {
            order_stream: self.order_stream.clone(),
        }]
        .into())
    }

    fn stream_resolver(&self) -> Option<&dyn StreamResolver<Self::State>> {
        Some(self)
    }
}

impl StreamResolver<CheckoutState> for ProcessPaymentCommand {
    fn discover_related_streams(&self, state: &CheckoutState) -> Vec<StreamId> {
        state.discovered_payment_stream.iter().cloned().collect()
    }
}

struct CaptureAcrossStreamsCommand {
    order_stream: StreamId,
}

impl CaptureAcrossStreamsCommand {
    fn new(order_stream: StreamId) -> Self {
        Self { order_stream }
    }
}

impl CommandStreams for CaptureAcrossStreamsCommand {
    fn stream_declarations(&self) -> StreamDeclarations {
        StreamDeclarations::try_from_streams(vec![self.order_stream.clone()])
            .expect("capture command declares the order stream statically")
    }
}

impl CommandLogic for CaptureAcrossStreamsCommand {
    type Event = CheckoutEvent;
    type State = CheckoutState;

    fn apply(&self, mut state: Self::State, event: &Self::Event) -> Self::State {
        state.record(event);
        state
    }

    fn handle(&self, state: Self::State) -> Result<NewEvents<Self::Event>, CommandError> {
        let payment_stream = state
            .discovered_payment_stream
            .clone()
            .ok_or(CommandError::ValidationError)?;

        Ok(vec![
            CheckoutEvent::PaymentCaptured {
                order_stream: self.order_stream.clone(),
            },
            CheckoutEvent::PaymentMethodCaptured { payment_stream },
        ]
        .into())
    }

    fn stream_resolver(&self) -> Option<&dyn StreamResolver<Self::State>> {
        Some(self)
    }
}

impl StreamResolver<CheckoutState> for CaptureAcrossStreamsCommand {
    fn discover_related_streams(&self, state: &CheckoutState) -> Vec<StreamId> {
        state.discovered_payment_stream.iter().cloned().collect()
    }
}

async fn seed_order_payment_link<S: EventStore>(
    store: &S,
    order_stream: &StreamId,
    payment_stream: &StreamId,
) {
    let writes = StreamWrites::new()
        .register_stream(order_stream.clone(), StreamVersion::new(0))
        .and_then(|writes| {
            writes.append(CheckoutEvent::OrderPaymentMethodLinked {
                order_stream: order_stream.clone(),
                payment_stream: payment_stream.clone(),
            })
        })
        .expect("order stream seeding should register and append event");

    store
        .append_events(writes)
        .await
        .expect("order stream seed write succeeds");
}

async fn seed_payment_method_history<S: EventStore>(store: &S, payment_stream: &StreamId) {
    let writes = StreamWrites::new()
        .register_stream(payment_stream.clone(), StreamVersion::new(0))
        .and_then(|writes| {
            writes.append(CheckoutEvent::PaymentMethodAuthorized {
                payment_stream: payment_stream.clone(),
            })
        })
        .expect("payment stream seeding should register and append event");

    store
        .append_events(writes)
        .await
        .expect("payment stream seed write succeeds");
}

fn test_stream_id(value: &str) -> StreamId {
    StreamId::try_new(value.to_string()).expect("valid stream id for test fixtures")
}
