// trybuild compile-fail fixture: missing #[stream] should produce a helpful error.
// Run via tests/trybuild.rs; this source is intentionally "broken" outside that harness.
use eventcore::{Command, StreamId};

#[derive(Command)]
struct MissingStreamAttribute {
    account_id: StreamId,
}

fn main() {}
