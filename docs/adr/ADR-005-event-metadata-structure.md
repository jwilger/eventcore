# ADR-005: Event Metadata Structure

## Status

accepted

## Context

EventCore events require structured metadata to support auditing, compliance, distributed tracing, and debugging. Events are immutable facts that, once persisted, become the permanent record of what happened in the system. The metadata accompanying each event must capture not just the domain change itself, but also the context in which that change occurred.

**Key Forces:**

1. **Audit and Compliance**: Business and regulatory requirements demand knowing who performed an action, when it occurred, and why (e.g., GDPR, SOX, HIPAA)
2. **Distributed Tracing**: Modern distributed systems require correlation across service boundaries and causation chains linking events to their triggering operations
3. **Debugging and Observability**: Production debugging requires understanding event relationships, timing, and context without reproducing the entire system state
4. **Extensibility**: Applications have domain-specific metadata needs that the library cannot anticipate
5. **Performance**: Metadata storage adds overhead to every event write and read operation
6. **Immutability**: Once written, event metadata must be tamper-evident and unchangeable
7. **Global Ordering**: Events across different streams need deterministic time-based ordering for projection building and debugging
8. **Type Safety**: Metadata must follow ADR-003's type system patterns (validated types, no primitive obsession)

**Why This Decision Now:**

Event metadata structure is referenced in ADR-002 as a "first-class concern" of the EventStore trait but not fully specified. Before implementing the EventStore trait and event persistence, we must define what metadata every event carries and why, as this shapes the storage schema, API design, and consumer experience.

## Decision

EventCore events will carry structured, immutable metadata with the following fields:

**1. Event Identity and Ordering**

- **Event ID**: Globally unique identifier using UUIDv7 format for time-based ordering across all streams
- **Stream ID**: Identifier of the stream this event belongs to (from ADR-003 validated StreamId type)
- **Stream Version**: Position of this event within its stream (monotonically increasing from 1)
- **Timestamp**: When the event was committed to the event store (not when the command was initiated)

**2. Distributed Tracing and Correlation**

- **Correlation ID**: Groups all events and operations belonging to the same logical business operation (e.g., all events from a single API request)
- **Causation ID**: Identifies the immediate cause of this event (typically the command ID or parent event ID that triggered this event)

**3. Extensibility**

- **Custom Metadata**: Generic type parameter `M` with trait bounds `Serialize + DeserializeOwned`, allowing applications to define their own strongly-typed metadata structures

**4. Type Safety Integration**

All metadata fields follow ADR-003 patterns:

- Validated domain types (EventId, StreamId, CorrelationId, etc.) instead of primitives
- Construction returns Result types for validation errors
- Types implement standard traits (Debug, Clone, Serialize, Deserialize)
- Custom metadata uses generic type parameter for application-defined strongly-typed structures

**5. Immutability Guarantee**

Once an event is committed to the event store:

- All metadata fields are immutable and cannot be modified
- Storage backends ensure metadata integrity (constraints, checksums, append-only storage)
- Any metadata changes require new events, not modifications

## Rationale

**Why UUIDv7 for Event ID:**

Global event ordering is essential for:

- Projection building across multiple streams requires deterministic ordering
- Debugging requires understanding temporal relationships between events
- Event replay and testing benefit from reproducible ordering

UUIDv7 provides time-based ordering while maintaining uniqueness guarantees without coordination. Alternative UUIDs (v4) lack temporal information; sequential IDs require centralized coordination.

**Why Correlation and Causation IDs:**

Modern distributed systems span multiple services, commands, and events. Correlation and causation provide:

- **Correlation ID** links all work for a single business operation (e.g., "transfer money" request generates events in multiple streams, all share correlation ID)
- **Causation ID** tracks direct cause-effect relationships (e.g., "account debited" event caused by "transfer money" command)
- Together they enable distributed tracing tools to reconstruct entire operation flows
- Essential for debugging multi-stream operations from ADR-001

Without these IDs, tracing multi-stream operations becomes guesswork.

**Why Timestamp at Commit Time:**

Timestamp represents when the event became part of the permanent record:

- Enables time-based queries and projections
- Supports event retention policies
- Aids debugging by showing when state changes occurred
- Commit time (not command initiation time) ensures consistency with version ordering

Command initiation time can be included in custom metadata if needed for domain purposes.

**Why Generic Type for Custom Metadata:**

EventCore is an infrastructure library serving diverse domains with varying metadata needs. Applications require different business-specific metadata based on their domains:

- Audit information (e.g., actor IDs, IP addresses, user agents)
- Business context (e.g., invoice IDs, transaction amounts, tenant IDs)
- Compliance data (e.g., data classification, retention periods)

A generic type parameter `M: Serialize + DeserializeOwned` maintains EventCore's infrastructure focus while providing flexibility:

- **Type Safety Maintained**: Applications define strongly-typed metadata structs, gaining compile-time validation per ADR-003 principles
- **No Primitive Obsession**: Applications enforce their own metadata schemas using validated domain types
- **Infrastructure Neutrality**: EventCore doesn't impose business domain assumptions (like "users" or "actors")
- **Compile-Time Checks**: Applications catch metadata structure errors at compile time, not runtime
- **Application Control**: Each application defines metadata appropriate for its domain and compliance requirements

EventCore provides the infrastructure mechanism (generic metadata storage and retrieval); applications provide the business-specific metadata structure. This maintains clear separation between infrastructure concerns (EventCore) and business domain concerns (applications).

**Why Immutability:**

Events are facts, not mutable records:

- Audit integrity requires tamper-evidence
- Event replay depends on events never changing
- Debugging requires stable event history
- Regulatory compliance demands immutable audit logs

Metadata modification would undermine event sourcing fundamentals and create audit gaps.

**Trade-offs Accepted:**

- **Storage Overhead**: Metadata adds ~200-500 bytes per event depending on custom metadata size
- **Serialization Cost**: More metadata means more bytes to serialize/deserialize
- **Query Complexity**: Rich metadata enables complex queries but requires indexing strategy
- **Generic Complexity**: Applications must define and manage their own metadata types

These trade-offs are acceptable because:

- Metadata overhead is negligible compared to value of auditability and traceability
- Storage is cheap; losing audit trail or debugging capability is expensive
- Complex queries are opt-in (applications index what they need)
- Generic type parameter maintains type safety per ADR-003 while giving applications full control

## Consequences

**Positive:**

- **Complete Infrastructure Trail**: Every event captures when, where, and correlation context for tracing
- **Distributed Tracing**: Correlation and causation enable full request tracing across service boundaries
- **Debugging Support**: Metadata provides context needed to understand event relationships and timing
- **Global Event Ordering**: UUIDv7 enables deterministic ordering for projections and debugging
- **Type-Safe Extensibility**: Generic metadata type maintains ADR-003 type safety while supporting domain-specific needs
- **Type Safety Throughout**: All metadata (standard and custom) follows ADR-003 validated type patterns
- **Tamper Evidence**: Immutability ensures audit trail integrity
- **Infrastructure Neutrality**: No business domain assumptions (actors, users, etc.) imposed by library

**Negative:**

- **Storage Overhead**: Metadata increases event size and storage requirements
- **Performance Impact**: Serialization and deserialization overhead for metadata fields
- **Index Requirements**: Querying by metadata (correlation, custom fields) requires backend indexes
- **Application Metadata Responsibility**: Applications must define and manage their own metadata type structures
- **Migration Burden**: Adding new standard metadata fields requires schema migrations

**Enabled Future Decisions:**

- Observability integrations can extract correlation/causation for distributed tracing (OpenTelemetry, Jaeger)
- Projection builders can use event timestamps for time-based filtering and ordering
- Infrastructure queries can filter by timestamp ranges, correlation IDs, stream IDs
- Compliance tools can verify immutability and export audit trails
- Debugging tools can reconstruct operation flows via causation chains
- Storage backends can optimize indexes based on common metadata queries
- Applications define custom metadata structures for domain-specific projections and analytics
- Applications implement audit trails with actor information in their custom metadata types

**Constrained Future Decisions:**

- Storage backends must preserve all metadata fields exactly as provided (standard and custom)
- Event serialization formats must include all standard metadata fields
- Metadata types must follow ADR-003 validated type patterns
- Custom metadata type `M` must implement `Serialize + DeserializeOwned` for persistence
- Adding new standard metadata fields requires library major version bump
- Metadata fields cannot be modified after event commit (append-only operations only)
- UUIDv7 generation must be consistent and monotonic for time-based ordering
- Applications control custom metadata schema; EventCore provides storage mechanism only

## Alternatives Considered

### Alternative 1: Minimal Metadata (Only Event ID and Stream ID)

Include only essential identity fields, leaving all context to custom metadata.

**Rejected Because:**

- Pushes common requirements (correlation, causation, timestamp) to every application
- No standardization means inconsistent infrastructure patterns across applications
- Distributed tracing requires manual implementation by every consumer
- Infrastructure needs standard fields (when, correlation context) built-in
- Loses opportunity to enforce type safety on common metadata
- Applications would reinvent correlation/causation in incompatible ways
- Library value proposition diminished (infrastructure should solve common problems)

### Alternative 2: Separate Metadata Store

Store domain events in one location and metadata in separate database/service.

**Rejected Because:**

- Violates atomicity guarantees from ADR-001 (event and metadata must be written together)
- Introduces complexity of keeping two stores synchronized
- Potential for metadata loss or inconsistency on failures
- Query complexity increased (join across two stores)
- Performance overhead of multiple storage operations
- Contradicts event sourcing principle (events are complete records)
- Audit trail integrity compromised (events and metadata can diverge)

