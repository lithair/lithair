//! Attribute macro for LithairModel with automatic serde defaults
//!
//! This macro transforms `#[db(default = X)]` into `#[serde(default = "...")]`
//! enabling automatic migration support for new mandatory fields.

use proc_macro2::{Span, TokenStream};
use quote::{quote, ToTokens};
use syn::{parse2, Attribute, Data, DeriveInput, Fields, Ident, Meta};

/// Parse default value from #[db(default = X)] attribute
fn extract_default_from_db_attr(attr: &Attribute) -> Option<String> {
    if !attr.path().is_ident("db") {
        return None;
    }

    if let Meta::List(meta_list) = &attr.meta {
        let tokens_str = meta_list.tokens.to_string();

        // Look for "default = X" pattern
        if let Some(default_start) = tokens_str.find("default") {
            let remaining = &tokens_str[default_start..];
            if let Some(eq_pos) = remaining.find('=') {
                let after_eq = remaining[eq_pos + 1..].trim();
                // Find the end of the value (comma or end of string)
                let value_end = after_eq.find(',').unwrap_or(after_eq.len());
                let value = after_eq[..value_end].trim();
                if !value.is_empty() {
                    return Some(value.to_string());
                }
            }
        }
    }

    None
}

/// Generate the lithair_model attribute macro implementation
pub fn lithair_model_impl(input: TokenStream) -> TokenStream {
    let input = match parse2::<DeriveInput>(input) {
        Ok(input) => input,
        Err(err) => return err.to_compile_error(),
    };

    let name = &input.ident;
    let vis = &input.vis;
    let generics = &input.generics;

    // Collect existing derives and other attributes
    let mut other_attrs: Vec<&Attribute> = Vec::new();
    let mut has_serialize = false;
    let mut has_deserialize = false;
    let mut has_declarative = false;

    for attr in &input.attrs {
        if attr.path().is_ident("derive") {
            // Check if Serialize/Deserialize/DeclarativeModel are in the derive
            let tokens_str = attr.meta.to_token_stream().to_string();
            if tokens_str.contains("Serialize") {
                has_serialize = true;
            }
            if tokens_str.contains("Deserialize") {
                has_deserialize = true;
            }
            if tokens_str.contains("DeclarativeModel") {
                has_declarative = true;
            }
            other_attrs.push(attr);
        } else {
            other_attrs.push(attr);
        }
    }

    // Process fields
    let fields = match &input.data {
        Data::Struct(data_struct) => match &data_struct.fields {
            Fields::Named(fields_named) => &fields_named.named,
            _ => {
                return syn::Error::new_spanned(
                    name,
                    "lithair_model only supports structs with named fields",
                )
                .to_compile_error();
            }
        },
        _ => {
            return syn::Error::new_spanned(name, "lithair_model can only be applied to structs")
                .to_compile_error();
        }
    };

    // Generate field tokens with serde defaults
    let mut field_tokens = Vec::new();
    let mut default_fns = Vec::new();

    for field in fields {
        let field_name = field.ident.as_ref().unwrap();
        let field_ty = &field.ty;
        let field_vis = &field.vis;

        // Collect field attributes, potentially adding serde(default)
        let mut field_attrs: Vec<TokenStream> = Vec::new();
        let mut has_serde_default = false;
        let mut db_default_value: Option<String> = None;

        for attr in &field.attrs {
            // Check for existing serde(default)
            if attr.path().is_ident("serde") {
                let tokens_str = attr.meta.to_token_stream().to_string();
                if tokens_str.contains("default") {
                    has_serde_default = true;
                }
            }

            // Extract default from db attribute
            if let Some(default_val) = extract_default_from_db_attr(attr) {
                db_default_value = Some(default_val);
            }

            // Keep all original attributes
            field_attrs.push(attr.to_token_stream());
        }

        // If we have a db(default) but no serde(default), add one
        if let Some(default_val) = db_default_value {
            if !has_serde_default {
                // Generate a default function name
                let fn_name =
                    Ident::new(&format!("__lithair_default_{}", field_name), Span::call_site());

                // Parse the default value to generate appropriate code
                let default_expr: TokenStream = if default_val == "true" || default_val == "false" {
                    // Boolean
                    default_val.parse().unwrap()
                } else if default_val.starts_with('"') {
                    // String literal
                    let s = default_val.trim_matches('"');
                    quote! { #s.to_string() }
                } else if default_val.contains('.') {
                    // Float
                    let f: f64 = default_val.parse().unwrap_or(0.0);
                    quote! { #f }
                } else if let Ok(i) = default_val.parse::<i64>() {
                    // Integer
                    quote! { #i as _ }
                } else {
                    // Assume it's a function name or expression
                    default_val.parse().unwrap_or_else(|_| quote! { Default::default() })
                };

                // Generate default function
                let field_ty_clone = field_ty.clone();
                default_fns.push(quote! {
                    #[doc(hidden)]
                    #[allow(non_snake_case)]
                    fn #fn_name() -> #field_ty_clone {
                        #default_expr
                    }
                });

                // Add serde(default) attribute
                let fn_name_str = fn_name.to_string();
                field_attrs.push(quote! {
                    #[serde(default = #fn_name_str)]
                });
            }
        }

        field_tokens.push(quote! {
            #(#field_attrs)*
            #field_vis #field_name: #field_ty
        });
    }

    // Rebuild the struct with modified fields
    let other_attrs_tokens: Vec<TokenStream> =
        other_attrs.iter().map(|a| a.to_token_stream()).collect();

    // Add derives if not present
    let derive_additions = {
        let mut adds = Vec::new();
        if !has_serialize {
            adds.push(quote! { serde::Serialize });
        }
        if !has_deserialize {
            adds.push(quote! { serde::Deserialize });
        }
        if !has_declarative {
            adds.push(quote! { lithair_macros::DeclarativeModel });
        }
        if adds.is_empty() {
            quote! {}
        } else {
            quote! { #[derive(#(#adds),*)] }
        }
    };

    quote! {
        // Default value functions (generated)
        #(#default_fns)*

        // Re-emit the struct with added derives and serde defaults
        #(#other_attrs_tokens)*
        #derive_additions
        #vis struct #name #generics {
            #(#field_tokens),*
        }
    }
}
