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

#[derive(Debug)]
struct InvalidAmount;

fn validate(amount: &u64) -> Result<u64, InvalidAmount> {
    (*amount > 0).then_some(*amount).ok_or(InvalidAmount)
}

mapping! {
    ValidatedAmount:
        Request.amount => Transfer.amount
        using try validate, error = InvalidAmount;
}

fn main() {
    let request = Request::model_builder().amount(1).build();
    let _amount = ValidatedAmount::apply(request.as_ref()).expect("valid amount");
}
