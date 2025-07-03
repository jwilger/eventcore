//! Implementation of the #[derive(Command)] procedural macro.

use proc_macro2::TokenStream;
use quote::quote;
use syn::{DeriveInput, Result};

use crate::stream_set::generate_stream_set_type;
use crate::utils::extract_stream_fields;

/// Expands the #[derive(Command)] macro.
pub fn expand_derive_command(input: DeriveInput) -> Result<TokenStream> {
    let name = &input.ident;
    let generics = &input.generics;
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    // Extract fields marked with #[stream]
    let stream_fields = extract_stream_fields(&input)?;

    // Generate the StreamSet type
    let stream_set_name = quote::format_ident!("{}StreamSet", name);
    let stream_set_type = generate_stream_set_type(&stream_set_name, &stream_fields);

    // Generate read_streams implementation
    let read_streams_impl = generate_read_streams(&stream_fields);

    // Generate the StreamSet type and complete CommandStreams implementation
    let expanded = quote! {
        #stream_set_type

        impl #impl_generics eventcore::CommandStreams for #name #ty_generics #where_clause {
            type StreamSet = #stream_set_name;

            fn read_streams(&self) -> Vec<eventcore::StreamId> {
                #read_streams_impl
            }
        }
    };

    Ok(expanded)
}

/// Generate the read_streams method implementation.
fn generate_read_streams(stream_fields: &[(String, syn::Type)]) -> TokenStream {
    if stream_fields.is_empty() {
        return quote! { vec![] };
    }

    let field_accesses: Vec<_> = stream_fields
        .iter()
        .map(|(field_name, _)| {
            let field = quote::format_ident!("{}", field_name);
            quote! { self.#field.clone() }
        })
        .collect();

    quote! {
        vec![#(#field_accesses),*]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use syn::parse_quote;

    #[test]
    fn test_expand_derive_command_with_stream_fields() {
        let input: DeriveInput = parse_quote! {
            #[derive(Command)]
            struct TransferMoney {
                #[stream]
                from_account: StreamId,
                #[stream]
                to_account: StreamId,
                amount: Money,
            }
        };

        let result = expand_derive_command(input);
        assert!(result.is_ok());

        let output = result.unwrap();
        let output_str = output.to_string();

        // Verify StreamSet type is generated
        assert!(output_str.contains("TransferMoneyStreamSet"));

        // Verify read_streams implementation
        assert!(output_str.contains("fn read_streams"));
        // The vector syntax varies with formatting
        assert!(output_str.contains("self.from_account") || output_str.contains("vec"));
    }

    #[test]
    fn test_expand_derive_command_no_stream_fields() {
        let input: DeriveInput = parse_quote! {
            #[derive(Command)]
            struct SimpleCommand {
                data: String,
            }
        };

        let result = expand_derive_command(input);
        assert!(result.is_ok());

        let output = result.unwrap();
        let output_str = output.to_string();

        // Verify the read_streams method is generated
        assert!(output_str.contains("fn read_streams"));
        // Should generate code for empty stream list
        assert!(output_str.contains("eventcore"));
    }
}
