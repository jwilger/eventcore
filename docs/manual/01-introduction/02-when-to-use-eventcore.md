# Chapter 1.2: When to Use EventCore

In the modern age of fast computers and cheap storage, event sourcing should be the default approach for any line-of-business application. This chapter explores why EventCore is the right choice for your next project and addresses common concerns.

## Why Event Sourcing Should Be Your Default

Traditional CRUD databases were designed in an era of expensive storage and slow computers. They optimize for storage efficiency by throwing away history - a terrible trade-off in today's world. Here's why event sourcing, and specifically EventCore, should be your default choice:

### 1. **History is Free**

Storage costs have plummeted. The complete history of your business operations costs pennies to store but provides immense value:

- Debug production issues by replaying events
- Satisfy any future audit requirement
- Build new features on historical data
- Prove compliance retroactively

### 2. **CRUD Lies About Your Business**

CRUD operations (Create, Read, Update, Delete) are technical concepts that don't match business reality:

- "Update" erases the reason for change
- "Delete" pretends things never existed
- State-based models lose critical business context

Event sourcing captures what actually happened: "CustomerChangedAddress", "OrderCancelled", "PriceAdjusted"

### 3. **Future-Proof by Default**

With EventCore, you never have to say "we didn't track that":

- New reporting requirements? Replay events into new projections
- Need to add analytics? The data is already there
- Compliance rules changed? Full history available

## EventCore Makes Event Sourcing Practical

While event sourcing should be the default, EventCore specifically excels by solving traditional event sourcing pain points:

### 1. Complex Business Transactions

**Problem**: Your business operations span multiple entities that must change together.

**Example**: E-commerce order fulfillment

```rust
#[derive(Command, Clone)]
struct FulfillOrder {
    #[stream]
    order: StreamId,         // Update order status
    #[stream]
    inventory: StreamId,     // Deduct items
    #[stream]
    shipping: StreamId,      // Create shipping record
    #[stream]
    customer: StreamId,      // Update loyalty points
}
```

**Why EventCore**: Traditional systems require distributed transactions or eventual consistency. EventCore makes this atomic and simple.

### 2. Financial Systems

**Problem**: Need complete audit trail and strong consistency for money movements.

**Example**: Payment processing

```rust
#[derive(Command, Clone)]
struct ProcessPayment {
    #[stream]
    customer_account: StreamId,
    #[stream]
    merchant_account: StreamId,
    #[stream]
    payment_gateway: StreamId,
    #[stream]
    tax_authority: StreamId,
}
```

**Why EventCore**:

- Every state change is recorded
- Natural audit log for compliance
- Atomic operations prevent partial payments
- Easy to replay for reconciliation

### 3. Collaborative Systems

**Problem**: Multiple users modifying shared resources with conflict resolution needs.

**Example**: Project management tool

```rust
#[derive(Command, Clone)]
struct MoveTaskToColumn {
    #[stream]
    task: StreamId,
    #[stream]
    from_column: StreamId,
    #[stream]
    to_column: StreamId,
    #[stream]
    project: StreamId,
}
```

**Why EventCore**:

- Event streams enable real-time updates
- Natural conflict resolution through events
- Complete history of who did what when

### 4. Regulatory Compliance

**Problem**: Regulations require you to show complete history of data changes.

**Example**: Healthcare records

```rust
#[derive(Command, Clone)]
struct UpdatePatientRecord {
    #[stream]
    patient: StreamId,
    #[stream]
    physician: StreamId,
    #[stream]
    audit_log: StreamId,
}
```

**Why EventCore**:

- Immutable event log satisfies auditors
- Can prove system state at any point in time
- Natural GDPR compliance (event-level data retention)

### 5. Domain-Driven Design

**Problem**: Your domain has complex rules that span multiple aggregates.

**Example**: Insurance claim processing

```rust
#[derive(Command, Clone)]
struct ProcessClaim {
    #[stream]
    claim: StreamId,
    #[stream]
    policy: StreamId,
    #[stream]
    customer: StreamId,
    #[stream]
    adjuster: StreamId,
}
```

**Why EventCore**:

- Commands match business operations exactly
- No artificial aggregate boundaries
- Domain events become first-class citizens

## Addressing Common Concerns

### "But Event Sourcing is Complex!"

**Myth**: Event sourcing adds unnecessary complexity.

**Reality**: EventCore makes it simpler than CRUD:

- No O/R mapping impedance mismatch
- Commands map directly to business operations
- No "load-modify-save" race conditions
- Debugging is easier with full history

### "What About Performance?"

**Myth**: Event sourcing is slow because it stores everything.

**Reality**:

- EventCore achieves ~83 ops/sec with PostgreSQL - plenty for most business applications
- Read models can be optimized for any query pattern
- No complex joins needed - data is pre-projected
- Scales horizontally by splitting streams

### "Storage Costs Will Explode!"

**Myth**: Storing all events is expensive.

**Reality**: Let's do the math:

- Average event size: ~1KB
- 1000 events/day = 365K events/year = 365MB/year
- S3 storage cost: ~$0.023/GB/month = $0.10/year
- **Your complete business history costs less than a coffee**

### "What About GDPR/Privacy?"

**Myth**: You can't delete data with event sourcing.

**Reality**: EventCore provides better privacy controls:

- Crypto-shredding: Delete encryption keys to make data unreadable
- Event-level retention policies
- Selective projection rebuilding
- Actually know what data you have about someone

## Special Considerations

### Large Binary Data

For systems with large binary data (images, videos), use a hybrid approach:

- Store metadata and operations as events
- Store binaries in object storage (S3)
- Best of both worlds

### Graph-Heavy Queries

For social networks or recommendation engines:

- Use EventCore for the write side
- Project into graph databases for queries
- Maintain consistency through event streams

### Cache-Like Workloads

For session storage or caching:

- These aren't business operations
- Use appropriate tools (Redis)
- EventCore for business logic, Redis for caching

## Migration Considerations

### From Traditional Database

**Good fit if**:

- You need better audit trails
- Business rules span multiple tables
- You're already using event-driven architecture

**Poor fit if**:

- Current solution works well
- No complex business rules
- Just need basic CRUD

### From Microservices

**Good fit if**:

- Struggling with distributed transactions
- Need better consistency guarantees
- Want to simplify architecture

**Poor fit if**:

- True service isolation is required
- Different teams own different services
- Services use different tech stacks

## Performance Considerations

EventCore is optimized for:

- ✅ Correctness and consistency
- ✅ Complex business operations
- ✅ Audit and compliance needs

EventCore is NOT optimized for:

- ❌ Maximum throughput (~83 ops/sec with PostgreSQL)
- ❌ Minimum latency (ms-level operations)
- ❌ Large binary data

## The Right Question

Instead of asking "Do I need event sourcing?", ask:

**"Can I afford to throw away my business history?"**

In an era of:

- Regulatory scrutiny
- Data-driven decisions
- Machine learning opportunities
- Debugging production issues
- Changing business requirements

The answer is almost always **NO**.

## Decision Framework

### Start with EventCore for:

- ✅ **Any line-of-business application** - Your default choice
- ✅ **Multi-entity operations** - EventCore's sweet spot
- ✅ **Financial systems** - Audit trail included
- ✅ **Collaborative tools** - Natural conflict resolution
- ✅ **Regulated industries** - Compliance built-in
- ✅ **Domain-driven design** - Commands match your domain

### Consider Alternatives Only For:

- 🤔 **Pure caching layers** - Use Redis alongside EventCore
- 🤔 **Binary blob storage** - Hybrid approach with S3
- 🤔 **>1000 ops/sec** - Add caching or consider specialized solutions

## Summary

In 2024 and beyond, the question isn't "Why event sourcing?" but "Why would you throw away your business history?"

EventCore makes event sourcing practical by:

- Eliminating aggregate boundary problems
- Providing multi-stream atomicity
- Making it type-safe and simple
- Scaling to real business needs

**Storage is cheap. History is valuable. Make event sourcing your default.**

Ready to dive deeper? Let's explore [Event Modeling Fundamentals](./03-event-modeling.md) →
