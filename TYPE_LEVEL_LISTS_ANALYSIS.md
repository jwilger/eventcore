# Type-Level Lists for Stream Access Control - Feasibility Analysis

## Overview

Type-level lists in Rust allow encoding list data structures at the type level using recursive type definitions. This enables compile-time validation and manipulation of collections without runtime overhead.

## Current Type-Level List Approaches in Rust

### 1. HList (Heterogeneous List) Pattern

```rust
// Basic HList definition
pub struct HNil;
pub struct HCons<H, T>(H, T);

// Type alias for convenience
type HList!(H, $($rest:ty),*) = HCons<H, HList!($($rest),*)>;
type HList!() = HNil;

// Example usage
type StreamList = HList!(AccountStream, TransferLogStream, ProductStream);
```

### 2. Peano Number Indexed Lists

```rust
// Define natural numbers at type level
pub struct Z; // Zero
pub struct S<N>(PhantomData<N>); // Successor

// Type-level list with length tracking
pub struct TList<T, N> {
    _marker: PhantomData<(T, N)>,
}

// Example: List of 3 stream types
type ThreeStreams = TList<(AccountStream, TransferLogStream, ProductStream), S<S<S<Z>>>>;
```

### 3. Const Generic Array-Based Lists

```rust
// Using const generics for fixed-size type lists
pub struct TypeList<const N: usize, T: 'static> {
    types: [&'static str; N], // Type names
    _marker: PhantomData<T>,
}

// Macro to create type lists
macro_rules! type_list {
    ($($ty:ty),*) => {
        TypeList<{count_types!($($ty),*)}, ($($ty),*)> {
            types: [$(stringify!($ty)),*],
            _marker: PhantomData,
        }
    };
}
```

## Application to EventCore Stream Access Control

### 1. HList-Based Stream Sets

```rust
use std::marker::PhantomData;

// Define stream types as zero-sized markers
pub struct AccountStream<const ID: &'static str>;
pub struct TransferLogStream;
pub struct ProductStream<const ID: &'static str>;

// HList implementation for type-level lists
pub struct HNil;
pub struct HCons<Head, Tail> {
    _phantom: PhantomData<(Head, Tail)>,
}

// Type-level operations on HLists
pub trait Contains<T> {
    const CONTAINS: bool;
}

impl<T> Contains<T> for HNil {
    const CONTAINS: bool = false;
}

impl<Head, Tail, T> Contains<T> for HCons<Head, Tail>
where
    Tail: Contains<T>,
{
    const CONTAINS: bool = false || Tail::CONTAINS;
}

impl<Head, Tail> Contains<Head> for HCons<Head, Tail>
where
    Tail: Contains<Head>,
{
    const CONTAINS: bool = true;
}

// Command with type-level stream list
impl Command for TransferCommand {
    type StreamSet = HCons<
        AccountStream<"source">,
        HCons<
            AccountStream<"target">,
            HCons<TransferLogStream, HNil>
        >
    >;
    
    async fn handle(
        &self,
        read_streams: ReadStreams<Self::StreamSet>,
        state: Self::State,
        input: Self::Input,
        _stream_resolver: &mut StreamResolver,
    ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
        // Compile-time checked stream access
        let source_write = read_streams.write_to::<AccountStream<"source">>(
            input.source_stream_id(),
            event
        )?; // Compiles only if AccountStream<"source"> is in StreamSet
        
        Ok(vec![source_write])
    }
}
```

### 2. Type-Level Stream Set Operations

```rust
// Trait for type-level list operations
pub trait StreamSetOps {
    type Length: Nat; // Natural number representing length
    
    fn stream_count() -> usize;
    fn contains_stream<S>() -> bool where Self: Contains<S>;
}

// Implementation for HNil
impl StreamSetOps for HNil {
    type Length = Z;
    
    fn stream_count() -> usize { 0 }
    fn contains_stream<S>() -> bool { false }
}

// Implementation for HCons
impl<Head, Tail> StreamSetOps for HCons<Head, Tail>
where
    Tail: StreamSetOps,
    Tail::Length: Add<S<Z>>,
{
    type Length = <Tail::Length as Add<S<Z>>>::Output;
    
    fn stream_count() -> usize { 1 + Tail::stream_count() }
    
    fn contains_stream<S>() -> bool 
    where 
        Self: Contains<S>
    {
        Self::CONTAINS
    }
}

// Compile-time stream access validation
impl<StreamSet, E> StreamWrite<StreamSet, E>
where
    StreamSet: StreamSetOps,
{
    pub fn new_typed<S>(
        read_streams: &ReadStreams<StreamSet>,
        stream_id: StreamId,
        event: E,
    ) -> Self
    where
        StreamSet: Contains<S>,
        [(); StreamSet::CONTAINS as usize]: // Compile-time assertion
    {
        Self {
            stream_id,
            event,
            _phantom: PhantomData,
        }
    }
}
```