### Alternative 3: No Custom Metadata Extension Point

Define only standard fields; applications implement custom metadata in event payload.

**Rejected Because:**

- Forces domain metadata into event payload where it doesn't belong
- Metadata and domain data have different purposes and lifecycle
- Difficult to query events by metadata (requires deserializing payloads)
- Loses separation of concerns (infrastructure vs domain)
- Applications resort to workarounds (additional events just for metadata)
- Limits extensibility of event sourcing infrastructure

### Alternative 4: Use Event Payload for All Context

Store correlation, causation, timestamp as part of every event's domain payload.

**Rejected Because:**

- Requires every event type to include infrastructure fields
- Violates single responsibility (domain events shouldn't know about tracing)
- Duplicates fields across all event types
- Makes event evolution harder (changing metadata structure affects all events)
- Loses ability to enforce metadata structure at library level
- Query optimization difficult (metadata scattered in event payloads)

### Alternative 5: Sequential Integer Event IDs

Use auto-incrementing integers instead of UUIDv7 for event IDs.

**Rejected Because:**

- Requires centralized coordination for ID generation (single point of contention)
- Difficult to generate IDs before commit in distributed scenarios
- No time-based ordering information embedded in ID
- Not suitable for distributed event sourcing systems
- UUID is standard practice in event sourcing ecosystem
- UUIDv7 provides both uniqueness and ordering without coordination

### Alternative 6: Mutable Metadata Fields

Allow updating certain metadata fields after event commit (e.g., adding correlation information retroactively).

**Rejected Because:**

- Violates event sourcing immutability principle
- Compromises audit trail integrity (can't trust historical records)
- Introduces complex versioning for metadata itself
- Regulatory compliance requires immutable audit logs
- Creates ambiguity (which metadata version is authoritative?)
- Event replay becomes unreliable if metadata changes

### Alternative 7: Include Actor ID and Actor Type as Standard Fields

Include actor attribution fields (Actor ID, Actor Type) in standard event metadata.

**Rejected Because:**

- **Business Domain Concern**: Actor information is application/business logic, not infrastructure concern
- **Assumes Domain Model**: EventCore shouldn't assume applications have "users" or "actors"
- **Not Universal**: Many use cases don't have actors (system-generated events, time-triggered events, integration events)
- **Forces Inappropriate Modeling**: Applications forced to invent actor IDs for non-actor scenarios
- **Infrastructure Scope Violation**: EventCore is infrastructure library; actor concepts are business domain
- **Reduces Flexibility**: Standard fields can't easily accommodate different actor models across applications
- **Better in Custom Metadata**: Applications needing actor information can add it to their custom metadata type with appropriate domain modeling

EventCore remains infrastructure-focused; applications add business-specific metadata (including actors) as needed.

### Alternative 8: Separate Correlation and Causation Tracking System

Use external tracing system (OpenTelemetry only) instead of embedding in events.

**Rejected Because:**

- Events become the source of truth for what happened; tracing must align with events
- External tracing system may have different retention than events
- Difficult to correlate events with traces retroactively
- Debugging requires two separate systems instead of one unified record
- Event replay doesn't preserve tracing context
- Increases operational complexity (two systems to maintain)

### Alternative 9: Stringly-Typed Custom Metadata Map

Use `HashMap<String, Value>` or similar map structure for custom metadata instead of generic type parameter.

**Rejected Because:**

- **Loses Type Safety**: Map-based approach uses string keys and untyped values, losing compile-time validation
- **Runtime Errors**: Typos in metadata keys only discovered at runtime, not compile time
- **No Schema Validation**: Applications can't enforce metadata structure requirements at type level
- **Violates ADR-003**: Primitive obsession (strings) instead of validated domain types
- **Poor Developer Experience**: No IDE autocomplete, no type checking, no refactoring support
- **Inconsistent with Library Philosophy**: EventCore emphasizes type safety; stringly-typed maps contradict this
- **Testing Burden**: Requires runtime tests to verify metadata structure instead of compiler guarantees

Generic type parameter maintains type safety throughout while still allowing application-specific flexibility.

## References

- ADR-001: Multi-Stream Atomicity Implementation Strategy
- ADR-002: Event Store Trait Design (metadata as "first-class concern")
- ADR-003: Type System Patterns for Domain Safety (validated types)
- ADR-004: Error Handling Hierarchy (correlation/causation in errors)
- REQUIREMENTS_ANALYSIS.md: FR-4.2 Event Ordering (UUIDv7)
- REQUIREMENTS_ANALYSIS.md: FR-4.3 Event Metadata
- UUIDv7 Specification: https://datatracker.ietf.org/doc/draft-ietf-uuidrev-rfc4122bis/
- OpenTelemetry Trace Context: https://www.w3.org/TR/trace-context/
