# Const Generics for Compile-Time Stream Set Validation

## Current Implementation Analysis

The current stream access control system uses:

1. **Runtime validation** in `StreamWrite::new()` that checks if a stream was declared
2. **Phantom types** (`StreamSet`) that don't provide compile-time guarantees
3. **Dynamic stream discovery** that requires runtime coordination

### Issues with Current Approach

1. **Runtime errors** for stream access violations that could be caught at compile time
2. **No compile-time guarantee** that all declared streams are actually used
3. **Complex runtime coordination** for dynamic stream discovery
4. **Type erasure** - `StreamSet = ()` provides no type-level information

## Proposed Const Generics Improvements

### 1. Type-Level Stream Sets Using Const Generics

```rust
// Define a compile-time stream set using const generics and type-level programming
pub struct StreamSet<const STREAMS: &'static [&'static str]>;

// Example usage in commands
impl Command for TransferMoneyCommand {
    type StreamSet = StreamSet<&["account-source", "account-target", "transfers"]>;
    
    fn read_streams(&self, input: &Self::Input) -> Vec<StreamId> {
        // Compiler enforces that returned streams match the const generic
        vec![
            self.source_stream(input),
            self.target_stream(input), 
            self.transfer_log_stream(),
        ]
    }
}
```

### 2. Compile-Time Stream Access Validation

```rust
// StreamWrite becomes compile-time validated
impl<const STREAMS: &'static [&'static str], E> StreamWrite<StreamSet<STREAMS>, E> {
    pub fn new_checked<const STREAM: &'static str>(
        read_streams: &ReadStreams<StreamSet<STREAMS>>,
        stream_id: StreamId,
        event: E,
    ) -> Self 
    where
        [(); contains_stream::<STREAMS, STREAM>()]: // Compile-time assertion
    {
        Self {
            stream_id,
            event,
            _phantom: PhantomData,
        }
    }
}

// Compile-time function to check stream membership
const fn contains_stream<const STREAMS: &'static [&'static str], const STREAM: &'static str>() -> usize {
    let mut i = 0;
    while i < STREAMS.len() {
        if str_eq(STREAMS[i], STREAM) {
            return 1; // Found
        }
        i += 1;
    }
    panic!("Stream not declared in StreamSet"); // Compile-time error
}

const fn str_eq(a: &str, b: &str) -> bool {
    if a.len() != b.len() { return false; }
    let a_bytes = a.as_bytes();
    let b_bytes = b.as_bytes();
    let mut i = 0;
    while i < a.len() {
        if a_bytes[i] != b_bytes[i] { return false; }
        i += 1;
    }
    true
}
```

### 3. Typed Stream Identifiers

```rust
// Define stream types at the type level
pub struct AccountStream<const ACCOUNT_ID: &'static str>;
pub struct TransferLogStream;

// Commands declare their stream dependencies explicitly
impl Command for TransferMoneyCommand {
    type StreamSet = (
        AccountStream<"source">, 
        AccountStream<"target">, 
        TransferLogStream
    );
    
    async fn handle(
        &self,
        read_streams: ReadStreams<Self::StreamSet>,
        state: Self::State,
        input: Self::Input,
        _stream_resolver: &mut StreamResolver,
    ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
        // Type-safe stream access
        let source_write = read_streams.write_to::<AccountStream<"source">>(
            input.source_stream_id(),
            DebitEvent { amount: input.amount }
        );
        
        let target_write = read_streams.write_to::<AccountStream<"target">>(
            input.target_stream_id(),
            CreditEvent { amount: input.amount }
        );
        
        let log_write = read_streams.write_to::<TransferLogStream>(
            StreamId::transfers(),
            TransferLoggedEvent { transfer_id: input.transfer_id }
        );
        
        Ok(vec![source_write, target_write, log_write])
    }
}
```

### 4. Enhanced Dynamic Stream Discovery with Type Safety

