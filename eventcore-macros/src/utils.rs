//! Utility functions for macro expansion.

use syn::{Attribute, Data, DeriveInput, Error, Fields, Result};

/// Extract fields marked with #[stream] attribute.
pub fn extract_stream_fields(input: &DeriveInput) -> Result<Vec<(String, syn::Type)>> {
    let fields = match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(fields) => &fields.named,
            _ => {
                return Err(Error::new_spanned(
                    input,
                    "Command derive only supports structs with named fields",
                ))
            }
        },
        _ => {
            return Err(Error::new_spanned(
                input,
                "Command derive only supports structs",
            ))
        }
    };

    let mut stream_fields = Vec::new();

    for field in fields {
        if has_stream_attribute(&field.attrs) {
            let field_name = field
                .ident
                .as_ref()
                .ok_or_else(|| Error::new_spanned(field, "Field must have a name"))?
                .to_string();
            let field_type = field.ty.clone();
            stream_fields.push((field_name, field_type));
        }
    }

    Ok(stream_fields)
}

/// Check if a field has the #[stream] attribute.
pub fn has_stream_attribute(attrs: &[Attribute]) -> bool {
    attrs.iter().any(parse_stream_attribute)
}

/// Parse a #[stream] attribute.
pub fn parse_stream_attribute(attr: &Attribute) -> bool {
    attr.path().is_ident("stream")
}

#[cfg(test)]
mod tests {
    use super::*;
    use syn::parse_quote;

    #[test]
    fn test_extract_stream_fields_from_struct() {
        let input: DeriveInput = parse_quote! {
            struct TestCommand {
                #[stream]
                account_id: StreamId,
                #[stream]
                catalog_id: StreamId,
                amount: Money,
            }
        };

        let result = extract_stream_fields(&input).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].0, "account_id");
        assert_eq!(result[1].0, "catalog_id");
    }

    #[test]
    fn test_extract_stream_fields_no_streams() {
        let input: DeriveInput = parse_quote! {
            struct SimpleCommand {
                data: String,
                value: i32,
            }
        };

        let result = extract_stream_fields(&input).unwrap();
        assert_eq!(result.len(), 0);
    }

    #[test]
    fn test_parse_stream_attribute() {
        let attr: Attribute = parse_quote! { #[stream] };
        assert!(parse_stream_attribute(&attr));

        let attr: Attribute = parse_quote! { #[other] };
        assert!(!parse_stream_attribute(&attr));
    }
}
