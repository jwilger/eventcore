# Chapter 5.5: Performance Optimization

EventCore is designed for performance, but complex event-sourced systems need careful optimization. This chapter covers patterns and techniques for maximizing performance in production.

## Performance Fundamentals

### Key Metrics

Monitor these critical metrics:

```rust
use prometheus::{Counter, Histogram, Gauge, register_counter, register_histogram, register_gauge};

lazy_static! {
    // Throughput metrics
    static ref COMMANDS_PER_SECOND: Counter = register_counter!(
        "eventcore_commands_per_second",
        "Commands executed per second"
    ).unwrap();

    static ref EVENTS_PER_SECOND: Counter = register_counter!(
        "eventcore_events_per_second",
        "Events written per second"
    ).unwrap();

    // Latency metrics
    static ref COMMAND_LATENCY: Histogram = register_histogram!(
        "eventcore_command_latency_seconds",
        "Command execution latency"
    ).unwrap();

    static ref EVENT_STORE_LATENCY: Histogram = register_histogram!(
        "eventcore_event_store_latency_seconds",
        "Event store operation latency"
    ).unwrap();

    // Resource usage
    static ref ACTIVE_STREAMS: Gauge = register_gauge!(
        "eventcore_active_streams",
        "Number of active event streams"
    ).unwrap();

    static ref MEMORY_USAGE: Gauge = register_gauge!(
        "eventcore_memory_usage_bytes",
        "Memory usage in bytes"
    ).unwrap();
}

#[derive(Debug, Clone)]
struct PerformanceMetrics {
    pub commands_per_second: f64,
    pub events_per_second: f64,
    pub avg_command_latency: Duration,
    pub p95_command_latency: Duration,
    pub p99_command_latency: Duration,
    pub memory_usage_mb: f64,
    pub active_streams: u64,
}

impl PerformanceMetrics {
    fn record_command_executed(&self, duration: Duration) {
        COMMANDS_PER_SECOND.inc();
        COMMAND_LATENCY.observe(duration.as_secs_f64());
    }

    fn record_events_written(&self, count: usize) {
        EVENTS_PER_SECOND.inc_by(count as f64);
    }
}
```

### Performance Targets

Typical performance targets for EventCore applications:

```rust
#[derive(Debug, Clone)]
struct PerformanceTargets {
    // Throughput targets
    pub min_commands_per_second: f64,      // 100+ commands/sec
    pub min_events_per_second: f64,        // 1000+ events/sec

    // Latency targets
    pub max_p50_latency: Duration,         // <10ms
    pub max_p95_latency: Duration,         // <50ms
    pub max_p99_latency: Duration,         // <100ms

    // Resource targets
    pub max_memory_usage_mb: f64,          // <1GB per service
    pub max_cpu_usage_percent: f64,        // <70%
}

impl PerformanceTargets {
    fn production() -> Self {
        Self {
            min_commands_per_second: 100.0,
            min_events_per_second: 1000.0,
            max_p50_latency: Duration::from_millis(10),
            max_p95_latency: Duration::from_millis(50),
            max_p99_latency: Duration::from_millis(100),
            max_memory_usage_mb: 1024.0,
            max_cpu_usage_percent: 70.0,
        }
    }

    fn development() -> Self {
        Self {
            min_commands_per_second: 10.0,
            min_events_per_second: 100.0,
            max_p50_latency: Duration::from_millis(50),
            max_p95_latency: Duration::from_millis(200),
            max_p99_latency: Duration::from_millis(500),
            max_memory_usage_mb: 512.0,
            max_cpu_usage_percent: 50.0,
        }
    }
}
```

## Event Store Optimization

### Connection Pooling

Optimize database connections for high throughput:

