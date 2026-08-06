use eventcore::{ModelCommand, ModelInput, StreamId, mapping};

#[derive(ModelInput)]
struct Request {
    #[model(origin)]
    amount: u64,
}

#[derive(ModelCommand)]
struct Transfer {
    #[stream]
    stream: StreamId,
    amount: u64,
}

fn takes_the_wrong_type(_amount: &str) -> u64 {
    1
}

mapping! {
    InvalidTransform:
        Request.amount => Transfer.amount
        using takes_the_wrong_type;
}

fn main() {}
