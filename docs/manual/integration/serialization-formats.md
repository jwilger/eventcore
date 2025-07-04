# Serialization Formats in EventCore

EventCore supports multiple serialization formats for event storage, allowing you to choose the best format for your specific requirements.

## Supported Formats

### JSON (Default)
- **Pros**: Human-readable, excellent debugging, wide tool support
- **Cons**: Larger size, slower performance
- **Use when**: Debugging, development, or when human readability is important

### MessagePack
- **Pros**: 30-50% smaller than JSON, faster than JSON, self-describing
- **Cons**: Binary format (not human-readable), less tool support
- **Use when**: You need a balance of size and performance with schema flexibility

### Bincode
- **Pros**: Fastest serialization/deserialization, very compact
- **Cons**: Binary format, Rust-specific, less flexible with schema changes
- **Use when**: Maximum performance is critical and you control all clients

## Configuration

### With PostgreSQL

```rust
use eventcore_postgres::PostgresConfig;
use eventcore::serialization::SerializationFormat;

let mut config = PostgresConfig::new("postgres://localhost/eventcore");
config.serialization_format = SerializationFormat::MessagePack;

let event_store = PostgresEventStore::new(config).await?;
```

### With In-Memory Store

The in-memory store always uses the default Rust serialization internally, but you can use different formats when exporting/importing data.

## Performance Comparison

Based on benchmarks with typical event payloads:

| Format | Size (relative to JSON) | Serialize Speed | Deserialize Speed |
|--------|------------------------|-----------------|-------------------|
| JSON | 100% (baseline) | 1x (baseline) | 1x (baseline) |
| MessagePack | 60-70% | 1.5-2x faster | 1.5-2x faster |
| Bincode | 40-50% | 3-5x faster | 3-5x faster |

## Migration Between Formats

To migrate existing events from one format to another:

1. Create a migration tool that reads events in the old format
2. Re-serialize them in the new format
3. Use EventCore's schema evolution features if needed

```rust
// Example migration approach
let old_serializer = JsonEventSerializer::new();
let new_serializer = MessagePackEventSerializer::new();

// Read with old format
let event_data = old_serializer.deserialize_stored_event(&data, "EventType").await?;

// Write with new format
let new_data = new_serializer.serialize_stored_event(&event_data, "EventType").await?;
```

## Format Selection Guidelines

Choose your serialization format based on these factors:

1. **Development Stage**
   - Development/Testing: JSON for easy debugging
   - Production: MessagePack or Bincode for efficiency

2. **Performance Requirements**
   - Low latency critical: Bincode
   - Balanced performance: MessagePack
   - Performance not critical: JSON

3. **Storage Costs**
   - Storage expensive: Bincode or MessagePack
   - Storage cheap: JSON acceptable

4. **Interoperability**
   - Multiple languages: JSON or MessagePack
   - Rust-only: Any format, Bincode fastest

5. **Debugging Needs**
   - Frequent debugging: JSON
   - Rare debugging: Binary formats acceptable

## Example Usage

See `examples/serialization_formats_example.rs` for a complete example demonstrating:
- Format comparison
- Performance benchmarking
- Configuration with different stores