```rust
use sqlx::{Pool, Postgres, ConnectOptions};
use std::time::Duration;

#[derive(Debug, Clone)]
struct OptimizedPostgresConfig {
    pub database_url: String,
    pub max_connections: u32,
    pub min_connections: u32,
    pub acquire_timeout: Duration,
    pub idle_timeout: Duration,
    pub max_lifetime: Duration,
    pub connect_timeout: Duration,
    pub command_timeout: Duration,
}

impl OptimizedPostgresConfig {
    fn production() -> Self {
        Self {
            database_url: "postgresql://user:pass@host/db".to_string(),
            max_connections: 20,           // Higher for production
            min_connections: 5,            // Always keep minimum ready
            acquire_timeout: Duration::from_secs(30),
            idle_timeout: Duration::from_secs(600),     // 10 minutes
            max_lifetime: Duration::from_secs(1800),    // 30 minutes
            connect_timeout: Duration::from_secs(5),
            command_timeout: Duration::from_secs(30),
        }
    }

    async fn create_pool(&self) -> Result<Pool<Postgres>, sqlx::Error> {
        let options = sqlx::postgres::PgConnectOptions::from_url(&url::Url::parse(&self.database_url)?)?
            .application_name("eventcore-optimized");

        sqlx::postgres::PgPoolOptions::new()
            .max_connections(self.max_connections)
            .min_connections(self.min_connections)
            .acquire_timeout(self.acquire_timeout)
            .idle_timeout(self.idle_timeout)
            .max_lifetime(self.max_lifetime)
            .connect_with(options)
            .await
    }
}

struct OptimizedPostgresEventStore {
    pool: Pool<Postgres>,
    config: OptimizedPostgresConfig,
    batch_size: usize,
}

impl OptimizedPostgresEventStore {
    async fn new(config: OptimizedPostgresConfig) -> Result<Self, sqlx::Error> {
        let pool = config.create_pool().await?;

        Ok(Self {
            pool,
            config,
            batch_size: 1000, // Optimal batch size for PostgreSQL
        })
    }
}
```

### Batch Operations

Batch database operations for better throughput:

```rust
#[async_trait]
impl EventStore for OptimizedPostgresEventStore {
    type Event = serde_json::Value;
    type Error = EventStoreError;

    async fn write_events_batch(
        &self,
        events: Vec<EventToWrite<Self::Event>>,
    ) -> Result<WriteResult, Self::Error> {
        if events.is_empty() {
            return Ok(WriteResult { events_written: 0 });
        }

        // Batch events by stream for version checking
        let mut stream_batches: HashMap<StreamId, Vec<_>> = HashMap::new();
        for event in events {
            stream_batches.entry(event.stream_id.clone()).or_default().push(event);
        }

        let mut transaction = self.pool.begin().await?;
        let mut total_written = 0;

        for (stream_id, batch) in stream_batches {
            let written = self.write_stream_batch(&mut transaction, stream_id, batch).await?;
            total_written += written;
        }

        transaction.commit().await?;

        Ok(WriteResult { events_written: total_written })
    }

    async fn write_stream_batch(
        &self,
        transaction: &mut sqlx::Transaction<'_, Postgres>,
        stream_id: StreamId,
        events: Vec<EventToWrite<Self::Event>>,
    ) -> Result<usize, EventStoreError> {
        if events.is_empty() {
            return Ok(0);
        }

        // Check current version
        let current_version = self.get_stream_version(&mut *transaction, &stream_id).await?;

        // Validate expected versions
        let expected_version = events[0].expected_version;
        if expected_version != current_version {
            return Err(EventStoreError::VersionConflict {
                stream: stream_id,
                expected: expected_version,
                actual: current_version,
            });
        }

        // Prepare batch insert
        let mut values = Vec::new();
        let mut parameters = Vec::new();
        let mut param_index = 1;

        for (i, event) in events.iter().enumerate() {
            let version = current_version.0 + i as u64 + 1;
            let event_id = EventId::new_v7();

            values.push(format!(
                "(${}, ${}, ${}, ${}, ${}, ${}, ${})",
                param_index, param_index + 1, param_index + 2, param_index + 3,
                param_index + 4, param_index + 5, param_index + 6
            ));

            parameters.extend([
                event_id.as_ref(),
                stream_id.as_ref(),
                &version.to_string(),
                &event.event_type,
                &serde_json::to_string(&event.payload)?,
                &serde_json::to_string(&event.metadata)?,
                &Utc::now().to_rfc3339(),
            ]);

            param_index += 7;
        }

        let query = format!(
            "INSERT INTO events (id, stream_id, version, event_type, payload, metadata, occurred_at) VALUES {}",
            values.join(", ")
        );

        let mut query_builder = sqlx::query(&query);
        for param in parameters {
            query_builder = query_builder.bind(param);
        }

        let rows_affected = query_builder.execute(&mut **transaction).await?.rows_affected();

        Ok(rows_affected as usize)
    }
}
```

### Read Optimization

Optimize reading patterns:

```rust
impl OptimizedPostgresEventStore {
    // Optimized stream reading with pagination
    async fn read_stream_paginated(
        &self,
        stream_id: &StreamId,
        from_version: EventVersion,
        page_size: usize,
    ) -> Result<StreamEvents<Self::Event>, Self::Error> {
        let query = "
            SELECT id, stream_id, version, event_type, payload, metadata, occurred_at
            FROM events
            WHERE stream_id = $1 AND version >= $2
            ORDER BY version ASC
            LIMIT $3
        ";

        let rows = sqlx::query(query)
            .bind(stream_id.as_ref())
            .bind(from_version.as_ref())
            .bind(page_size as i64)
            .fetch_all(&self.pool)
            .await?;

        let events = rows.into_iter()
            .map(|row| self.row_to_event(row))
            .collect::<Result<Vec<_>, _>>()?;

        let version = events.last()
            .map(|e| e.version)
            .unwrap_or(from_version);

        Ok(StreamEvents {
            stream_id: stream_id.clone(),
            version,
            events,
        })
    }

    // Multi-stream reading with parallel queries
    async fn read_multiple_streams(
        &self,
        stream_ids: Vec<StreamId>,
        options: ReadOptions,
    ) -> Result<Vec<StreamEvents<Self::Event>>, Self::Error> {
        let futures = stream_ids.into_iter().map(|stream_id| {
            self.read_stream(&stream_id, options.clone())
        });

        let results = futures::future::try_join_all(futures).await?;
        Ok(results)
    }

    // Optimized subscription reading
    async fn read_all_events_from(
        &self,
        position: EventPosition,
        batch_size: usize,
    ) -> Result<Vec<StoredEvent<Self::Event>>, Self::Error> {
        let query = "
            SELECT id, stream_id, version, event_type, payload, metadata, occurred_at
            FROM events
            WHERE occurred_at > $1
            ORDER BY occurred_at ASC
            LIMIT $2
        ";

        let rows = sqlx::query(query)
            .bind(position.timestamp)
            .bind(batch_size as i64)
            .fetch_all(&self.pool)
            .await?;

        rows.into_iter()
            .map(|row| self.row_to_event(row))
            .collect()
    }
}
```

## Memory Optimization

### State Management

Optimize memory usage in command state:

```rust
use std::collections::LRU;

#[derive(Clone)]
struct OptimizedCommandExecutor {
    event_store: Arc<dyn EventStore<Event = serde_json::Value>>,
    state_cache: Arc<RwLock<LruCache<StreamId, Arc<dyn Any + Send + Sync>>>>,
    cache_size: usize,
}

impl OptimizedCommandExecutor {
    fn new(event_store: Arc<dyn EventStore<Event = serde_json::Value>>) -> Self {
        Self {
            event_store,
            state_cache: Arc::new(RwLock::new(LruCache::new(NonZeroUsize::new(1000).unwrap()))),
            cache_size: 1000,
        }
    }

    async fn execute_with_caching<C: Command>(
        &self,
        command: &C,
    ) -> CommandResult<ExecutionResult> {
        let stream_declarations = self.read_streams_for_command(command).await?;

        // Try to get cached state
        let cached_state = self.get_cached_state::<C>(&stream_declarations).await;

        let state = match cached_state {
            Some(state) => state,
            None => {
                // Reconstruct state and cache it
                let state = self.reconstruct_state::<C>(&stream_declarations).await?;
                self.cache_state(&stream_declarations, &state).await;
                state
            }
        };

        // Execute command
        let mut stream_resolver = StreamResolver::new();
        let events = command.handle(stream_declarations, state, &mut stream_resolver).await?;

        // Write events and invalidate cache
        let result = self.write_events(events).await?;
        self.invalidate_cache_for_streams(&result.affected_streams).await;

        Ok(result)
    }

    async fn get_cached_state<C: Command>(&self, stream_declarations: &StreamDeclarations<C::StreamSet>) -> Option<C::State> {
        let cache = self.state_cache.read().await;

        // Check if all streams are cached and up-to-date
        for stream_data in stream_declarations.iter() {
            if let Some(cached) = cache.get(&stream_data.stream_id) {
                // Verify cache is current
                if !self.is_cache_current(&stream_data, cached).await {
                    return None;
                }
            } else {
                return None;
            }
        }

        // All streams cached - reconstruct state from cache
        self.reconstruct_from_cache(stream_declarations).await
    }

    async fn cache_state<C: Command>(&self, stream_declarations: &StreamDeclarations<C::StreamSet>, state: &C::State) {
        let mut cache = self.state_cache.write().await;

        for stream_data in stream_declarations.iter() {
            let cached_data = CachedStreamData {
                stream_id: stream_data.stream_id.clone(),
                version: stream_data.version,
                events: stream_data.events.clone(),
                cached_at: Utc::now(),
            };

            cache.put(stream_data.stream_id.clone(), Arc::new(cached_data));
        }
    }
}

#[derive(Debug, Clone)]
struct CachedStreamData {
    stream_id: StreamId,
    version: EventVersion,
    events: Vec<StoredEvent<serde_json::Value>>,
    cached_at: DateTime<Utc>,
}
```

