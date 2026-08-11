# Chapter 5.2: Event Versioning

Event versioning is a systematic approach to managing changes in event schemas
while preserving the ability to read historical data. This chapter covers how
EventCore handles schema change and the application-level patterns you can build
on top of it.

## How EventCore Handles Versions

It is important to be clear about what EventCore does and does not do, because
the rest of this chapter builds on it.

EventCore stores events as JSON blobs via `serde`. Deserialization is driven
entirely by the JSON structure. The `event_type` column stored alongside each
event (derived from your event type's `Event::event_type_name()`) is
informational metadata for auditing and debugging — it is **not** used to route
or version events during deserialization.

Because of this, EventCore deliberately has **no** version registry, no upcasting
subsystem, and no per-event version field. Schema evolution is handled with two
plain `serde` techniques, formalized in
[ADR-0035](../../adr/ADR-0035-event-schema-evolution-via-enum-variants.md):

1. **Additive changes** — add a new field with `#[serde(default)]`. Old events
   deserialize with the default value. No other changes are required.
2. **Incompatible changes** — add a **new enum variant** rather than mutating an
   existing one. Old variants remain so historical events still deserialize;
   `apply()` and projectors match every variant.

Everything else in this chapter — explicit semantic version markers, migration
chains, archival, and version metrics — is an **application-level** pattern you
may choose to build. None of it is provided by EventCore, and none of it is
required to evolve schemas safely.

## Versioning Strategies

### Additive Evolution with serde Defaults

The simplest evolution is adding fields. Mark new fields with `#[serde(default)]`
so historical events that lack the field still deserialize:

```rust
use eventcore::StreamId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
enum UserEvent {
    Registered {
        stream_id: StreamId,
        email: Email,
        username: Username,

        // Added later — old events deserialize with the default.
        #[serde(default)]
        preferences: UserPreferences,
    },
}
```

This is a backward-compatible change: no migration step, no new variant, and
the same `apply()` / projection code keeps working.

### Incompatible Evolution with New Variants

When a change cannot be expressed as an additive field — a field is removed, a
type changes meaning, or an invariant changes — add a new variant instead of
editing the old one (see ADR-0035). Old variants are kept forever because they
represent historical facts:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
enum UserEvent {
    // Original variant — still deserializes from historical events.
    RegisteredV1 {
        user_id: UserId,
        email: Email,
        username: Username,
    },

    // Incompatible reshape — emitted going forward.
    RegisteredV2 {
        user_id: UserId,
        email: Email,
        first_name: FirstName,
        last_name: LastName,
    },
}
```

The rules from ADR-0035:

1. Old variants are never removed.
2. `apply()` handles all variants.
3. `handle()` emits only the latest variant.
4. Projectors handle all variants.

The Rust compiler enforces rule 2 and rule 4 for you: adding a variant turns
every non-exhaustive `match` into a compile error until you handle it.

### Application-Level Version Markers (Optional)

If you want explicit, human-readable version markers for documentation or
compatibility checks, you can define your own value type. This is purely an
application convention — EventCore never reads it:

```rust
// Application-level version marker. Not an EventCore type.
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
}
```

## Version-Aware Serialization

EventCore does **not** provide a versioned serializer, a type registry, or a
versioned payload wrapper. Serialization is handled by `serde` and the JSON the
backend stores. Versioning happens at the type level using the variant approach
above.

The idiomatic way to carry an explicit version tag in the JSON is serde's
externally- or internally-tagged enum representation. With an internal tag, the
version becomes a field in the stored JSON and `serde` selects the right variant
on read:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "version")]
enum UserEvent {
    #[serde(rename = "1")]
    V1(UserEventV1),

    #[serde(rename = "2")]
    V2(UserEventV2),

    #[serde(rename = "3")]
    V3(UserEventV3),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct UserEventV1 {
    user_id: UserId,
    email: Email,
    username: Username,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct UserEventV2 {
    user_id: UserId,
    email: Email,
    first_name: FirstName,
    last_name: LastName,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct UserEventV3 {
    user_id: UserId,
    email: Email,
    profile: UserProfile,
    preferences: UserPreferences,
}
```

