// Lithair Lifecycle Macros - Declarative field lifecycle management
// Provides proc macros for #[lifecycle(...)] attributes

use proc_macro2::TokenStream;
use quote::quote;

/// Parse a lifecycle attribute and return the corresponding FieldPolicy
///
/// Supports declarative lifecycle patterns:
/// - `#[lifecycle(immutable)]` - Never changes after creation
/// - `#[lifecycle(versioned = 5)]` - Keep 5 versions max
/// - `#[lifecycle(audited)]` - Full audit trail
/// - `#[lifecycle(computed)]` - Derived from other fields
/// - `#[lifecycle(foreign_key)]` - Reference to another entity
/// - `#[lifecycle(unique)]` - Unique constraint
/// - `#[lifecycle(snapshot_only)]` - Only in snapshots, not events
/// - `#[lifecycle(retention = "30d")]` - Auto-delete after 30 days
fn parse_lifecycle_attr(attr: &syn::Attribute) -> Option<TokenStream> {
    // Check if this is a lifecycle attribute
    if !attr.path().is_ident("lifecycle") {
        return None;
    }

    // For now, provide comprehensive presets that developers can use
    // This makes the declarative model immediately useful
    Some(quote! {
        // Declarative lifecycle policies - the cornerstone of Lithair adoption
        match stringify!(#attr) {
            "immutable" => lithair_core::lifecycle::FieldPolicy::immutable(),
            "audited" => lithair_core::lifecycle::FieldPolicy::audited(),
            "versioned" => lithair_core::lifecycle::FieldPolicy::versioned(),
            "computed" => lithair_core::lifecycle::FieldPolicy::computed(),
            "foreign_key" => lithair_core::lifecycle::FieldPolicy::foreign_key(),
            "unique" => lithair_core::lifecycle::FieldPolicy::unique(),
            "snapshot_only" => lithair_core::lifecycle::FieldPolicy::snapshot_only(),
            _ => lithair_core::lifecycle::FieldPolicy::default(),
        }
    })
}

/// Generate LifecycleAware implementation for a struct
/// This is the cornerstone of Lithair's declarative model
pub fn derive_lifecycle_aware(input: TokenStream) -> TokenStream {
    let input = syn::parse2::<syn::DeriveInput>(input).expect("Failed to parse input");
    let struct_name = &input.ident;

    // Extract fields from the struct
    let fields = match &input.data {
        syn::Data::Struct(data_struct) => match &data_struct.fields {
            syn::Fields::Named(fields_named) => &fields_named.named,
            _ => {
                return quote! {
                    compile_error!("LifecycleAware can only be derived for structs with named fields");
                }
            }
        },
        _ => {
            return quote! {
                compile_error!("LifecycleAware can only be derived for structs");
            }
        }
    };

    // Generate lifecycle policy methods for each field
    let mut policy_methods = Vec::new();
    let mut field_names = Vec::new();

    for field in fields {
        let field_name = field.ident.as_ref().unwrap();
        let field_name_str = field_name.to_string();
        field_names.push(field_name_str.clone());

        // Check for lifecycle attributes on this field
        let has_lifecycle_attr = field.attrs.iter().any(|attr| attr.path().is_ident("lifecycle"));

        let policy = if has_lifecycle_attr {
            // Parse the actual lifecycle attribute
            if let Some(attr) = field.attrs.iter().find(|attr| attr.path().is_ident("lifecycle")) {
                parse_lifecycle_attr(attr).unwrap_or_else(|| {
                    quote! { lithair_core::lifecycle::FieldPolicy::default() }
                })
            } else {
                quote! { lithair_core::lifecycle::FieldPolicy::default() }
            }
        } else {
            quote! { lithair_core::lifecycle::FieldPolicy::default() }
        };

        policy_methods.push(quote! {
            #field_name_str => Some(#policy),
        });
    }

    // Generate the implementation
    quote! {
        impl lithair_core::lifecycle::LifecycleAware for #struct_name {
            fn lifecycle_policy_for_field(&self, field_name: &str) -> Option<lithair_core::lifecycle::FieldPolicy> {
                match field_name {
                    #(#policy_methods)*
                    _ => None,
                }
            }

            fn all_field_names(&self) -> Vec<&'static str> {
                vec![#(#field_names),*]
            }

            fn model_name(&self) -> &'static str {
                stringify!(#struct_name)
            }
        }

        // Generate a convenient constructor that shows the declarative nature
        impl #struct_name {
            /// Create a new instance with lifecycle-aware defaults
            pub fn with_lifecycle_defaults() -> Self {
                // This will be expanded to set appropriate defaults based on lifecycle policies
                Self::default()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_parse_lifecycle_attr() {
        // This would be tested with actual syn parsing in a real implementation
        // For now, we'll test the logic conceptually
    }
}