### Event Streaming

Stream events instead of loading everything into memory:

```rust
use tokio_stream::{Stream, StreamExt};
use futures::stream::TryStreamExt;

trait StreamingEventStore {
    fn stream_events(
        &self,
        stream_id: &StreamId,
        from_version: EventVersion,
    ) -> impl Stream<Item = Result<StoredEvent<serde_json::Value>, EventStoreError>>;

    fn stream_all_events(
        &self,
        from_position: EventPosition,
    ) -> impl Stream<Item = Result<StoredEvent<serde_json::Value>, EventStoreError>>;
}

impl StreamingEventStore for OptimizedPostgresEventStore {
    fn stream_events(
        &self,
        stream_id: &StreamId,
        from_version: EventVersion,
    ) -> impl Stream<Item = Result<StoredEvent<serde_json::Value>, EventStoreError>> {
        let pool = self.pool.clone();
        let stream_id = stream_id.clone();
        let page_size = 100;

        async_stream::try_stream! {
            let mut current_version = from_version;

            loop {
                let query = "
                    SELECT id, stream_id, version, event_type, payload, metadata, occurred_at
                    FROM events
                    WHERE stream_id = $1 AND version >= $2
                    ORDER BY version ASC
                    LIMIT $3
                ";

                let rows = sqlx::query(query)
                    .bind(stream_id.as_ref())
                    .bind(current_version.as_ref())
                    .bind(page_size as i64)
                    .fetch_all(&pool)
                    .await?;

                if rows.is_empty() {
                    break;
                }

                for row in rows {
                    let event = self.row_to_event(row)?;
                    current_version = EventVersion::from(event.version.as_u64() + 1);
                    yield event;
                }

                if rows.len() < page_size {
                    break;
                }
            }
        }
    }
}
```

// Usage in projections #[async_trait]
impl Projection for StreamingProjection {
type Event = serde_json::Value;
type Error = ProjectionError;

    async fn rebuild_from_stream(
        &mut self,
        event_stream: impl Stream<Item = Result<StoredEvent<Self::Event>, EventStoreError>>,
    ) -> Result<(), Self::Error> {
        let mut stream = std::pin::pin!(event_stream);

        while let Some(event_result) = stream.next().await {
            let event = event_result?;
            self.apply(&event).await?;

            // Checkpoint every 1000 events
            if event.version.as_u64() % 1000 == 0 {
                self.save_checkpoint(event.version).await?;
            }
        }

        Ok(())
    }

}

````

## Concurrency Optimization

### Parallel Command Execution

Execute independent commands in parallel:

```rust
use tokio::sync::Semaphore;
use std::sync::Arc;

#[derive(Clone)]
struct ParallelCommandExecutor {
    inner: CommandExecutor,
    concurrency_limit: Arc<Semaphore>,
    stream_locks: Arc<RwLock<HashMap<StreamId, Arc<Mutex<()>>>>>,
}

impl ParallelCommandExecutor {
    fn new(inner: CommandExecutor, max_concurrency: usize) -> Self {
        Self {
            inner,
            concurrency_limit: Arc::new(Semaphore::new(max_concurrency)),
            stream_locks: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    async fn execute_batch<C: Command>(
        &self,
        commands: Vec<C>,
    ) -> Vec<CommandResult<ExecutionResult>> {
        // Group commands by affected streams
        let stream_groups = self.group_by_streams(&commands).await;

        let futures = stream_groups.into_iter().map(|(streams, commands)| {
            self.execute_stream_group(streams, commands)
        });

        let results = futures::future::join_all(futures).await;

        // Flatten results
        results.into_iter().flatten().collect()
    }

    async fn execute_stream_group<C: Command>(
        &self,
        affected_streams: HashSet<StreamId>,
        commands: Vec<C>,
    ) -> Vec<CommandResult<ExecutionResult>> {
        // Acquire locks for all streams in this group
        let _locks = self.acquire_stream_locks(&affected_streams).await;

        // Execute commands sequentially within the group
        let mut results = Vec::new();

        for command in commands {
            let _permit = self.concurrency_limit.acquire().await.unwrap();
            let result = self.inner.execute(&command).await;
            results.push(result);
        }

        results
    }

    async fn group_by_streams<C: Command>(
        &self,
        commands: &[C],
    ) -> HashMap<HashSet<StreamId>, Vec<C>> {
        let mut groups = HashMap::new();

        for command in commands {
            let streams = command.stream_declarations().into_iter().collect();
            groups.entry(streams).or_insert_with(Vec::new).push(command.clone());
        }

        groups
    }

    async fn acquire_stream_locks(
        &self,
        stream_ids: &HashSet<StreamId>,
    ) -> Vec<tokio::sync::MutexGuard<'_, ()>> {
        let mut locks = Vec::new();

        // Sort stream IDs to prevent deadlocks
        let mut sorted_streams: Vec<_> = stream_ids.iter().collect();
        sorted_streams.sort();

        for stream_id in sorted_streams {
            let lock = {
                let stream_locks = self.stream_locks.read().await;
                stream_locks.get(stream_id).cloned()
            };

            let lock = match lock {
                Some(lock) => lock,
                None => {
                    let mut stream_locks = self.stream_locks.write().await;
                    stream_locks.entry(stream_id.clone())
                        .or_insert_with(|| Arc::new(Mutex::new(())))
                        .clone()
                }
            };

            locks.push(lock.lock().await);
        }

        locks
    }
}
````

### Async Batching

Batch operations automatically:

```rust
use tokio::sync::mpsc;
use tokio::time::{interval, Duration};

