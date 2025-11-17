use eventcore_macros::Command;

struct MoneyAmount(u64);

#[derive(Command)]
struct WrongStreamType {
    #[stream]
    amount: MoneyAmount,
}

fn main() {}
