# ADR-014: Queue-Based Stream Discovery Execution

## Status

accepted

## Supersedes

- ADR-009: Stream Resolver Design for Dynamic Discovery

## Context

ADR-009 introduced the StreamResolver trait so commands could request additional streams at runtime. That decision modeled discovery as a multi-pass protocol: the executor read all statically declared streams, invoked the resolver, and—if new streams were returned—restarted the read phase. While correct, the explicit re-execution phase complicated the executor’s control flow and encouraged redundant reads of streams that had already been loaded (especially painful for long histories).

As we implemented FR-1.3 (Dynamic Stream Discovery) in the executor, two additional forces surfaced:

1. **Unnecessary Re-Reads**: Restarting the read phase caused the executor to re-read static streams even when only previously unseen streams were discovered.
2. **State Drift**: Tracking partial state between passes added branching complexity to `execute()`, making it harder to reason about optimistic concurrency boundaries.
3. **Queue Semantics Already Present**: ADR-008’s retry policy already walks a set of streams and expected versions. Modeling discovery as a queue/visited set aligns with that mental model.
4. **Deterministic Ordering**: A FIFO queue provides a deterministic traversal order for diagnostics and testing.

To address these forces we are adopting a single-pass queue protocol that still honors ADR-009’s goals but avoids the re-execution overhead.

## Decision

1. **StreamResolver<State> Trait (Unchanged API Surface)**

   Commands continue to opt into dynamic discovery by implementing:

   ```rust
   pub trait StreamResolver<State> {
       fn discover_related_streams(&self, state: &State) -> Vec<StreamId>;
   }
   ```

   - The trait remains optional; most commands rely solely on static `#[stream]` declarations.
   - Resolvers SHOULD avoid duplicates, but the executor deduplicates defensively.

2. **CommandLogic::stream_resolver() Hook**

   `CommandLogic` now exposes an overridable `fn stream_resolver(&self) -> Option<&dyn StreamResolver<Self::State>>` so commands can return `Some(self)` (or a helper) without relying on downcasting. Commands that do nothing return `None`.

3. **Queue-Based Executor Flow**

   Instead of a re-execution loop, `execute()` maintains three collections for the duration of a single attempt:
   - `VecDeque<StreamId>` queue seeded with statically declared streams
   - `HashSet<StreamId>` named `scheduled` to deduplicate enqueued IDs
   - `HashSet<StreamId>` named `visited` to guarantee each stream is read once per attempt

   Pseudocode:

   ```rust
   let resolver = command.stream_resolver();
   let mut queue = VecDeque::from(static_streams);
   let mut scheduled = HashSet::from(static_streams);
   let mut visited = HashSet::new();

   while let Some(stream_id) = queue.pop_front() {
       if !visited.insert(stream_id.clone()) {
           continue;
       }

       let reader = store.read_stream(stream_id.clone()).await?;
       fold_events_into_state(&mut state, reader);
       expected_versions.insert(stream_id.clone(), reader.current_version());

       if let Some(resolver) = resolver {
           for related in resolver.discover_related_streams(&state) {
               if scheduled.insert(related.clone()) {
                   queue.push_back(related);
               }
           }
       }
   }
   ```

   Every stream—static or discovered—is read exactly once per attempt. Newly discovered streams join the queue immediately, so “multi-pass” discovery naturally occurs as additional IDs arrive.

4. **Atomicity and Optimistic Concurrency**

   The executor captures the current version of every visited stream and includes those versions in the write barrier. Discovered streams therefore participate fully in ADR-001 (atomicity) and ADR-007 (optimistic concurrency) guarantees without extra bookkeeping.

5. **Error Handling and Retries**
   - Discovery remains deterministic and pure: `discover_related_streams` inspects state, returns IDs, and never mutates infrastructure.
   - Discovery errors are permanent (no retry) per ADR-004; version conflicts trigger the retry policy from ADR-008, which reruns the entire queue against fresh state.

## Rationale

- **Type-First**: StreamResolver stays generic over `State`, so commands operate on rich domain types rather than infrastructure primitives.
- **Readability**: A single queue loop is easier to reason about than explicitly rewinding to “Phase 2” multiple times.
- **Performance**: Avoiding redundant reads dramatically reduces I/O for large streams while still providing complete history for newly discovered streams.
- **Determinism**: FIFO ordering coupled with a visited set produces predictable logs and simplifies testing (see `tests/I-007-dynamic_stream_discovery_test.rs`).
- **Retry Safety**: Because retries reconstruct the queue from scratch, discovery uses the latest state and avoids stale stream sets.

## Consequences

### Positive

- Each stream is read at most once per execution attempt, minimizing storage load.
- State reconstruction is monotonic: every iteration folds more events into the same `state` value.
- Discovery logic lives entirely inside domain commands; infrastructure remains agnostic of business-specific IDs.
- Integration tests can assert per-stream read counts and optimistic-concurrency behavior without mocks.
- Documentation can describe a single-pass workflow instead of multi-phase re-execution.

### Negative

- The queue adds more data structures (VecDeque + two HashSets) to the executor, increasing heap allocations slightly.
- Discovery bugs that emit unbounded unique IDs will still enqueue indefinitely; developers must test resolvers thoroughly.
- Instrumentation must be careful to log both scheduled and visited sets for observability.

## Supersession Notes

- ADR-009 still documents the motivations for introducing StreamResolver and the guarantees it must uphold.
- ADR-014 replaces the executor orchestration portion of ADR-009 with the queue/visited design outlined above.
- Future changes to discovery should build on this ADR unless they fundamentally alter the queue semantics, in which case a subsequent ADR must supersede ADR-014 explicitly.