struct BatchProcessor<T, R> {
    sender: mpsc::UnboundedSender<BatchItem<T, R>>,
    batch_size: usize,
    batch_timeout: Duration,
}

struct BatchItem<T, R> {
    item: T,
    response_sender: oneshot::Sender<R>,
}

impl<T, R> BatchProcessor<T, R>
where
    T: Send + 'static,
    R: Send + 'static,
{
    fn new<F, Fut>(
        batch_size: usize,
        batch_timeout: Duration,
        processor: F,
    ) -> Self
    where
        F: Fn(Vec<T>) -> Fut + Send + 'static,
        Fut: Future<Output = Vec<R>> + Send,
    {
        let (sender, receiver) = mpsc::unbounded_channel();

        tokio::spawn(Self::batch_worker(receiver, batch_size, batch_timeout, processor));

        Self {
            sender,
            batch_size,
            batch_timeout,
        }
    }

    async fn process(&self, item: T) -> Result<R, BatchError> {
        let (response_sender, response_receiver) = oneshot::channel();

        self.sender.send(BatchItem {
            item,
            response_sender,
        })?;

        response_receiver.await.map_err(BatchError::Cancelled)
    }

    async fn batch_worker<F, Fut>(
        mut receiver: mpsc::UnboundedReceiver<BatchItem<T, R>>,
        batch_size: usize,
        batch_timeout: Duration,
        processor: F,
    )
    where
        F: Fn(Vec<T>) -> Fut + Send + 'static,
        Fut: Future<Output = Vec<R>> + Send,
    {
        let mut batch = Vec::new();
        let mut senders = Vec::new();
        let mut timer = interval(batch_timeout);

        loop {
            select! {
                item = receiver.recv() => {
                    match item {
                        Some(BatchItem { item, response_sender }) => {
                            batch.push(item);
                            senders.push(response_sender);

                            if batch.len() >= batch_size {
                                Self::process_batch(&processor, &mut batch, &mut senders).await;
                            }
                        }
                        None => break, // Channel closed
                    }
                }
                _ = timer.tick() => {
                    if !batch.is_empty() {
                        Self::process_batch(&processor, &mut batch, &mut senders).await;
                    }
                }
            }
        }
    }

    async fn process_batch<F, Fut>(
        processor: &F,
        batch: &mut Vec<T>,
        senders: &mut Vec<oneshot::Sender<R>>,
    )
    where
        F: Fn(Vec<T>) -> Fut,
        Fut: Future<Output = Vec<R>>,
    {
        if batch.is_empty() {
            return;
        }

        let items = std::mem::take(batch);
        let response_senders = std::mem::take(senders);

        let results = processor(items).await;

        for (sender, result) in response_senders.into_iter().zip(results) {
            let _ = sender.send(result); // Ignore send errors
        }
    }
}

// Usage for batched event writing
type EventBatch = BatchProcessor<EventToWrite<serde_json::Value>, Result<(), EventStoreError>>;