### 3. Macro-Based Type-Level Stream Declaration

```rust
// Macro to simplify stream set creation
macro_rules! stream_set {
    () => { HNil };
    ($head:ty) => { HCons<$head, HNil> };
    ($head:ty, $($tail:ty),+) => { HCons<$head, stream_set!($($tail),+)> };
}

// Usage in commands
impl Command for TransferCommand {
    type StreamSet = stream_set!(
        AccountStream<"source">,
        AccountStream<"target">,
        TransferLogStream
    );
}

// Macro for type-safe stream writes
macro_rules! write_to_stream {
    ($read_streams:expr, $stream_type:ty, $stream_id:expr, $event:expr) => {
        $read_streams.write_to::<$stream_type>($stream_id, $event)
    };
}

// Usage in command handlers
let writes = vec![
    write_to_stream!(read_streams, AccountStream<"source">, source_id, debit_event),
    write_to_stream!(read_streams, AccountStream<"target">, target_id, credit_event),
    write_to_stream!(read_streams, TransferLogStream, log_id, log_event),
];
```

### 4. Dynamic Stream Discovery with Type-Level Lists

```rust
// Type-safe stream set extension
pub trait ExtendStreamSet<Extension> {
    type Extended;
    
    fn extend(self, extension: Extension) -> Self::Extended;
}

impl<Extension> ExtendStreamSet<Extension> for HNil {
    type Extended = Extension;
    
    fn extend(self, extension: Extension) -> Self::Extended {
        extension
    }
}

impl<Head, Tail, Extension> ExtendStreamSet<Extension> for HCons<Head, Tail>
where
    Tail: ExtendStreamSet<Extension>,
{
    type Extended = HCons<Head, Tail::Extended>;
    
    fn extend(self, extension: Extension) -> Self::Extended {
        HCons {
            _phantom: PhantomData,
        }
    }
}

// Dynamic discovery with type safety
pub struct StreamResolver<CurrentStreams> {
    current_streams: CurrentStreams,
}

impl<CurrentStreams> StreamResolver<CurrentStreams> {
    pub fn add_streams<NewStreams>(
        self,
        new_streams: NewStreams,
    ) -> StreamResolver<CurrentStreams::Extended>
    where
        CurrentStreams: ExtendStreamSet<NewStreams>,
    {
        StreamResolver {
            current_streams: self.current_streams.extend(new_streams),
        }
    }
}
```

## Feasibility Assessment

### Advantages

#### 1. Compile-Time Safety
```rust
// Impossible to compile if stream not in set
let invalid_write = read_streams.write_to::<UnknownStream>(id, event);
// ^^^^ Compiler error: UnknownStream not in StreamSet
```

#### 2. Zero Runtime Cost
```rust
// All type checking happens at compile time
// Runtime code is just direct field access
impl<StreamSet> ReadStreams<StreamSet> {
    pub fn write_to<S>(&self, stream_id: StreamId, event: E) -> StreamWrite<StreamSet, E>
    where
        StreamSet: Contains<S>,
        [(); StreamSet::CONTAINS as usize]:
    {
        // No runtime validation needed
        StreamWrite::new_unchecked(stream_id, event)
    }
}
```

#### 3. Rich Type-Level Operations
```rust
// Type-level stream set operations
type BaseStreams = stream_set!(AccountStream<"main">, TransferLogStream);
type ExtendedStreams = stream_set_append!(BaseStreams, ProductStream<"p1">);
type MergedStreams = stream_set_merge!(ExtendedStreams, AuditStreams);
```

### Challenges

