extern crate eventcore;

use eventcore::{ModelEvent, ModelInput, mapping};

#[derive(ModelEvent)]
enum Event {
    Opened,
    Deposited(u64),
}

#[derive(ModelInput)]
struct Input {
    #[model(origin)]
    amount: u64,
}

mapping! { DepositAmount: Input.amount => Event.Deposited using clone; }

fn main() {
    let opened = Event::model_variant_opened();
    let deposited = Event::model_variant_deposited(
        eventcore::model::FieldValue::from_value(10_u64),
    );
    assert!(matches!(opened.into_inner(), Event::Opened));
    assert!(matches!(deposited.into_inner(), Event::Deposited(10)));

    let input = Input::model_builder().amount(12).build();
    let mapped = Event::model_variant_deposited(DepositAmount::apply(input.as_ref()));
    assert!(matches!(mapped.into_inner(), Event::Deposited(12)));
}
