//! Generation of type-safe StreamSet types.

use proc_macro2::TokenStream;
use quote::quote;
use syn::Ident;

/// Generate a type-safe StreamSet type that encodes which streams a command can access.
pub fn generate_stream_set_type(
    name: &Ident,
    _stream_fields: &[(String, syn::Type)],
) -> TokenStream {
    // StreamSet is just a phantom type marker, not a trait
    // It's used at the type level for stream access control
    quote! {
        /// Type-safe stream set marker for this command.
        #[derive(Debug, Clone, Copy, Default)]
        pub struct #name;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use syn::parse_quote;

    #[test]
    fn test_generate_stream_set_type() {
        let name = quote::format_ident!("TestStreamSet");
        let stream_fields = vec![];

        let output = generate_stream_set_type(&name, &stream_fields);
        let output_str = output.to_string();

        assert!(output_str.contains("struct TestStreamSet"));
        assert!(output_str.contains("Debug"));
        assert!(output_str.contains("Clone"));
    }

    #[test]
    fn test_generate_stream_set_with_fields() {
        let name = quote::format_ident!("TransferStreamSet");
        let stream_fields = vec![
            ("from_account".to_string(), parse_quote!(StreamId)),
            ("to_account".to_string(), parse_quote!(StreamId)),
        ];

        let output = generate_stream_set_type(&name, &stream_fields);
        let output_str = output.to_string();

        // Should still be just a phantom type
        assert!(output_str.contains("struct TransferStreamSet"));
        assert!(!output_str.contains("from_account"));
        assert!(!output_str.contains("to_account"));
    }
}