#### 1. Complex Type Signatures
```rust
// Type signatures become very verbose
impl Command for ComplexCommand {
    type StreamSet = HCons<
        AccountStream<"source">,
        HCons<
            AccountStream<"target">,
            HCons<
                ProductStream<"product1">,
                HCons<
                    ProductStream<"product2">,
                    HCons<TransferLogStream, HNil>
                >
            >
        >
    >;
}
```

#### 2. Compile-Time Performance
```rust
// Large type-level lists can impact compile times
// Each operation requires recursive trait resolution
type VeryLargeStreamSet = HCons<Stream1, HCons<Stream2, /* ... 100 more streams ... */>>;
```

#### 3. Error Message Quality
```rust
// Type errors can be cryptic
error[E0277]: the trait bound `HCons<AccountStream<"source">, HNil>: Contains<UnknownStream>` is not satisfied
  --> src/lib.rs:42:5
   |
42 |     read_streams.write_to::<UnknownStream>(id, event);
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
```

#### 4. Dynamic Discovery Complexity
```rust
// Type-safe dynamic discovery requires complex trait machinery
pub trait DynamicStreamDiscovery<Initial, Discovered> {
    type Result;
    type DiscoveryState: StreamDiscoveryState;
    
    fn discover(
        initial: Initial,
        discovery_fn: impl FnOnce(&mut Self::DiscoveryState) -> Discovered
    ) -> Self::Result;
}
```

## Comparison with Alternatives

### Type-Level Lists vs Const Generics

| Aspect | Type-Level Lists | Const Generics |
|--------|------------------|----------------|
| **Type Safety** | Excellent - full type system | Good - const validation |
| **Performance** | Zero runtime cost | Zero runtime cost |
| **Compile Time** | Can be slow for large lists | Generally faster |
| **Error Messages** | Often cryptic | Usually clearer |
| **API Complexity** | High - requires traits | Medium - familiar syntax |
| **Dynamic Discovery** | Complex but type-safe | Simpler but less safe |

### Type-Level Lists vs Runtime Validation

| Aspect | Type-Level Lists | Runtime Validation |
|--------|------------------|-------------------|
| **Error Detection** | Compile time | Runtime |
| **Performance** | Zero cost | Hash map lookups |
| **Flexibility** | Statically determined | Fully dynamic |
| **Debugging** | Compile-time errors | Runtime debugging |
| **Learning Curve** | Steep | Gentle |

## Recommended Approach

### Hybrid Strategy

1. **Use type-level lists for static stream sets**
   - Commands with known, fixed stream dependencies
   - Core library components where type safety is critical

2. **Use const generics for simple cases**
   - Commands with few streams (1-3)
   - Where compile-time performance is important

3. **Keep runtime validation for dynamic discovery**
   - Commands with complex dynamic stream requirements
   - Backwards compatibility scenarios

### Implementation Plan

#### Phase 1: Proof of Concept
```rust
// Create basic HList implementation
// Implement Contains trait for stream validation
// Build macro support for ergonomic usage
```

#### Phase 2: Core Integration
```rust
// Integrate with existing StreamWrite API
// Add type-level validation alongside runtime checks
// Create migration utilities
```

#### Phase 3: Advanced Features
```rust
// Implement stream set operations (union, intersection)
// Add support for dynamic discovery with type safety
// Optimize compile-time performance
```

#### Phase 4: Production Hardening
```rust
// Comprehensive testing and benchmarking
// Error message improvements
// Documentation and examples
```

## Conclusion

Type-level lists offer powerful compile-time safety for EventCore's stream access control, but with significant complexity trade-offs:

### When to Use Type-Level Lists:
- **Critical safety requirements** - Financial systems, medical devices
- **Static stream sets** - Commands with fixed, known dependencies  
- **Performance-critical paths** - Zero runtime validation cost essential
- **Expert development teams** - Can handle complex type signatures

### When to Avoid:
- **Rapid prototyping** - Type complexity slows development
- **Dynamic heavy workloads** - Complex dynamic discovery requirements
- **Large stream sets** - Compile-time performance issues
- **Junior developer teams** - High learning curve

### Recommendation:
Implement as an **optional advanced feature** alongside existing runtime validation. Provide migration path and clear documentation for when each approach is appropriate. This aligns with EventCore's philosophy of providing powerful tools while maintaining usability.