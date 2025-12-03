//! RBAC Role macro implementation
//!
//! Generates automatic permission checking from declarative attributes

use proc_macro2::TokenStream;
use quote::quote;
use syn::{parse2, Data, DeriveInput, Error, Fields, Meta, Result};

pub fn derive_rbac_role(input: TokenStream) -> Result<TokenStream> {
    let input: DeriveInput = parse2(input)?;
    let name = &input.ident;
    
    // Extract permission type from attributes or use default
    let permission_type = extract_permission_type(&input)?;
    
    let Data::Enum(data_enum) = &input.data else {
        return Err(Error::new_spanned(
            &input,
            "RbacRole can only be derived for enums",
        ));
    };
    
    // Generate match arms for each variant
    let mut match_arms = Vec::new();
    
    for variant in &data_enum.variants {
        let variant_name = &variant.ident;
        
        // Check if variant has fields (we don't support that yet)
        if !matches!(variant.fields, Fields::Unit) {
            return Err(Error::new_spanned(
                variant,
                "RbacRole only supports unit variants (no fields)",
            ));
        }
        
        // Extract permissions from attributes
        let permissions = extract_permissions(&variant.attrs)?;
        
        if permissions.is_empty() {
            // No permissions specified - deny all
            continue;
        }
        
        // Check for "all" permission
        if permissions.iter().any(|p| p == "all") {
            // This role has all permissions
            match_arms.push(quote! {
                #name::#variant_name => true,
            });
        } else {
            // Generate pattern match for specific permissions
            let perm_patterns: Vec<_> = permissions.iter().map(|p| {
                let perm_ident = syn::Ident::new(p, proc_macro2::Span::call_site());
                quote! { #permission_type::#perm_ident }
            }).collect();
            
            match_arms.push(quote! {
                #name::#variant_name => matches!(permission, #(#perm_patterns)|*),
            });
        }
    }
    
    // Add catch-all for variants without permissions
    match_arms.push(quote! {
        _ => false,
    });
    
    Ok(quote! {
        impl #name {
            /// Check if this role has the given permission
            ///
            /// This method is automatically generated from #[permissions(...)] attributes
            pub fn has_permission(&self, permission: #permission_type) -> bool {
                match self {
                    #(#match_arms)*
                }
            }
        }
    })
}

/// Extract permission type from enum-level attributes
fn extract_permission_type(input: &DeriveInput) -> Result<syn::Path> {
    for attr in &input.attrs {
        if attr.path().is_ident("permission_type") {
            if let Meta::NameValue(meta) = &attr.meta {
                if let syn::Expr::Path(expr_path) = &meta.value {
                    return Ok(expr_path.path.clone());
                }
            }
        }
    }
    
    // Default permission type
    Ok(syn::parse_str("ProductPermission").unwrap())
}

/// Extract permissions from variant attributes
fn extract_permissions(attrs: &[syn::Attribute]) -> Result<Vec<String>> {
    let mut permissions = Vec::new();
    
    for attr in attrs {
        if attr.path().is_ident("permissions") {
            // Parse #[permissions(PermA, PermB, PermC)]
            attr.parse_nested_meta(|meta| {
                if let Some(ident) = meta.path.get_ident() {
                    permissions.push(ident.to_string());
                    Ok(())
                } else {
                    Err(meta.error("Expected permission identifier"))
                }
            })?;
        } else if attr.path().is_ident("permission") {
            // Also support singular #[permission(PermA)]
            attr.parse_nested_meta(|meta| {
                if let Some(ident) = meta.path.get_ident() {
                    permissions.push(ident.to_string());
                    Ok(())
                } else {
                    Err(meta.error("Expected permission identifier"))
                }
            })?;
        } else if attr.path().is_ident("doc") {
            // Skip doc comments
            continue;
        }
    }
    
    Ok(permissions)
}