impl OptimizedPostgresEventStore {
    fn new_with_batching(pool: Pool<Postgres>) -> (Self, EventBatch) {
        let store = Self::new(pool);
        let store_clone = store.clone();

        let batch_processor = BatchProcessor::new(
            100,                           // Batch size
            Duration::from_millis(10),     // Batch timeout
            move |events| {
                let store = store_clone.clone();
                async move {
                    match store.write_events_batch(events).await {
                        Ok(_) => vec![Ok(()); events.len()],
                        Err(e) => vec![Err(e); events.len()],
                    }
                }
            }
        );

        (store, batch_processor)
    }
}
```

## Projection Optimization

### Incremental Updates

Update projections incrementally:

```rust
#[async_trait]
trait IncrementalProjection {
    type Event;
    type State;
    type Error;

    async fn apply_incremental(
        &mut self,
        event: &StoredEvent<Self::Event>,
        previous_state: Option<&Self::State>,
    ) -> Result<Self::State, Self::Error>;

    async fn get_checkpoint(&self) -> Result<EventVersion, Self::Error>;
    async fn save_checkpoint(&self, version: EventVersion) -> Result<(), Self::Error>;
}

struct OptimizedUserProjection {
    users: HashMap<UserId, UserSummary>,
    last_processed_version: EventVersion,
    checkpoint_interval: u64,
}

#[async_trait]
impl IncrementalProjection for OptimizedUserProjection {
    type Event = UserEvent;
    type State = HashMap<UserId, UserSummary>;
    type Error = ProjectionError;

    async fn apply_incremental(
        &mut self,
        event: &StoredEvent<Self::Event>,
        previous_state: Option<&Self::State>,
    ) -> Result<Self::State, Self::Error> {
        // Clone state if provided, otherwise start fresh
        let mut state = previous_state.cloned().unwrap_or_default();

        // Apply only this event
        match &event.payload {
            UserEvent::Registered { user_id, email, profile } => {
                state.insert(*user_id, UserSummary {
                    id: *user_id,
                    email: email.clone(),
                    display_name: profile.display_name(),
                    created_at: event.occurred_at,
                    updated_at: event.occurred_at,
                });
            }
            UserEvent::ProfileUpdated { user_id, profile } => {
                if let Some(user) = state.get_mut(user_id) {
                    user.display_name = profile.display_name();
                    user.updated_at = event.occurred_at;
                }
            }
        }

        // Update checkpoint
        self.last_processed_version = event.version;

        // Save checkpoint periodically
        if event.version.as_u64() % self.checkpoint_interval == 0 {
            self.save_checkpoint(event.version).await?;
        }

        Ok(state)
    }

    async fn get_checkpoint(&self) -> Result<EventVersion, Self::Error> {
        Ok(self.last_processed_version)
    }

    async fn save_checkpoint(&self, version: EventVersion) -> Result<(), Self::Error> {
        // Save to persistent storage
        // Implementation depends on your checkpoint store
        Ok(())
    }
}
```

### Materialized Views

Use database materialized views for query optimization:

```sql
-- Create materialized view for user summaries
CREATE MATERIALIZED VIEW user_summaries AS
SELECT
    (payload->>'user_id')::uuid as user_id,
    payload->>'email' as email,
    payload->'profile'->>'display_name' as display_name,
    occurred_at as created_at,
    occurred_at as updated_at
FROM events
WHERE event_type = 'UserRegistered'
UNION ALL
SELECT
    (payload->>'user_id')::uuid as user_id,
    NULL as email,
    payload->'profile'->>'display_name' as display_name,
    NULL as created_at,
    occurred_at as updated_at
FROM events
WHERE event_type = 'UserProfileUpdated';

-- Create indexes for fast queries
CREATE INDEX idx_user_summaries_user_id ON user_summaries(user_id);
CREATE INDEX idx_user_summaries_email ON user_summaries(email);

-- Refresh materialized view (can be automated)
REFRESH MATERIALIZED VIEW user_summaries;
```

## Benchmarking and Profiling

### Performance Testing

Create comprehensive benchmarks:

```rust
use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};
use tokio::runtime::Runtime;

fn benchmark_command_execution(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let store = rt.block_on(async {
        InMemoryEventStore::new()
    });
    let executor = CommandExecutor::new(store);

    let mut group = c.benchmark_group("command_execution");

    for concurrency in [1, 10, 50, 100].iter() {
        group.bench_with_input(
            BenchmarkId::new("create_user", concurrency),
            concurrency,
            |b, &concurrency| {
                b.to_async(&rt).iter(|| async {
                    let commands: Vec<_> = (0..concurrency)
                        .map(|i| CreateUser {
                            email: Email::try_new(format!("user{}@example.com", i)).unwrap(),
                            first_name: FirstName::try_new(format!("User{}", i)).unwrap(),
                            last_name: LastName::try_new("Test".to_string()).unwrap(),
                        })
                        .collect();

                    let futures = commands.into_iter().map(|cmd| executor.execute(&cmd));
                    let results = futures::future::join_all(futures).await;

                    black_box(results);
                });
            }
        );
    }

    group.finish();
}

