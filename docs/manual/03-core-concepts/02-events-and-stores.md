# Chapter 3.2: Events and Event Stores

Events are the heart of EventCore - immutable records of things that happened in your system. This chapter explores event design, storage, and the guarantees EventCore provides.

## What Makes a Good Event?

Events should be:

1. **Past Tense** - They record what happened, not what should happen
2. **Immutable** - Once written, events never change
3. **Self-Contained** - Include all necessary data
4. **Business-Focused** - Represent domain concepts, not technical details

### Event Design Principles

```rust
use eventcore::StreamId;
use eventcore_macros::Event;
use serde::{Serialize, Deserialize};

// ❌ Bad: Technical focus, present tense, missing context
#[derive(Serialize, Deserialize)]
struct UpdateUser {
    id: String,
    data: HashMap<String, Value>,
}

// ✅ Good: Business focus, past tense, complete information
// Use #[derive(Event)] with #[stream] to mark the aggregate identity
#[derive(Event, Debug, Clone, Serialize, Deserialize)]
struct CustomerEmailChanged {
    #[stream]
    customer_id: StreamId,
    old_email: Email,
    new_email: Email,
    changed_by: UserId,
    changed_at: DateTime<Utc>,
    reason: EmailChangeReason,
}
```

> **Note:** The `#[derive(Event)]` macro is the recommended way to implement the `Event` trait.
> It automatically generates `stream_id()`, `event_type_name()`, and `all_type_names()` methods.

## Event Structure in EventCore

### Core Event Types

```rust
use eventcore::StreamId;
use eventcore_macros::Event;
use serde::{Serialize, Deserialize};

/// Your domain event using the Event derive macro
#[derive(Event, Debug, Clone, Serialize, Deserialize)]
pub struct OrderShipped {
    #[stream]
    pub order_id: StreamId,
    pub tracking_number: TrackingNumber,
    pub carrier: Carrier,
    pub shipped_at: DateTime<Utc>,
}

/// Event ready to be written
pub struct EventToWrite<E> {
    pub stream_id: StreamId,
    pub payload: E,
    pub metadata: Option<EventMetadata>,
    pub expected_version: ExpectedVersion,
}

/// Event as stored in the event store
pub struct StoredEvent<E> {
    pub id: EventId,                  // UUIDv7 for global ordering
    pub stream_id: StreamId,          // Which stream this belongs to
    pub version: EventVersion,        // Position in the stream
    pub payload: E,                   // Your domain event
    pub metadata: EventMetadata,      // Who, when, why
    pub occurred_at: DateTime<Utc>,   // When it happened
}
```

### Event IDs and Ordering

EventCore uses UUIDv7 for event IDs, providing:

```rust
// UUIDv7 properties:
// - Globally unique
// - Time-ordered (sortable)
// - Millisecond precision timestamp
// - No coordination required

let event1 = EventId::new();
let event2 = EventId::new();

// Later events have higher IDs
assert!(event2 > event1);

// Extract timestamp
let timestamp = event1.timestamp();
```

### Event Metadata

Every event carries metadata for auditing and debugging:

```rust
pub struct EventMetadata {
    /// Who triggered this event
    pub user_id: Option<UserId>,

    /// Correlation ID for tracking across services
    pub correlation_id: CorrelationId,

    /// What caused this event (previous event ID)
    pub causation_id: Option<CausationId>,

    /// Custom metadata
    pub custom: HashMap<String, Value>,
}

// Building metadata
let metadata = EventMetadata::new()
    .with_user_id(UserId::from("alice@example.com"))
    .with_correlation_id(CorrelationId::new())
    .caused_by(&previous_event)
    .with_custom("ip_address", "192.168.1.1")
    .with_custom("user_agent", "MyApp/1.0");
```

## Event Store Abstraction

EventCore defines a trait that storage adapters implement:

```rust
#[async_trait]
pub trait EventStore: Send + Sync {
    type Event: Send + Sync;
    type Error: Error + Send + Sync;

    /// Read events from a specific stream
    async fn read_stream(
        &self,
        stream_id: &StreamId,
        options: ReadOptions,
    ) -> Result<StreamEvents<Self::Event>, Self::Error>;

    /// Read events from multiple streams
    async fn read_streams(
        &self,
        stream_ids: &[StreamId],
        options: ReadOptions,
    ) -> Result<Vec<StreamEvents<Self::Event>>, Self::Error>;

    /// Write events atomically to multiple streams
    async fn write_events(
        &self,
        events: Vec<EventToWrite<Self::Event>>,
    ) -> Result<WriteResult, Self::Error>;

    /// Subscribe to real-time events
    async fn subscribe(
        &self,
        options: SubscriptionOptions,
    ) -> Result<Box<dyn EventSubscription<Self::Event>>, Self::Error>;
}
```

## Stream Versioning

Streams maintain version numbers for optimistic concurrency:

```rust
pub struct StreamEvents<E> {
    pub stream_id: StreamId,
    pub version: EventVersion,    // Current version after these events
    pub events: Vec<StoredEvent<E>>,
}

// Version control options
pub enum ExpectedVersion {
    /// Stream must not exist
    NoStream,

    /// Stream must be at this exact version
    Exact(EventVersion),

    /// Stream must exist but any version is OK
    Any,

    /// No version check (dangerous!)
    NoCheck,
}
```

### Using Version Control

```rust
// First write - stream shouldn't exist
let first_event = EventToWrite {
    stream_id: stream_id.clone(),
    payload: AccountOpened { /* ... */ },
    metadata: None,
    expected_version: ExpectedVersion::NoStream,
};

// Subsequent writes - check version
let next_event = EventToWrite {
    stream_id: stream_id.clone(),
    payload: MoneyDeposited { /* ... */ },
    metadata: None,
    expected_version: ExpectedVersion::Exact(EventVersion::new(1)),
};
```

## Storage Adapters

### PostgreSQL Adapter

The production-ready adapter with ACID guarantees:

```rust
use eventcore_postgres::{PostgresEventStore, PostgresConfig};

let config = PostgresConfig::new("postgresql://localhost/eventcore")
    .with_pool_size(20)
    .with_schema("eventcore");

let event_store = PostgresEventStore::new(config).await?;

// Initialize schema (one time)
event_store.initialize().await?;
```

PostgreSQL schema:

```sql
-- Events table with optimal indexing
CREATE TABLE events (
    id UUID PRIMARY KEY DEFAULT gen_uuidv7(),
    stream_id VARCHAR(255) NOT NULL,
    version BIGINT NOT NULL,
    event_type VARCHAR(255) NOT NULL,
    payload JSONB NOT NULL,
    metadata JSONB NOT NULL,
    occurred_at TIMESTAMPTZ NOT NULL,

    -- Ensure stream version uniqueness
    UNIQUE(stream_id, version),

    -- Indexes for common queries
    INDEX idx_stream_id (stream_id),
    INDEX idx_occurred_at (occurred_at),
    INDEX idx_event_type (event_type)
);
```

### In-Memory Adapter

Perfect for testing and development:

```rust
use eventcore_memory::InMemoryEventStore;

let event_store = InMemoryEventStore::<MyEvent>::new();

// Optionally add chaos for testing
let chaotic_store = event_store
    .with_chaos(ChaosConfig {
        failure_probability: 0.1,  // 10% chance of failure
        latency_ms: Some(50..200), // Random latency
    });
```

## Event Design Patterns

### Event Granularity

Choose the right level of detail:

```rust
// ❌ Too coarse - loses important details
struct OrderUpdated {
    order_id: OrderId,
    new_state: OrderState,  // What actually changed?
}

// ❌ Too fine - creates event spam
struct OrderFieldUpdated {
    order_id: OrderId,
    field_name: String,
    old_value: Value,
    new_value: Value,
}

// ✅ Just right - meaningful business events
enum OrderEvent {
    OrderPlaced { customer: CustomerId, items: Vec<Item> },
    PaymentReceived { amount: Money, method: PaymentMethod },
    OrderShipped { tracking: TrackingNumber },
    OrderDelivered { signed_by: String },
}
```

### Event Evolution

Design events to evolve gracefully:

```rust
// Version 1
#[derive(Serialize, Deserialize)]
struct UserRegistered {
    user_id: UserId,
    email: Email,
}

// Version 2 - Added field with default
#[derive(Serialize, Deserialize)]
struct UserRegistered {
    user_id: UserId,
    email: Email,
    #[serde(default)]
    referral_code: Option<String>,  // New field
}

// Version 3 - Structural change
#[derive(Serialize, Deserialize)]
#[serde(tag = "version")]
enum UserRegisteredVersioned {
    #[serde(rename = "1")]
    V1 { user_id: UserId, email: Email },

    #[serde(rename = "2")]
    V2 {
        user_id: UserId,
        email: Email,
        referral_code: Option<String>,
    },

    #[serde(rename = "3")]
    V3 {
        user_id: UserId,
        email: Email,
        referral: Option<ReferralInfo>,  // Richer type
    },
}
```

### Event Enrichment

Add context to events:

```rust
trait EventEnricher {
    fn enrich<E>(&self, event: E) -> EnrichedEvent<E>;
}

struct EnrichedEvent<E> {
    pub event: E,
    pub context: EventContext,
}

struct EventContext {
    pub session_id: SessionId,
    pub request_id: RequestId,
    pub feature_flags: HashMap<String, bool>,
    pub environment: Environment,
}
```

## Querying Events

### Read Options

Control how events are read:

```rust
let options = ReadOptions::default()
    .from_version(EventVersion::new(10))    // Start from version 10
    .to_version(EventVersion::new(20))      // Up to version 20
    .max_events(100)                        // Limit results
    .backwards();                           // Read in reverse

let events = event_store
    .read_stream(&stream_id, options)
    .await?;
```

### Reading Multiple Streams

For multi-stream operations:

```rust
let stream_ids = vec![
    StreamId::from_static("order-123"),
    StreamId::from_static("inventory-abc"),
    StreamId::from_static("payment-xyz"),
];

let all_events = event_store
    .read_streams(&stream_ids, ReadOptions::default())
    .await?;

// Events from all streams, ordered by EventId (time)
```

### Global Event Feed

Read all events across all streams:

```rust
let all_events = event_store
    .read_all_events(
        ReadOptions::default()
            .after(last_known_event_id)  // For pagination
            .max_events(1000)
    )
    .await?;
```

## Event Store Guarantees

### 1. Atomicity

All events in a write operation succeed or fail together:

```rust
let events = vec![
    EventToWrite { /* withdraw from account A */ },
    EventToWrite { /* deposit to account B */ },
];

// Both events written atomically
event_store.write_events(events).await?;
```

### 2. Consistency

Version checks prevent conflicting writes:

```rust
// Two concurrent commands read version 5
let command1_events = vec![/* ... */];
let command2_events = vec![/* ... */];

// First write succeeds
event_store.write_events(command1_events).await?;  // OK

// Second write fails - version conflict
event_store.write_events(command2_events).await?;  // Error: Version conflict
```

### 3. Durability

Events are persisted before returning success:

```rust
// After this returns, events are durable
let result = event_store.write_events(events).await?;

// Even if the process crashes, events are safe
```

### 4. Ordering

Events maintain both stream order and global order:

```rust
// Stream order: version within a stream
stream_events.events[0].version < stream_events.events[1].version

// Global order: EventId across all streams
all_events[0].id < all_events[1].id
```

## Performance Optimization

### Batch Writing

Write multiple events efficiently:

```rust
// Batch events for better performance
let mut batch = Vec::with_capacity(1000);

for item in large_dataset {
    batch.push(EventToWrite {
        stream_id: compute_stream_id(&item),
        payload: process_item(item),
        metadata: None,
        expected_version: ExpectedVersion::Any,
    });

    // Write in batches
    if batch.len() >= 100 {
        event_store.write_events(batch.drain(..).collect()).await?;
    }
}

// Write remaining
if !batch.is_empty() {
    event_store.write_events(batch).await?;
}
```

### Stream Partitioning

Distribute load across streams:

```rust
// Instead of one hot stream
let stream_id = StreamId::from_static("orders");

// Partition by hash
let stream_id = StreamId::from_static(&format!(
    "orders-{}",
    order_id.hash() % 16  // 16 partitions
));
```

### Caching Strategies

Cache recent events for read performance:

```rust
struct CachedEventStore<ES: EventStore> {
    inner: ES,
    cache: Arc<RwLock<LruCache<StreamId, StreamEvents<ES::Event>>>>,
}

impl<ES: EventStore> CachedEventStore<ES> {
    async fn read_stream_cached(
        &self,
        stream_id: &StreamId,
        options: ReadOptions,
    ) -> Result<StreamEvents<ES::Event>, ES::Error> {
        // Check cache first
        if options.is_from_start() {
            if let Some(cached) = self.cache.read().await.get(stream_id) {
                return Ok(cached.clone());
            }
        }

        // Read from store
        let events = self.inner.read_stream(stream_id, options).await?;

        // Update cache
        self.cache.write().await.insert(stream_id.clone(), events.clone());

        Ok(events)
    }
}
```

## Testing with Events

### Event Fixtures

Create test events easily:

```rust
// Helper builders live in your test module; implement what you need for your domain.

fn create_account_opened_event() -> StoredEvent<BankEvent> {
    StoredEventBuilder::new()
        .with_stream_id(StreamId::from_static("account-123"))
        .with_version(EventVersion::new(1))
        .with_payload(BankEvent::AccountOpened {
            owner: "Alice".to_string(),
            initial_balance: 1000,
        })
        .with_metadata(
            EventMetadataBuilder::new()
                .with_user_id(UserId::from("alice@example.com"))
                .build()
        )
        .build()
}
```

> **Note:** `StoredEventBuilder` in this example represents a project-specific helper; feel free to adapt it to your needs until official testing fixtures are published.

### Event Assertions

Test event properties:

```rust
// Custom assertions shown here can live in any test helper module.

#[test]
fn test_events_are_ordered() {
    let events = vec![/* ... */];

    assert_events_ordered(&events);
    assert_unique_event_ids(&events);
    assert_stream_version_progression(&events, &stream_id);
}
```

## Summary

Events in EventCore are:

- ✅ **Immutable records** of business facts
- ✅ **Time-ordered** with UUIDv7 IDs
- ✅ **Version-controlled** for consistency
- ✅ **Atomically written** across streams
- ✅ **Rich with metadata** for auditing

Best practices:

1. Design events around business concepts
2. Include all necessary data in events
3. Plan for event evolution
4. Use version control for consistency
5. Optimize storage with partitioning

Next, let's explore [State Reconstruction](./03-state-reconstruction.md) →
