# Compliance

EventCore's immutable, append-only event log gives you a tamper-evident audit
trail for free, but compliance is largely about controls you build _around_
EventCore. This chapter shows where EventCore's real API fits and where you must
reach for storage-layer or application-layer mechanisms instead.

> **📋 Comprehensive Compliance Checklist**
>
> For a detailed compliance checklist covering OWASP, NIST, SOC2, PCI DSS, GDPR, and HIPAA requirements, see our [COMPLIANCE_CHECKLIST.md](https://github.com/jwilger/eventcore/blob/main/COMPLIANCE_CHECKLIST.md).
>
> This checklist provides actionable items for achieving compliance with major security frameworks and regulations.

> **What EventCore gives you, and what it does not**
>
> EventCore is a library, not a compliance platform. Its public surface is
> small: commands (`CommandLogic`), the `execute()` entry point, the
> `EventStore` trait and its backends, and `run_projection()` for read models.
> It deliberately does **not** ship encryption, key management, backup,
> retention, or audit-reporting APIs. Those are concerns you own at the
> application layer (your commands and projections) or the storage layer (your
> database). The sections below mark each abstraction as **app-owned**,
> **storage-layer**, or **EventCore** so you always know which is which.

## GDPR Compliance

### Data Protection Principles

1. **Lawfulness**: Store only data with legal basis
2. **Purpose Limitation**: Use data only for stated purposes
3. **Data Minimization**: Store only necessary data
4. **Accuracy**: Provide mechanisms to correct data
5. **Storage Limitation**: Implement retention policies
6. **Security**: Encrypt and protect personal data

### Right to Erasure (Right to be Forgotten)

EventCore events are immutable and append-only — there is no API to delete or
rewrite a stored event, and the PostgreSQL backend actively rejects `UPDATE`
and `DELETE` on the events table via database triggers. The standard way to
satisfy a GDPR Article 17 request against an immutable log is **crypto-shredding**:
encrypt each subject's personal data with a per-subject key (key management is
**app-owned** — use a KMS or vault), then delete the key to render the
ciphertext permanently unreadable. The events remain, but the PII inside them
can no longer be decrypted.

In EventCore terms, the erasure itself is just another fact in the log: you
record an "erased" event the same way you record any other — by running a
command through `execute()`. Application code must never construct events and
write them to the store directly; events are produced by a command's `handle()`
method and persisted atomically by `execute()`.

```rust
use eventcore::{
    execute, Command, CommandError, CommandLogic, Event, NewEvents, RetryPolicy,
    StreamId,
};
use serde::{Deserialize, Serialize};

// App-owned domain types. EventCore does not define UserId, KeyId, or
// timestamps — your application does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct UserId(String);

// App-owned event. Each event knows which stream it belongs to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
enum UserPrivacyEvent {
    UserDataErased {
        stream_id: StreamId,
        reason: String,
    },
}

impl Event for UserPrivacyEvent {
    fn stream_id(&self) -> &StreamId {
        match self {
            UserPrivacyEvent::UserDataErased { stream_id, .. } => stream_id,
        }
    }
    fn event_type_name() -> &'static str {
        "UserPrivacyEvent"
    }
}

// App-owned command. `#[derive(Command)]` generates the stream declaration
// from the `#[stream]`-tagged field.
#[derive(Command)]
struct ForgetUser {
    #[stream]
    user_stream: StreamId,
    reason: String,
}

impl CommandLogic for ForgetUser {
    type Event = UserPrivacyEvent;
    type State = ();

    fn apply(&self, state: Self::State, _event: &Self::Event) -> Self::State {
        state
    }

    fn handle(&self, _state: Self::State) -> Result<NewEvents<Self::Event>, CommandError> {
        // Record the erasure as an auditable fact. The key deletion that
        // actually renders the PII unreadable happens in your KMS/vault
        // integration (app-owned), outside the command.
        Ok(vec![UserPrivacyEvent::UserDataErased {
            stream_id: self.user_stream.clone(),
            reason: self.reason.clone(),
        }]
        .into())
    }
}

