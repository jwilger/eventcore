# Sagas Example

The saga example implements distributed transaction patterns using EventCore's multi-stream capabilities.

## What are Sagas?

Sagas are a pattern for managing long-running business processes that span multiple bounded contexts or services. EventCore makes implementing sagas straightforward with its multi-stream atomicity.

## Example Scenario

This example implements a travel booking saga that coordinates:

- Flight reservation
- Hotel booking
- Car rental
- Payment processing

Each step can fail, triggering compensating actions to maintain consistency.

## Running the Example

```bash
cargo run --example sagas
```

## Implementation Details

- **Orchestration**: Central saga coordinator manages the workflow
- **Compensation**: Automatic rollback on failures
- **Idempotency**: Safe retries with exactly-once semantics
- **Monitoring**: Built-in observability for saga progress

[View Source Code](https://github.com/jwilger/eventcore/tree/main/eventcore-examples/src/sagas)
