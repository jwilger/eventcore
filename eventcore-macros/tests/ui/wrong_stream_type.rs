// trybuild compile-fail fixture: #[stream] must target StreamId, not domain wrappers.
// Exercised via tests/trybuild.rs.
use eventcore::Command;

struct MoneyAmount(u64);

#[derive(Command)]
struct WrongStreamType {
    #[stream]
    amount: MoneyAmount,
}

fn main() {}