// Driving the command: delete the per-subject key in your vault first
// (app-owned), then persist the audit event via execute().
async fn forget_user<S: eventcore_types::EventStore>(
    store: S,
    user_stream: StreamId,
) -> Result<(), CommandError> {
    // 1. Delete the subject's encryption key in your KMS/vault here
    //    (app-owned — not an EventCore API).
    // 2. Record the erasure as an immutable, auditable fact.
    let command = ForgetUser {
        user_stream,
        reason: "GDPR Article 17 Request".to_string(),
    };
    execute(store, command, RetryPolicy::new()).await?;
    Ok(())
}
```

### Data Portability

Export a subject's data in a machine-readable format. EventCore's role here is
read-only: you read a subject's stream and serialize it. To read a stream's raw
history you call `EventStore::read_stream`, which returns a lazy
`EventStream`; the `collect_events` helper materializes it into a `Vec`. There
is no `find_user_streams` / `export_user_events` API in EventCore — modeling
which streams belong to a subject is **app-owned** (for example, by keying each
subject's stream on their identifier, or by maintaining a projection that
indexes streams by subject).

```rust
use eventcore::{collect_events, StreamId};
use eventcore_types::{EventStore, EventStoreError};
use serde::Serialize;

// App-owned export shape. EventCore does not define an export type.
#[derive(Serialize)]
struct UserDataExport {
    events: Vec<UserPrivacyEvent>,
}

// Read a subject's stream through the real EventStore API and collect it.
// Decryption of any encrypted fields is app-owned and happens after reading.
async fn export_user_data<S: EventStore>(
    store: &S,
    user_stream: StreamId,
) -> Result<UserDataExport, EventStoreError> {
    let stream = store.read_stream::<UserPrivacyEvent>(user_stream).await?;
    let events = collect_events(stream).await?;
    Ok(UserDataExport { events })
}
```

If a subject's data spans multiple streams, maintain a projection (a read model
built with `run_projection`) that maps the subject to their stream IDs, then
read each stream as above. The mapping is application domain knowledge, not
something EventCore infers.

## PCI DSS Compliance

### Never Store in Events

Events are immutable: once written, sensitive data in them cannot be redacted.
Never put raw cardholder data in an event payload. Store tokens from a
PCI-compliant tokenizer instead. The event types below are **app-owned** —
EventCore imposes no payment schema.

```rust
use eventcore::StreamId;
use serde::{Deserialize, Serialize};

// BAD - Never do this. Immutable events make this permanent.
#[derive(Serialize, Deserialize)]
struct PaymentProcessedBad {
    card_number: String,      // NEVER!
    cvv: String,             // NEVER!
    pin: String,             // NEVER!
}

// GOOD - Store only tokens and non-sensitive references.
#[derive(Serialize, Deserialize)]
struct PaymentProcessed {
    stream_id: StreamId,
    payment_id: String,
    card_token: String,    // From a PCI-compliant tokenizer
    last_four: String,     // "****1234"
    amount_cents: u64,
    merchant_ref: String,
}
```

### Audit Requirements

PCI DSS requires access logging for cardholder-data operations. EventCore's
event log records _what business facts happened_ (and is itself a strong audit
trail because it is immutable), but operational access logging — who viewed
what, from which IP — is **app-owned** infrastructure that lives alongside your
request handlers, not inside EventCore.

```rust
// App-owned audit logger. Not an EventCore type.
struct PciAuditLogger {
    logger: Box<dyn AuditSink>,
}

