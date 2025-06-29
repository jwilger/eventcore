# EventCore Macros

Procedural macros for the EventCore event sourcing library.

## Overview

This crate provides derive macros and attribute macros to reduce boilerplate when implementing commands and other EventCore patterns.

## Features

### `#[derive(Command)]`

The `Command` derive macro helps implement the EventCore `Command` trait by:

- Generating a type-safe `StreamSet` phantom type
- Implementing the `read_streams` method based on `#[stream]` field attributes
- Providing clear guidance on what methods still need manual implementation

#### Example

```rust
use eventcore_macros::Command;
use eventcore::types::StreamId;

#[derive(Command)]
struct TransferMoney {
    #[stream]
    from_account: StreamId,
    #[stream]
    to_account: StreamId,
    amount: Money,
}

// The macro generates:
// - TransferMoneyStreamSet phantom type
// - Implementation of read_streams() that returns [from_account, to_account]
//
// You still need to implement:
// - type Input = YourInputType
// - type State = YourStateType  
// - type Event = YourEventType
// - fn apply()
// - async fn handle()
```

### Future Macros

Additional macros are planned for future releases:

- `command!` declarative macro for simpler command definitions
- `#[stream]` attribute enhancements
- Helper macros for common patterns

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](../LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](../LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.

## Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.