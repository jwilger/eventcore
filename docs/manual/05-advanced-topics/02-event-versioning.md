# Chapter 5.2: Event Versioning

Event versioning is a systematic approach to managing changes in event schemas while preserving the ability to read historical data. This chapter covers EventCore's versioning strategies and implementation patterns.

## Versioning Strategies

### Semantic Versioning for Events

Apply semantic versioning principles to events:

```rust
use eventcore::serialization::EventVersion;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct EventSchemaVersion {
    major: u32,
    minor: u32,
    patch: u32,
}

impl EventSchemaVersion {
    const fn new(major: u32, minor: u32, patch: u32) -> Self {
        Self { major, minor, patch }
    }
    
    // Breaking changes
    const V1_0_0: Self = Self::new(1, 0, 0);
    const V2_0_0: Self = Self::new(2, 0, 0);
    
    // Backward compatible additions
    const V1_1_0: Self = Self::new(1, 1, 0);
    const V1_2_0: Self = Self::new(1, 2, 0);
    
    // Bug fixes/clarifications
    const V1_0_1: Self = Self::new(1, 0, 1);
}

trait VersionedEvent {
    const EVENT_TYPE: &'static str;
    const VERSION: EventSchemaVersion;
    
    fn is_compatible_with(version: &EventSchemaVersion) -> bool;
}
```

### Linear Versioning

Simpler approach with incremental versions:

```rust
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "version")]
enum UserEvent {
    #[serde(rename = "1")]
    V1(UserEventV1),
    
    #[serde(rename = "2")]
    V2(UserEventV2),
    
    #[serde(rename = "3")]
    V3(UserEventV3),
}

#[derive(Debug, Serialize, Deserialize)]
struct UserEventV1 {
    pub user_id: String,
    pub email: String,
    pub username: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct UserEventV2 {
    pub user_id: UserId,
    pub email: Email,
    pub first_name: String,
    pub last_name: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct UserEventV3 {
    pub user_id: UserId,
    pub email: Email,
    pub profile: UserProfile,
    pub preferences: UserPreferences,
}
```

## Version-Aware Serialization

EventCore provides automatic version handling:

```rust
use eventcore::serialization::{VersionedSerializer, SerializationFormat};

#[derive(Clone)]
struct EventSerializer {
    format: SerializationFormat,
    registry: TypeRegistry,
}

impl EventSerializer {
    fn new() -> Self {
        let mut registry = TypeRegistry::new();
        
        // Register all versions
        registry.register_versioned::<UserEventV1>("UserEvent", 1);
        registry.register_versioned::<UserEventV2>("UserEvent", 2);
        registry.register_versioned::<UserEventV3>("UserEvent", 3);
        
        Self {
            format: SerializationFormat::Json,
            registry,
        }
    }
    
    fn serialize_event<T>(&self, event: &T) -> Result<VersionedPayload, SerializationError>
    where
        T: Serialize + VersionedEvent,
    {
        let data = self.format.serialize(event)?;
        
        Ok(VersionedPayload {
            event_type: T::EVENT_TYPE.to_string(),
            version: T::VERSION.to_string(),
            format: self.format,
            data,
        })
    }
    
    fn deserialize_event<T>(&self, payload: &VersionedPayload) -> Result<T, SerializationError>
    where
        T: DeserializeOwned + VersionedEvent,
    {
        // Check version compatibility
        let payload_version = EventSchemaVersion::parse(&payload.version)?;
        if !T::is_compatible_with(&payload_version) {
            return Err(SerializationError::IncompatibleVersion {
                expected: T::VERSION,
                found: payload_version,
            });
        }
        
        self.format.deserialize(&payload.data)
    }
}

#[derive(Debug, Clone)]
struct VersionedPayload {
    event_type: String,
    version: String,
    format: SerializationFormat,
    data: Vec<u8>,
}
```

## Migration Chains

Handle complex version transitions:

```rust
use eventcore::serialization::{MigrationChain, Migration};

struct UserEventMigrationChain {
    migrations: Vec<Box<dyn Migration<UserEvent, UserEvent>>>,
}

impl UserEventMigrationChain {
    fn new() -> Self {
        let migrations: Vec<Box<dyn Migration<UserEvent, UserEvent>>> = vec![
            Box::new(V1ToV2Migration),
            Box::new(V2ToV3Migration),
        ];
        
        Self { migrations }
    }
    
    fn migrate_to_latest(&self, event: UserEvent, from_version: u32) -> Result<UserEvent, MigrationError> {
        let mut current_event = event;
        let mut current_version = from_version;
        
        // Apply migrations in sequence
        while current_version < UserEvent::LATEST_VERSION {
            let migration = self.migrations
                .get((current_version - 1) as usize)
                .ok_or(MigrationError::NoMigrationPath { 
                    from: current_version, 
                    to: UserEvent::LATEST_VERSION 
                })?;
            
            current_event = migration.migrate(current_event)?;
            current_version += 1;
        }
        
        Ok(current_event)
    }
}

struct V1ToV2Migration;

impl Migration<UserEvent, UserEvent> for V1ToV2Migration {
    fn migrate(&self, event: UserEvent) -> Result<UserEvent, MigrationError> {
        match event {
            UserEvent::V1(v1) => {
                // Convert V1 to V2
                let user_id = UserId::try_from(v1.user_id)
                    .map_err(|e| MigrationError::ConversionFailed(e.to_string()))?;
                
                let email = Email::try_from(v1.email)
                    .map_err(|e| MigrationError::ConversionFailed(e.to_string()))?;
                
                // Extract names from username
                let (first_name, last_name) = split_username(&v1.username);
                
                Ok(UserEvent::V2(UserEventV2 {
                    user_id,
                    email,
                    first_name,
                    last_name,
                }))
            }
            other => Ok(other), // Already V2 or later
        }
    }
}

fn split_username(username: &str) -> (String, String) {
    let parts: Vec<&str> = username.split('_').collect();
    match parts.len() {
        1 => (parts[0].to_string(), String::new()),
        2 => (parts[0].to_string(), parts[1].to_string()),
        _ => (parts[0].to_string(), parts[1..].join("_")),
    }
}
```

## Event Store Integration

Integrate versioning with the event store:

```rust
#[async_trait]
impl EventStore for VersionedEventStore {
    type Event = VersionedEvent;
    type Error = EventStoreError;
    
    async fn write_events(
        &self,
        events: Vec<EventToWrite<Self::Event>>,
    ) -> Result<WriteResult, Self::Error> {
        let versioned_events: Result<Vec<_>, _> = events
            .into_iter()
            .map(|event| {
                let payload = self.serializer.serialize_event(&event.payload)?;
                Ok(EventToWrite {
                    stream_id: event.stream_id,
                    payload,
                    metadata: event.metadata,
                    expected_version: event.expected_version,
                })
            })
            .collect();
        
        self.inner.write_events(versioned_events?).await
    }
    
    async fn read_stream(
        &self,
        stream_id: &StreamId,
        options: ReadOptions,
    ) -> Result<StreamEvents<Self::Event>, Self::Error> {
        let raw_events = self.inner.read_stream(stream_id, options).await?;
        
        let events: Result<Vec<_>, _> = raw_events
            .events
            .into_iter()
            .map(|event| {
                let payload = self.serializer.deserialize_event(&event.payload)?;
                Ok(StoredEvent {
                    id: event.id,
                    stream_id: event.stream_id,
                    version: event.version,
                    payload,
                    metadata: event.metadata,
                    occurred_at: event.occurred_at,
                })
            })
            .collect();
        
        Ok(StreamEvents {
            stream_id: raw_events.stream_id,
            version: raw_events.version,
            events: events?,
        })
    }
}
```

## Version-Aware Projections

Projections that handle multiple event versions:

```rust
#[async_trait]
impl Projection for UserProjection {
    type Event = VersionedEvent;
    type Error = ProjectionError;
    
    async fn apply(&mut self, event: &StoredEvent<Self::Event>) -> Result<(), Self::Error> {
        match &event.payload {
            VersionedEvent::User(user_event) => {
                self.apply_user_event(user_event, event.occurred_at).await?;
            }
            _ => {} // Ignore other event types
        }
        Ok(())
    }
}

impl UserProjection {
    async fn apply_user_event(
        &mut self, 
        event: &UserEvent, 
        occurred_at: DateTime<Utc>
    ) -> Result<(), ProjectionError> {
        match event {
            UserEvent::V1(v1) => {
                // Handle V1 events
                let user = User {
                    id: UserId::try_from(v1.user_id.clone())?,
                    email: v1.email.clone(),
                    display_name: v1.username.clone(),
                    first_name: None,
                    last_name: None,
                    profile: None,
                    preferences: UserPreferences::default(),
                    created_at: occurred_at,
                    updated_at: occurred_at,
                };
                self.users.insert(user.id.clone(), user);
            }
            UserEvent::V2(v2) => {
                // Handle V2 events
                let user = User {
                    id: v2.user_id.clone(),
                    email: v2.email.to_string(),
                    display_name: format!("{} {}", v2.first_name, v2.last_name),
                    first_name: Some(v2.first_name.clone()),
                    last_name: Some(v2.last_name.clone()),
                    profile: None,
                    preferences: UserPreferences::default(),
                    created_at: occurred_at,
                    updated_at: occurred_at,
                };
                self.users.insert(user.id.clone(), user);
            }
            UserEvent::V3(v3) => {
                // Handle V3 events
                let user = User {
                    id: v3.user_id.clone(),
                    email: v3.email.to_string(),
                    display_name: v3.profile.display_name(),
                    first_name: Some(v3.profile.first_name.clone()),
                    last_name: Some(v3.profile.last_name.clone()),
                    profile: Some(v3.profile.clone()),
                    preferences: v3.preferences.clone(),
                    created_at: occurred_at,
                    updated_at: occurred_at,
                };
                self.users.insert(user.id.clone(), user);
            }
        }
        Ok(())
    }
}
```

## Version Compatibility Rules

Define clear compatibility rules:

```rust
#[derive(Debug, Clone, PartialEq)]
enum CompatibilityLevel {
    FullyCompatible,    // Can read/write without issues
    ReadOnly,           // Can read but not write
    RequiresMigration,  // Need migration to use
    Incompatible,       // Cannot use
}

trait VersionCompatibility {
    fn check_compatibility(reader_version: &str, event_version: &str) -> CompatibilityLevel;
}

struct UserEventCompatibility;

impl VersionCompatibility for UserEventCompatibility {
    fn check_compatibility(reader_version: &str, event_version: &str) -> CompatibilityLevel {
        use CompatibilityLevel::*;
        
        match (reader_version, event_version) {
            // Same version - fully compatible
            (r, e) if r == e => FullyCompatible,
            
            // Reader newer than event - usually compatible
            ("2", "1") | ("3", "1") | ("3", "2") => FullyCompatible,
            
            // Reader older than event - may need migration
            ("1", "2") | ("1", "3") | ("2", "3") => RequiresMigration,
            
            // Special compatibility rules
            ("1.1", "1.0") => FullyCompatible, // Minor versions compatible
            
            _ => Incompatible,
        }
    }
}

// Usage in deserialization
fn deserialize_with_compatibility_check<T>(
    payload: &VersionedPayload,
    reader_version: &str,
) -> Result<T, SerializationError>
where
    T: DeserializeOwned + VersionCompatibility,
{
    let compatibility = T::check_compatibility(reader_version, &payload.version);
    
    match compatibility {
        CompatibilityLevel::FullyCompatible => {
            // Direct deserialization
            serde_json::from_slice(&payload.data)
                .map_err(SerializationError::Deserialization)
        }
        CompatibilityLevel::ReadOnly => {
            // Deserialize but mark as read-only
            let mut event: T = serde_json::from_slice(&payload.data)?;
            // Mark event as read-only somehow
            Ok(event)
        }
        CompatibilityLevel::RequiresMigration => {
            // Apply migration
            let migrated = migrate_to_version(&payload.data, &payload.version, reader_version)?;
            serde_json::from_slice(&migrated)
                .map_err(SerializationError::Deserialization)
        }
        CompatibilityLevel::Incompatible => {
            Err(SerializationError::IncompatibleVersion {
                reader: reader_version.to_string(),
                event: payload.version.clone(),
            })
        }
    }
}
```