impl PciAuditLogger {
    async fn log_payment_access(
        &self,
        user_id: &str,
        action: &str,
        resource: &str,
        ip_address: &str,
    ) -> Result<(), AuditError> {
        let entry = AuditEntry {
            user_id: user_id.to_string(),
            action: action.to_string(),
            resource: resource.to_string(),
            ip_address: ip_address.to_string(),
            success: true,
        };
        self.logger.log(entry).await
    }
}
# trait AuditSink { }
# struct AuditEntry { user_id: String, action: String, resource: String, ip_address: String, success: bool }
# struct AuditError;
```

## HIPAA Compliance

### Protected Health Information (PHI)

At-rest encryption is a **storage-layer** concern, not an EventCore API.
Applications do not encrypt individual events and append them through some
EventCore method — no such method exists. Choose one of:

- **SQLite**: enable the non-default `encryption` feature on `eventcore-sqlite`,
  which links SQLCipher (`rusqlite/bundled-sqlcipher-vendored-openssl`) so the
  entire database file is encrypted at rest.
- **PostgreSQL**: use disk-, filesystem-, or tablespace-level encryption (for
  example, an encrypted volume, or transparent data encryption provided by your
  managed Postgres).
- **Field-level / crypto-shredding**: if you need per-subject erasure (see GDPR
  above), encrypt PHI fields with per-subject keys _in your command logic_
  before the event is constructed. This is app-owned: the ciphertext is just a
  string field in your event payload, and EventCore stores it like any other
  data.

```rust
use eventcore::StreamId;
use serde::{Deserialize, Serialize};

// App-owned event. PHI fields hold app-encrypted ciphertext (Strings);
// non-PHI fields are stored in the clear. EventCore treats all of these
// as ordinary serialized data.
#[derive(Serialize, Deserialize)]
struct PatientRecorded {
    stream_id: StreamId,
    patient_id: String,
    // App-encrypted PHI (ciphertext produced by your crypto layer)
    encrypted_name: String,
    encrypted_ssn: String,
    encrypted_diagnosis: String,
    // Non-PHI can be stored unencrypted
    admission_date: String,
    room_number: String,
}
```

### Access Controls

HIPAA's minimum-necessary rule is enforced in **app-owned** authorization code
that runs before you call `execute()` or read a projection. EventCore has no
concept of roles or permissions.

```rust
// App-owned role model. Not an EventCore type.
#[derive(Debug, Clone)]
enum HipaaRole {
    Doctor,
    Nurse,
    Admin,
    Billing,
}

impl HipaaRole {
    fn can_access_phi(&self) -> bool {
        matches!(self, HipaaRole::Doctor | HipaaRole::Nurse)
    }

    fn can_access_billing(&self) -> bool {
        matches!(self, HipaaRole::Admin | HipaaRole::Billing)
    }
}
```

Check authorization in your handler, then dispatch the command through
`execute()` only if the caller is permitted.

## SOX Compliance

### Financial Controls

Segregation of duties and approval workflows are **app-owned** policy that wraps
EventCore. Once the policy checks pass, the command is persisted through the
real `execute()` entry point — the only way to append events — which gives you
the immutable audit trail SOX wants.

```rust
use eventcore::{execute, CommandError, RetryPolicy};
use eventcore_types::EventStore;

// App-owned executor wrapper. The store generic must implement EventStore.
struct SoxCompliantExecutor<S: EventStore> {
    store: S,
    approvals: ApprovalService,
}