fn benchmark_event_store_operations(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let store = rt.block_on(async {
        PostgresEventStore::new("postgresql://localhost/eventcore_bench").await.unwrap()
    });

    let mut group = c.benchmark_group("event_store");

    for batch_size in [1, 10, 100, 1000].iter() {
        group.bench_with_input(
            BenchmarkId::new("write_events", batch_size),
            batch_size,
            |b, &batch_size| {
                b.to_async(&rt).iter(|| async {
                    let events: Vec<_> = (0..batch_size)
                        .map(|i| EventToWrite {
                            stream_id: StreamId::try_new(format!("test-{}", i)).unwrap(),
                            payload: json!({ "test": i }),
                            metadata: EventMetadata::default(),
                            expected_version: EventVersion::from(0),
                        })
                        .collect();

                    let result = store.write_events(events).await;
                    black_box(result);
                });
            }
        );
    }

    group.finish();
}

criterion_group!(benches, benchmark_command_execution, benchmark_event_store_operations);
criterion_main!(benches);
```

### Memory Profiling

Profile memory usage patterns:

```rust
use memory_profiler::{Allocator, ProfiledAllocator};

#[global_allocator]
static PROFILED_ALLOCATOR: ProfiledAllocator<std::alloc::System> = ProfiledAllocator::new(std::alloc::System);

#[derive(Debug)]
struct MemoryUsageReport {
    pub allocated_bytes: usize,
    pub deallocated_bytes: usize,
    pub peak_memory: usize,
    pub current_memory: usize,
}

impl MemoryUsageReport {
    fn capture() -> Self {
        let stats = PROFILED_ALLOCATOR.stats();
        Self {
            allocated_bytes: stats.allocated,
            deallocated_bytes: stats.deallocated,
            peak_memory: stats.peak,
            current_memory: stats.current,
        }
    }
}

#[cfg(test)]
mod memory_tests {
    use super::*;

    #[tokio::test]
    async fn test_memory_usage_during_batch_execution() {
        let initial_memory = MemoryUsageReport::capture();

        // Execute large batch of commands
        let store = InMemoryEventStore::new();
        let executor = CommandExecutor::new(store);

        let commands: Vec<_> = (0..10000)
            .map(|i| CreateUser {
                email: Email::try_new(format!("user{}@example.com", i)).unwrap(),
                first_name: FirstName::try_new(format!("User{}", i)).unwrap(),
                last_name: LastName::try_new("Test".to_string()).unwrap(),
            })
            .collect();

        let peak_memory = MemoryUsageReport::capture();

        for command in commands {
            executor.execute(&command).await.unwrap();
        }

        let final_memory = MemoryUsageReport::capture();

        println!("Initial memory: {:?}", initial_memory);
        println!("Peak memory: {:?}", peak_memory);
        println!("Final memory: {:?}", final_memory);

        // Assert memory doesn't grow unbounded
        let memory_growth = final_memory.current_memory.saturating_sub(initial_memory.current_memory);
        assert!(memory_growth < 100 * 1024 * 1024, "Memory growth too large: {} bytes", memory_growth);
    }
}
```

## Production Monitoring

### Performance Dashboards

Create monitoring dashboards:

```rust
use prometheus::{Opts, Registry, TextEncoder, Encoder};
use axum::{response::Html, routing::get, Router};

#[derive(Clone)]
struct PerformanceMonitor {
    registry: Registry,
    metrics: PerformanceMetrics,
}

impl PerformanceMonitor {
    fn new() -> Self {
        let registry = Registry::new();
        let metrics = PerformanceMetrics::new(&registry);

        Self { registry, metrics }
    }

    async fn metrics_handler(&self) -> String {
        let encoder = TextEncoder::new();
        let metric_families = self.registry.gather();
        encoder.encode_to_string(&metric_families).unwrap()
    }

