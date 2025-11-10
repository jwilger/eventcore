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

... (sections unchanged) ...

impl OptimizedCommandExecutor {
async fn execute_with_caching<C: Command>(&self, command: &C) -> CommandResult<ExecutionResult> {
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

        // Execute command using pure domain logic - command.handle takes reconstructed state and returns NewEvents
        let events = command.handle(state)?;

        // Executor is responsible for translating NewEvents into storage writes and invalidating caches
        let result = self.write_events(events).await?;
        self.invalidate_cache_for_streams(&result.affected_streams).await;

        Ok(result)
    }

    async fn get_cached_state<C: Command>(&self, stream_declarations: &StreamDeclarations) -> Option<C::State> {
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

    async fn cache_state<C: Command>(&self, stream_declarations: &StreamDeclarations, state: &C::State) {
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

... (rest unchanged) ...