## Event Archival and Compression

Handle old event versions efficiently:

```rust
use eventcore::archival::{EventArchiver, CompressionLevel};

struct VersionedEventArchiver {
    archiver: EventArchiver,
    retention_policy: RetentionPolicy,
}

#[derive(Debug, Clone)]
struct RetentionPolicy {
    pub keep_latest_versions: u32,
    pub archive_after_days: u32,
    pub compress_after_days: u32,
    pub delete_after_years: u32,
}

impl VersionedEventArchiver {
    async fn archive_old_versions(&self, stream_id: &StreamId) -> Result<ArchiveResult, ArchiveError> {
        let events = self.read_all_events(stream_id).await?;
        let mut archive_stats = ArchiveResult::default();
        
        for event in events {
            let age_days = (Utc::now() - event.occurred_at).num_days() as u32;
            
            match event.payload.version() {
                v if v < (CURRENT_VERSION - self.retention_policy.keep_latest_versions) => {
                    if age_days > self.retention_policy.delete_after_years * 365 {
                        // Delete very old events
                        self.archiver.delete_event(&event.id).await?;
                        archive_stats.deleted += 1;
                    } else if age_days > self.retention_policy.compress_after_days {
                        // Compress old events
                        self.archiver.compress_event(&event.id, CompressionLevel::High).await?;
                        archive_stats.compressed += 1;
                    } else if age_days > self.retention_policy.archive_after_days {
                        // Move to cold storage
                        self.archiver.archive_event(&event.id).await?;
                        archive_stats.archived += 1;
                    }
                }
                _ => {
                    // Keep recent versions in hot storage
                    archive_stats.retained += 1;
                }
            }
        }
        
        Ok(archive_stats)
    }
}

#[derive(Debug, Default)]
struct ArchiveResult {
    pub retained: u32,
    pub archived: u32,
    pub compressed: u32,
    pub deleted: u32,
}
```

## Version Monitoring

Monitor version usage in production:

```rust
use prometheus::{Counter, Histogram, IntGauge};

lazy_static! {
    static ref EVENT_VERSION_COUNTER: Counter = register_counter!(
        "eventcore_event_versions_total",
        "Total events by version"
    ).unwrap();
    
    static ref MIGRATION_DURATION: Histogram = register_histogram!(
        "eventcore_migration_duration_seconds",
        "Time spent migrating events"
    ).unwrap();
    
    static ref ACTIVE_VERSIONS: IntGauge = register_int_gauge!(
        "eventcore_active_event_versions",
        "Number of active event versions"
    ).unwrap();
}

struct VersionMetrics {
    version_counts: HashMap<String, u64>,
    migration_stats: HashMap<(String, String), MigrationStats>,
}

#[derive(Debug, Default)]
struct MigrationStats {
    pub total_migrations: u64,
    pub successful_migrations: u64,
    pub failed_migrations: u64,
    pub average_duration: Duration,
}

impl VersionMetrics {
    fn record_event_version(&mut self, event_type: &str, version: &str) {
        *self.version_counts
            .entry(format!("{}:{}", event_type, version))
            .or_insert(0) += 1;
        
        EVENT_VERSION_COUNTER
            .with_label_values(&[event_type, version])
            .inc();
    }
    
    fn record_migration(&mut self, from: &str, to: &str, duration: Duration, success: bool) {
        let key = (from.to_string(), to.to_string());
        let stats = self.migration_stats.entry(key).or_default();
        
        stats.total_migrations += 1;
        if success {
            stats.successful_migrations += 1;
        } else {
            stats.failed_migrations += 1;
        }
        
        // Update average duration
        let total_time = stats.average_duration * (stats.total_migrations - 1) as u32 + duration;
        stats.average_duration = total_time / stats.total_migrations as u32;
        
        MIGRATION_DURATION.observe(duration.as_secs_f64());
    }
    
    fn update_active_versions(&self) {
        let active_count = self.version_counts
            .keys()
            .map(|key| key.split(':').nth(1).unwrap_or("unknown"))
            .collect::<HashSet<_>>()
            .len();
        
        ACTIVE_VERSIONS.set(active_count as i64);
    }
}
```

