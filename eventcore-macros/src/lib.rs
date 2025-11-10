use proc_macro::TokenStream;

#[proc_macro_derive(Command, attributes(stream))]
pub fn command(_input: TokenStream) -> TokenStream {
    <TokenStream as std::str::FromStr>::from_str(
        "compile_error!(\"#[derive(Command)] is not implemented yet\");",
    )
    .expect("failed to emit compile_error for #[derive(Command)]")
}