    fn dashboard_routes(&self) -> Router {
        let monitor = self.clone();

        Router::new()
            .route("/metrics", get(move || monitor.metrics_handler()))
            .route("/health", get(|| async { "OK" }))
            .route("/dashboard", get(|| async {
                Html(include_str!("performance_dashboard.html"))
            }))
    }
}
```

### Alerting

Set up performance alerts:

```rust
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Clone)]
struct PerformanceAlerting {
    thresholds: PerformanceTargets,
    alert_cooldown: Duration,
    last_alert: Arc<Mutex<HashMap<String, DateTime<Utc>>>>,
    alert_enabled: Arc<AtomicBool>,
}

impl PerformanceAlerting {
    fn new(thresholds: PerformanceTargets) -> Self {
        Self {
            thresholds,
            alert_cooldown: Duration::from_minutes(5),
            last_alert: Arc::new(Mutex::new(HashMap::new())),
            alert_enabled: Arc::new(AtomicBool::new(true)),
        }
    }

    async fn check_metrics(&self, metrics: &PerformanceMetrics) {
        if !self.alert_enabled.load(Ordering::Relaxed) {
            return;
        }

        // Check command latency
        if metrics.p95_command_latency > self.thresholds.max_p95_latency {
            self.send_alert(
                "high_latency",
                &format!(
                    "P95 latency is {}ms, threshold is {}ms",
                    metrics.p95_command_latency.as_millis(),
                    self.thresholds.max_p95_latency.as_millis()
                )
            ).await;
        }

        // Check throughput
        if metrics.commands_per_second < self.thresholds.min_commands_per_second {
            self.send_alert(
                "low_throughput",
                &format!(
                    "Throughput is {:.1} commands/sec, threshold is {:.1}",
                    metrics.commands_per_second,
                    self.thresholds.min_commands_per_second
                )
            ).await;
        }

        // Check memory usage
        if metrics.memory_usage_mb > self.thresholds.max_memory_usage_mb {
            self.send_alert(
                "high_memory",
                &format!(
                    "Memory usage is {:.1}MB, threshold is {:.1}MB",
                    metrics.memory_usage_mb,
                    self.thresholds.max_memory_usage_mb
                )
            ).await;
        }
    }

    async fn send_alert(&self, alert_type: &str, message: &str) {
        let mut last_alerts = self.last_alert.lock().await;
        let now = Utc::now();

        // Check cooldown
        if let Some(last_time) = last_alerts.get(alert_type) {
            if now.signed_duration_since(*last_time) < self.alert_cooldown {
                return; // Still in cooldown
            }
        }

        // Send alert (implement your alerting system)
        self.dispatch_alert(alert_type, message).await;

        // Update last alert time
        last_alerts.insert(alert_type.to_string(), now);
    }

    async fn dispatch_alert(&self, alert_type: &str, message: &str) {
        // Implementation depends on your alerting system
        // Examples: Slack, PagerDuty, email, etc.
        tracing::error!("PERFORMANCE ALERT [{}]: {}", alert_type, message);

        // Example: Send to Slack
        if let Ok(webhook_url) = std::env::var("SLACK_WEBHOOK_URL") {
            let payload = json!({
                "text": format!("🚨 EventCore Performance Alert: {}", message),
                "channel": "#alerts",
                "username": "EventCore Monitor"
            });

            let client = reqwest::Client::new();
            let _ = client.post(&webhook_url)
                .json(&payload)
                .send()
                .await;
        }
    }
}
```

## Best Practices

1. **Measure first** - Always profile before optimizing
2. **Optimize bottlenecks** - Focus on the slowest operations
3. **Batch operations** - Reduce round trips to storage
4. **Cache wisely** - Cache expensive computations, not everything
5. **Stream large datasets** - Don't load everything into memory
6. **Monitor continuously** - Track performance in production
7. **Set alerts** - Get notified when performance degrades
8. **Test under load** - Use realistic workloads in testing

## Summary

Performance optimization in EventCore:

- ✅ **Comprehensive monitoring** - Track all key metrics
- ✅ **Database optimization** - Connection pooling and batching
- ✅ **Memory efficiency** - Streaming and caching strategies
- ✅ **Concurrency optimization** - Parallel execution patterns
- ✅ **Production monitoring** - Dashboards and alerting

Key strategies:

1. Optimize the event store with connection pooling and batching
2. Use streaming for large datasets to minimize memory usage
3. Implement parallel execution for independent commands
4. Monitor performance continuously with metrics and alerts
5. Profile and benchmark to identify bottlenecks

Performance is a journey, not a destination. Measure, optimize, and monitor continuously to ensure your EventCore applications scale effectively in production.

Next, let's explore the [Operations Guide](../06-operations/README.md) →