## Testing Event Versions

Comprehensive testing for versioned events:

```rust
#[cfg(test)]
mod version_tests {
    use super::*;
    use proptest::prelude::*;
    
    #[test]
    fn test_version_serialization_roundtrip() {
        let v3_event = UserEventV3 {
            user_id: UserId::new(),
            email: Email::try_new("test@example.com").unwrap(),
            profile: UserProfile {
                first_name: "Test".to_string(),
                last_name: "User".to_string(),
                bio: Some("Test bio".to_string()),
                avatar_url: None,
            },
            preferences: UserPreferences::default(),
        };
        
        let serializer = EventSerializer::new();
        
        // Serialize
        let payload = serializer.serialize_event(&v3_event).unwrap();
        assert_eq!(payload.version, "3");
        
        // Deserialize
        let deserialized: UserEventV3 = serializer.deserialize_event(&payload).unwrap();
        assert_eq!(v3_event.user_id, deserialized.user_id);
        assert_eq!(v3_event.email, deserialized.email);
    }
    
    #[test]
    fn test_migration_chain() {
        let v1_event = UserEvent::V1(UserEventV1 {
            user_id: "user_123".to_string(),
            email: "test@example.com".to_string(),
            username: "test_user".to_string(),
        });
        
        let migration_chain = UserEventMigrationChain::new();
        let v3_event = migration_chain.migrate_to_latest(v1_event, 1).unwrap();
        
        match v3_event {
            UserEvent::V3(v3) => {
                assert_eq!(v3.email.to_string(), "test@example.com");
                assert_eq!(v3.profile.first_name, "test");
                assert_eq!(v3.profile.last_name, "user");
            }
            _ => panic!("Expected V3 event after migration"),
        }
    }
    
    proptest! {
        #[test]
        fn version_compatibility_is_transitive(
            v1 in 1u32..10,
            v2 in 1u32..10,
            v3 in 1u32..10,
        ) {
            let versions = [v1, v2, v3];
            versions.sort();
            let [min_v, mid_v, max_v] = versions;
            
            // If min compatible with mid, and mid compatible with max,
            // then migration chain should work
            if UserEventCompatibility::check_compatibility(
                &mid_v.to_string(), &min_v.to_string()
            ) != CompatibilityLevel::Incompatible &&
            UserEventCompatibility::check_compatibility(
                &max_v.to_string(), &mid_v.to_string()
            ) != CompatibilityLevel::Incompatible {
                // Migration from min to max should be possible
                prop_assert!(can_migrate_between_versions(min_v, max_v));
            }
        }
    }
    
    fn can_migrate_between_versions(from: u32, to: u32) -> bool {
        // Implementation depends on your migration chain
        to >= from && (to - from) <= MAX_MIGRATION_DISTANCE
    }
}
```

## Best Practices

1. **Version everything explicitly** - Don't rely on implicit versioning
2. **Plan migration paths** - Design how old versions become new ones
3. **Test all paths** - Test reading old events with new code
4. **Monitor version usage** - Track which versions are in production
5. **Clean up old versions** - Archive or delete very old events
6. **Document changes** - Keep detailed changelogs
7. **Gradual rollouts** - Deploy new versions incrementally
8. **Backward compatibility** - Maintain as long as practical

## Summary

Event versioning in EventCore:

- ✅ **Explicit versioning** - Clear version tracking
- ✅ **Migration support** - Transform between versions
- ✅ **Compatibility checking** - Know what works together
- ✅ **Performance monitoring** - Track version usage
- ✅ **Testing support** - Comprehensive test patterns

Key patterns:
1. Use semantic or linear versioning consistently
2. Define clear compatibility rules
3. Implement migration chains for complex changes
4. Monitor version usage in production
5. Test all migration paths thoroughly

Next, let's explore [Long-Running Processes](./03-long-running-processes.md) →