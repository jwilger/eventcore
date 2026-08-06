use eventcore::ModelInput;

#[derive(ModelInput)]
struct GenericRequest<T> {
    #[model(origin)]
    value: T,
}

fn main() {}