```rust
// Type-safe dynamic stream addition
pub struct StreamResolver<S> {
    current_streams: S,
    additional_streams: Vec<Box<dyn Any>>, // Type-erased additional streams
}

impl<S> StreamResolver<S> {
    pub fn add_streams<T>(&mut self, streams: T) 
    where 
        T: StreamSetExtension<S>,
    {
        self.additional_streams.push(Box::new(streams));
    }
}

// Trait for extending stream sets safely
pub trait StreamSetExtension<Base> {
    type Extended;
    fn extend(base: Base, extension: Self) -> Self::Extended;
}

// Example: Dynamically adding product streams to an order command
impl StreamSetExtension<OrderStreamSet> for ProductStreamSet<N> {
    type Extended = ExtendedOrderStreamSet<N>;
    
    fn extend(base: OrderStreamSet, extension: Self) -> Self::Extended {
        ExtendedOrderStreamSet { order: base, products: extension }
    }
}
```

### 5. Const Generic Benefits

#### Compile-Time Validation
```rust
// This would fail to compile
let invalid_write = StreamWrite::new_checked::<"non-existent-stream">(
    &read_streams,
    stream_id,
    event
); // Compiler error: stream not in StreamSet
```

#### Zero Runtime Cost
```rust
// All validation happens at compile time
// Runtime code is just direct field access
pub fn write_to_account<const ACCOUNT: &'static str>(
    &self,
    stream_id: StreamId,
    event: E
) -> StreamWrite<Self::StreamSet, E> {
    // No runtime checks needed - compiler guarantees safety
    StreamWrite {
        stream_id,
        event,
        _phantom: PhantomData,
    }
}
```

#### Better IDE Support
```rust
// IDE can auto-complete available streams based on const generics
read_streams.write_to::<|>(); // IDE shows: AccountStream<"source">, AccountStream<"target">, TransferLogStream
```

## Implementation Plan

### Phase 1: Basic Const Generic Stream Sets
1. Add const generic parameters to `StreamSet`
2. Implement compile-time stream membership checking
3. Update `StreamWrite::new()` to use compile-time validation

### Phase 2: Typed Stream Identifiers  
1. Create stream type hierarchy
2. Implement type-safe stream access methods
3. Update commands to use typed streams

### Phase 3: Enhanced Dynamic Discovery
1. Implement type-safe stream set extension
2. Add compile-time validation for dynamic additions
3. Preserve type safety through dynamic discovery cycles

### Phase 4: Performance Optimization
1. Benchmark const generic vs runtime validation
2. Optimize compile times for complex stream sets
3. Add const generic feature flags for backwards compatibility

## Migration Strategy

### Backwards Compatibility
```rust
// Legacy support using type aliases
pub type LegacyStreamSet = StreamSet<&[]>; // Empty const generic

// Commands can migrate incrementally
impl Command for LegacyCommand {
    type StreamSet = LegacyStreamSet; // Old behavior
}

impl Command for ModernCommand {
    type StreamSet = StreamSet<&["stream1", "stream2"]>; // New behavior
}
```

### Incremental Adoption
1. **Phase 1**: Add const generic support alongside existing runtime validation
2. **Phase 2**: Migrate core examples to demonstrate benefits
3. **Phase 3**: Deprecate runtime-only validation
4. **Phase 4**: Remove legacy support in 2.0

## Expected Benefits

### Compile-Time Safety
- Stream access violations caught at compile time
- Impossible to write to undeclared streams
- Type-safe dynamic stream discovery

### Performance
- Zero runtime cost for stream access validation
- Faster command execution due to eliminated runtime checks
- Better compiler optimizations

### Developer Experience
- Better IDE support and auto-completion
- Clearer API with explicit stream dependencies
- Compile-time documentation of stream usage

### Reliability
- Eliminate entire class of runtime errors
- Safer refactoring of stream-dependent code
- More predictable dynamic stream discovery

## Challenges and Limitations

### Const Generic Limitations
- Limited string operations in const contexts
- Complex const generic expressions may impact compile times
- MSRV considerations for advanced const generic features

### Dynamic Stream Discovery Complexity
- Type-safe dynamic additions require complex trait machinery
- May need to balance type safety with runtime flexibility
- Could impact ease of use for simple dynamic scenarios

### Migration Complexity
- Large existing codebase to migrate
- Need to maintain backwards compatibility
- Complex interaction with macro-generated code

## Conclusion

Const generics offer significant opportunities to improve EventCore's type safety and performance. The proposed design provides:

1. **Compile-time stream access validation**
2. **Zero-cost runtime performance**
3. **Better developer experience**
4. **Incremental migration path**

This aligns with EventCore's type-driven development philosophy and addresses the expert feedback about modernizing Rust idioms while maintaining the innovative multi-stream capabilities.