impl<S: EventStore + Clone> SoxCompliantExecutor<S> {
    async fn execute_financial_command(
        &self,
        command: FinancialCommand,
        requester: &str,
    ) -> Result<(), CommandError> {
        // Segregation of duties (app-owned policy).
        if command.amount_cents() > 1_000_000 {
            let approver = self.approvals.get_approver(requester).await;
            self.approvals.request_approval(&command, &approver).await;
        }

        // Execute with full, immutable audit trail via the real execute().
        // execute() takes the store and command by value.
        execute(self.store.clone(), command, RetryPolicy::new()).await?;

        Ok(())
    }
}
# struct ApprovalService;
# impl ApprovalService {
#     async fn get_approver(&self, _r: &str) -> String { String::new() }
#     async fn request_approval(&self, _c: &FinancialCommand, _a: &str) {}
# }
# struct FinancialCommand;
# impl FinancialCommand { fn amount_cents(&self) -> u64 { 0 } }
```

> Note: `execute()` takes the store and command **by value**. If your wrapper
> needs to run multiple commands, clone the store (the in-memory and backend
> stores are cheap to clone, typically wrapping a shared pool or `Arc`) or pass
> a reference — `execute(&store, command, policy)` works too, because
> `EventStore` is implemented for `&T`.

## General Compliance Features

### Audit Trail

The event log _is_ your audit trail. Because events are immutable and
append-only, every state change is permanently recorded with its full context.
To produce a compliance report, build a **read model** with `run_projection`
that folds the relevant events into the shape an auditor needs — do not try to
mutate or summarize events in place.

EventCore identifies events in a projection by their `StreamPosition` cursor
and their owning `StreamId`. There is no separate `EventId` type. A projection that
backs a compliance report typically captures these alongside your domain data.

```rust
use eventcore::{StreamId, StreamPosition};
use std::collections::HashMap;

// App-owned read-model row produced by a projection. StreamId and
// StreamPosition are real EventCore types; the rest is your domain.
#[derive(Debug)]
struct ComplianceAuditRow {
    position: StreamPosition,
    stream_id: StreamId,
    actor: String,
    action: String,
    regulation: String, // "GDPR", "PCI", "HIPAA"
    details: HashMap<String, String>,
}
```

Drive the projection with `run_projection(projector, &backend, config)` (the
backend is passed **by reference**) and query the resulting read model for your
report.

### Data Retention

Events are immutable, so "retention" against an event store does not mean
deleting rows in place — EventCore exposes no API to delete or rewrite events,
and the Postgres backend blocks `DELETE` at the database level. Implement
retention at the boundaries instead:

- **Erasure / "delete"**: use crypto-shredding (delete per-subject keys), as in
  the GDPR section. The events remain but become unreadable. The decision to
  shred is **app-owned**.
- **Archive**: copy old data to cold storage at the **storage layer** — for
  PostgreSQL, dump partitions or tables with `pg_dump` / your backup tooling
  (e.g. CloudNativePG scheduled backups) and move them off the hot database;
  for SQLite or the file store, snapshot or copy the underlying file.
- **Anonymize**: record a new event (via a command and `execute()`) that
  supersedes the personal data in your read models, and rely on crypto-shredding
  to neutralize the original PII. You never edit the original event.

```rust
// App-owned retention policy description. EventCore has no RetentionManager,
// and there is no event-store method that deletes or rewrites events.
enum RetentionAction {
    /// Crypto-shred per-subject keys (events remain, PII unreadable).
    Shred,
    /// Archive at the storage layer (pg_dump / file snapshot).
    Archive,
    /// Supersede via a new command + execute(), then crypto-shred originals.
    Anonymize,
}

struct RetentionPolicy {
    regulation: String,
    data_type: String,
    retention_days: u32,
    action: RetentionAction,
}
```

The actual archive/snapshot mechanics live in your infrastructure (backup
schedules, object storage lifecycle rules), not in EventCore. EventCore's
contribution is the guarantee that the events it _does_ keep are unaltered.

## Compliance Checklist

- [ ] Encrypt PII/PHI at the storage layer (SQLCipher / disk encryption) or via
      app-owned field encryption + crypto-shredding
- [ ] Add app-owned access logging around command dispatch and projection reads
- [ ] Define retention policies as crypto-shred / storage-layer archive /
      supersede-via-command — never in-place event deletion
- [ ] Implement right to erasure (GDPR) via per-subject key deletion
- [ ] Provide data export by reading streams (`read_stream` + `collect_events`)
- [ ] Configure access controls (RBAC/ABAC) in application code before
      `execute()`
- [ ] Implement approval workflows (SOX) as policy wrapping `execute()`
- [ ] Set up monitoring and alerting in your application/infrastructure
- [ ] Document all compliance measures
- [ ] Run regular compliance audits against the immutable event log