With this layout, every event is just one of the variants of `UserEvent`. There
is nothing to register: when EventCore reads the stream, `serde` deserializes the
JSON straight into the correct variant.

## Migration Chains (Application-Level)

Because EventCore does not transform historical events, "migration" is an
application-level read-time concern: you fold every historical variant into the
shape your code wants to work with. The variant `match` in `apply()` and in your
projectors already does this for write-model and read-model state respectively.

If you also want a reusable, free-standing helper that normalizes an old variant
to the latest shape (for example, to share logic between a command and a
projection's read path), you can write one. The example below is **not** an
EventCore API — it is plain application code operating on your own enum:

```rust
// Application-level read-time normalization. Not an EventCore API.
// Folds any historical variant into the latest in-memory shape.
fn normalize_to_latest(event: UserEvent) -> UserEventV3 {
    match event {
        UserEvent::V1(v1) => {
            // V1 -> V2: extract names from the legacy username.
            let (first_name, last_name) = split_username(v1.username.as_ref());
            normalize_to_latest(UserEvent::V2(UserEventV2 {
                user_id: v1.user_id,
                email: v1.email,
                first_name,
                last_name,
            }))
        }
        UserEvent::V2(v2) => UserEventV3 {
            user_id: v2.user_id,
            email: v2.email,
            profile: UserProfile::from_names(v2.first_name, v2.last_name),
            preferences: UserPreferences::default(),
        },
        UserEvent::V3(v3) => v3,
    }
}

fn split_username(username: &str) -> (FirstName, LastName) {
    // ... domain-specific parsing returning your own validated types ...
    # unimplemented!()
}
```

Note that this normalization happens **in memory after reading** — the stored
events are never rewritten. This preserves immutability: a V1 event stays a V1
event on disk forever.

## Reading and Writing Versioned Events

EventCore has a single write path and a single read path. There is no
`write_versioned_events`, `read_versioned_stream`, `EventToWrite`, or
`WriteResult` — those types and methods do not exist.

### Writing

Events are never constructed and appended by application code directly. A
command's `handle()` returns the new events as `NewEvents`, and `execute()`
appends them atomically with optimistic concurrency control. To "write a
versioned event," your `handle()` simply returns the latest variant:

```rust
use eventcore::{execute, CommandError, CommandLogic, NewEvents, RetryPolicy};

impl CommandLogic for RegisterUser {
    type Event = UserEvent;
    type State = UserState;

    fn apply(&self, state: Self::State, event: &Self::Event) -> Self::State {
        // Fold every historical variant into write-model state.
        match event {
            UserEvent::V1(v1) => state.with_user_v1(v1),
            UserEvent::V2(v2) => state.with_user_v2(v2),
            UserEvent::V3(v3) => state.with_user_v3(v3),
        }
    }

    fn handle(&self, state: Self::State) -> Result<NewEvents<Self::Event>, CommandError> {
        state.require_not_registered()?;
        // Always emit the latest variant.
        Ok(vec![UserEvent::V3(self.to_v3())].into())
    }
}

// Persisted atomically through the single entry point.
let response = execute(store, command, RetryPolicy::default()).await?;
```

### Reading

To read a stream's full history, use the `EventStore::read_stream` method and
the `collect_events` helper to materialize it as a `Vec`. The store yields events
in stream-version order; your code matches on the variant to interpret each one:

```rust
use eventcore::{collect_events, StreamId};
use eventcore_types::EventStore;

let stream_id = StreamId::try_new("user-123")?;
let events: Vec<UserEvent> = collect_events(store.read_stream(stream_id).await?).await?;

for event in events {
    // Each event is one of the historical variants.
    let latest = normalize_to_latest(event);
    // ... use the normalized in-memory shape ...
}
```

In normal application code you rarely read streams by hand like this — that is
what command execution and projections do for you. The snippet above is for the
occasional case where you need the raw history (tooling, diagnostics, one-off
analysis).

## Version-Aware Projections

Projections are the read model. A `Projector` receives each event via `apply()`
and updates its read model. Because EventCore's compiler-checked enums force you
to handle every variant, a version-aware projection is just an exhaustive
`match`:

```rust
use eventcore::{Projector, StreamPosition};

// Application-level read model state.
struct UserReadModel {
    users: HashMap<UserId, UserView>,
}

impl Projector for UserProjection {
    type Event = UserEvent;
    type Error = std::convert::Infallible;
    type Context = UserReadModel;

    fn apply(
        &mut self,
        event: Self::Event,
        _position: StreamPosition,
        ctx: &mut Self::Context,
    ) -> Result<(), Self::Error> {
        match event {
            UserEvent::V1(v1) => {
                let view = UserView {
                    id: v1.user_id,
                    email: v1.email,
                    display_name: v1.username.into(),
                    profile: None,
                    preferences: UserPreferences::default(),
                };
                ctx.users.insert(view.id.clone(), view);
            }
            UserEvent::V2(v2) => {
                let view = UserView {
                    id: v2.user_id,
                    email: v2.email,
                    display_name: format!("{} {}", v2.first_name, v2.last_name).into(),
                    profile: None,
                    preferences: UserPreferences::default(),
                };
                ctx.users.insert(view.id.clone(), view);
            }
            UserEvent::V3(v3) => {
                let view = UserView {
                    id: v3.user_id,
                    email: v3.email,
                    display_name: v3.profile.display_name(),
                    profile: Some(v3.profile),
                    preferences: v3.preferences,
                };
                ctx.users.insert(view.id.clone(), view);
            }
        }
        Ok(())
    }

    fn name(&self) -> &str {
        "user-read-model"
    }
}
```

The projector is run with `run_projection(projector, &backend, config)`. See
[Projections](../02-getting-started/04-projections.md) for the full runner API.

## Version Compatibility Rules (Application-Level)

If your domain needs an explicit compatibility policy — for instance, an external
consumer that should refuse to process events it does not understand — you can
encode the rules in application code. This is your own logic; EventCore does not
gate reads on a compatibility check:

```rust
// Application-level compatibility policy. Not an EventCore concept.
#[derive(Debug, Clone, PartialEq)]
enum CompatibilityLevel {
    FullyCompatible,   // Can interpret directly
    RequiresMigration, // Need normalization to use
    Incompatible,      // Cannot use
}

fn check_compatibility(reader_version: u32, event_version: u32) -> CompatibilityLevel {
    use CompatibilityLevel::*;
    match (reader_version, event_version) {
        (r, e) if r == e => FullyCompatible,
        // Reader newer than the event — normalize forward.
        (r, e) if r > e => RequiresMigration,
        // Reader older than the event — cannot interpret newer schema.
        _ => Incompatible,
    }
}
```

In practice, the new-variant strategy makes most of this unnecessary: as long as
all readers are rebuilt against the current event enum, the compiler guarantees
every reader handles every variant.

## Event Archival and Retention (Application-Level)

EventCore treats events as immutable, append-only facts and does not provide
archival, compression, or deletion APIs. If your operational requirements call
for it, archival is implemented at the storage layer outside of EventCore — for
example, with database-native partitioning, cold-storage tiering, or backup
tooling specific to your backend.

The key constraints to respect:

- **Do not rewrite events.** Schema evolution never mutates stored events;
  archival should not either.
- **Preserve order and identity.** Any retention scheme must keep events in
  stream-version order and preserve the store's projection-cursor semantics so
  projections can resume from a checkpoint.
- **Coordinate with projections.** Removing or relocating historical events can
  invalidate read models that replay from the beginning. Rebuild affected
  projections after any retention operation.

Design these policies around your chosen backend's capabilities rather than
expecting EventCore to manage retention.

## Monitoring Version Usage

EventCore does not ship a metrics module. It exposes exactly one metrics
integration point: the `MetricsHook` trait, wired into command execution via
`RetryPolicy::with_metrics_hook`. The hook is notified on retry attempts; all
other telemetry is owned by your application.

```rust
use eventcore::{execute, MetricsHook, RetryContext, RetryPolicy};

// Application-owned metrics sink implementing the EventCore hook.
struct PrometheusRetryHook;

impl MetricsHook for PrometheusRetryHook {
    fn on_retry_attempt(&self, ctx: &RetryContext) {
        // ctx exposes: streams (Vec<StreamId>), attempt (AttemptNumber),
        // delay_ms (DelayMilliseconds).
        metrics::counter!("eventcore_command_retries_total").increment(1);
    }
}

let policy = RetryPolicy::default().with_metrics_hook(PrometheusRetryHook);
let response = execute(store, command, policy).await?;
```

To track which event versions are flowing through your system, instrument your
own projection or command code — for example, increment a counter inside the
projector's `apply()` based on the matched variant. EventCore does not record
version usage for you, so this counting lives entirely in application code:

```rust
// Application-owned metric, incremented inside your own projector/handler.
fn record_event_version(variant: &str) {
    metrics::counter!("app_event_versions_total", "variant" => variant.to_string())
        .increment(1);
}
```

## Testing Event Versions

The most valuable versioning test verifies that historical JSON still
deserializes and that every variant is handled. Because there is no serializer to
register, you test with plain `serde_json` against the JSON shape your backend
stores:

```rust
#[cfg(test)]
mod version_tests {
    use super::*;

    #[test]
    fn historical_v1_json_still_deserializes() {
        // JSON captured from a real V1 event on disk.
        let json = r#"{ "version": "1", "user_id": "user-123",
                        "email": "test@example.com", "username": "test_user" }"#;

        let event: UserEvent = serde_json::from_str(json).expect("V1 must still decode");
        assert!(matches!(event, UserEvent::V1(_)));
    }

    #[test]
    fn additive_field_defaults_for_old_events() {
        // Old JSON lacks the later #[serde(default)] field.
        let json = r#"{ "version": "3", "user_id": "user-123",
                        "email": "test@example.com", "profile": { } }"#;

        let event: UserEvent = serde_json::from_str(json).expect("old V3 must decode");
        if let UserEvent::V3(v3) = event {
            assert_eq!(v3.preferences, UserPreferences::default());
        }
    }

    #[test]
    fn read_time_normalization_covers_all_variants() {
        let v1 = UserEvent::V1(UserEventV1 {
            user_id: UserId::new(),
            email: Email::try_new("test@example.com").unwrap(),
            username: Username::try_new("test_user").unwrap(),
        });

        let latest = normalize_to_latest(v1);
        assert_eq!(latest.email.as_ref(), "test@example.com");
    }
}
```

Drive the full path — append the latest variant through `execute()` and replay
through a projector — with an integration test against
`eventcore_memory::InMemoryEventStore`, exactly as a downstream consumer would.

## Best Practices

1. **Prefer additive changes.** A `#[serde(default)]` field is the cheapest
   evolution and needs no new variant.
2. **Add variants, never mutate them.** Incompatible changes become new enum
   variants; old variants stay forever (ADR-0035).
3. **Let the compiler enforce coverage.** Exhaustive `match` in `apply()` and in
   projectors guarantees every version is handled.
4. **Normalize at read time, never rewrite on disk.** Stored events are
   immutable facts.
5. **Keep version logic in application code.** EventCore does not version,
   migrate, or archive for you.
6. **Capture real historical JSON in tests.** The contract is "old JSON still
   deserializes," so test against the bytes the backend stores.
7. **Rebuild readers together.** When you add a variant, recompile and redeploy
   every command and projector that matches the event.

## Summary

Event versioning with EventCore:

- ✅ **serde-driven** — events are JSON; deserialization follows the JSON shape
- ✅ **Additive changes** — `#[serde(default)]` fields, no migration step
- ✅ **Incompatible changes** — new enum variants, old variants preserved
- ✅ **Compiler-enforced coverage** — exhaustive `match` across all versions
- ✅ **Immutable history** — normalize in memory, never rewrite stored events

What EventCore does **not** provide (and you do not need): a version registry,
an upcasting subsystem, per-event version metadata, a versioned serializer, or
archival APIs. Migration chains, compatibility policies, retention, and version
metrics are application-level patterns you add only if your domain calls for
them.

Key patterns:

1. Reach for `#[serde(default)]` first; add a new variant only for incompatible
   change.
2. Handle every variant in `apply()` and in projectors.
3. Emit only the latest variant from `handle()`.
4. Persist exclusively through `execute()`; read history via `read_stream` +
   `collect_events`.
5. Test that historical JSON still deserializes.

Next, let's explore [Long-Running Processes](./03-long-running-processes.md) →
</content>
