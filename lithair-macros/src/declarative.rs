//! Declarative model macro implementation
//! 
//! This module implements the DeclarativeModel derive macro that parses
//! field attributes and generates unified specifications automatically.

use proc_macro2::TokenStream;
use quote::quote;
use syn::{parse2, DeriveInput, Data, Fields, Field, Attribute, Meta, MetaList, NestedMeta, Lit, Error, Path};
use std::collections::HashMap;

/// Parsed field attributes from declarative annotations
#[derive(Debug, Default)]
struct FieldAttributes {
    // DB attributes
    primary_key: bool,
    unique: bool,
    indexed: bool,
    foreign_key: Option<String>,
    nullable: bool,
    
    // Lifecycle attributes
    immutable: bool,
    audited: bool,
    versioned: u32,
    retention: usize,
    snapshot_only: bool,
    
    // Persistence attributes
    memory_only: bool,      // Force in-memory only
    persist: bool,          // Force persistence  
    auto_persist: bool,     // Auto-persist writes (default for most data)
    replication: bool,      // Enable replication
    history_tracking: bool, // Track full event history
    
    // HTTP attributes
    expose: bool,
    validation: Vec<String>,
    serialization: Option<String>,
    
    // Permission attributes
    read_permission: Option<String>,
    write_permission: Option<String>,
    owner_field: bool,
}

/// Parse attributes from a field
fn parse_field_attributes(field: &Field) -> FieldAttributes {
    let mut attrs = FieldAttributes {
        expose: true, // Default to exposed unless explicitly set to false
        retention: usize::MAX, // Default to no retention limit
        auto_persist: true, // Default to auto-persist data writes
        replication: true, // Default to enable replication
        history_tracking: true, // Default to track history
        ..Default::default()
    };
    
    for attr in &field.attrs {
        match attr.path.get_ident() {
            Some(ident) if ident == "db" => parse_db_attributes(&mut attrs, attr),
            Some(ident) if ident == "lifecycle" => parse_lifecycle_attributes(&mut attrs, attr),
            Some(ident) if ident == "persistence" => parse_persistence_attributes(&mut attrs, attr),
            Some(ident) if ident == "http" => parse_http_attributes(&mut attrs, attr),
            Some(ident) if ident == "permission" => parse_permission_attributes(&mut attrs, attr),
            Some(ident) if ident == "rbac" => parse_rbac_attributes(&mut attrs, attr),
            _ => {}
        }
    }
    
    // Apply intelligent defaults based on Lithair philosophy:
    // Foreign keys and constraints stay in-memory by default
    if attrs.foreign_key.is_some() || attrs.primary_key || attrs.unique {
        attrs.memory_only = true;
        attrs.auto_persist = false;
        attrs.history_tracking = false; // Constraints don't need history
    }
    
    attrs
}

/// Parse #[db(...)] attributes
fn parse_db_attributes(attrs: &mut FieldAttributes, attr: &Attribute) {
    if let Ok(meta) = attr.parse_meta() {
        if let Meta::List(meta_list) = meta {
            for nested_meta in meta_list.nested {
                match nested_meta {
                NestedMeta::Meta(Meta::Path(path)) if path.is_ident("primary_key") => {
                    attrs.primary_key = true;
                }
                NestedMeta::Meta(Meta::Path(path)) if path.is_ident("unique") => {
                    attrs.unique = true;
                }
                NestedMeta::Meta(Meta::Path(path)) if path.is_ident("indexed") => {
                    attrs.indexed = true;
                }
                NestedMeta::Meta(Meta::Path(path)) if path.is_ident("nullable") => {
                    attrs.nullable = true;
                }
                NestedMeta::Meta(Meta::NameValue(nv)) if nv.path.is_ident("fk") => {
                    if let Lit::Str(lit_str) = &nv.lit {
                        attrs.foreign_key = Some(lit_str.value());
                    }
                }
                _ => {}
            }
        }
    }
}

/// Parse #[lifecycle(...)] attributes
fn parse_lifecycle_attributes(attrs: &mut FieldAttributes, attr: &Attribute) {
    if let Ok(Meta::List(MetaList { nested, .. })) = attr.parse_meta() {
        for nested_meta in nested {
            match nested_meta {
                NestedMeta::Meta(Meta::Path(path)) if path.is_ident("immutable") => {
                    attrs.immutable = true;
                }
                NestedMeta::Meta(Meta::Path(path)) if path.is_ident("audited") => {
                    attrs.audited = true;
                }
                NestedMeta::Meta(Meta::Path(path)) if path.is_ident("snapshot_only") => {
                    attrs.snapshot_only = true;
                }
                NestedMeta::Meta(Meta::NameValue(nv)) if nv.path.is_ident("versioned") => {
                    if let Lit::Int(lit_int) = &nv.lit {
                        attrs.versioned = lit_int.base10_parse().unwrap_or(0);
                    }
                }
                NestedMeta::Meta(Meta::NameValue(nv)) if nv.path.is_ident("retention") => {
                    if let Lit::Int(lit_int) = &nv.lit {
                        attrs.retention = lit_int.base10_parse().unwrap_or(usize::MAX);
                    }
                }
                _ => {}
            }
        }
    }
}

/// Parse #[persistence(...)] attributes
fn parse_persistence_attributes(attrs: &mut FieldAttributes, attr: &Attribute) {
    if let Ok(Meta::List(MetaList { nested, .. })) = attr.parse_meta() {
        for nested_meta in nested {
            match nested_meta {
                NestedMeta::Meta(Meta::Path(path)) if path.is_ident("memory_only") => {
                    attrs.memory_only = true;
                    attrs.auto_persist = false;
                    attrs.replication = false;
                    attrs.history_tracking = false;
                }
                NestedMeta::Meta(Meta::Path(path)) if path.is_ident("persist") => {
                    attrs.persist = true;
                    attrs.auto_persist = true;
                }
                NestedMeta::Meta(Meta::Path(path)) if path.is_ident("no_auto_persist") => {
                    attrs.auto_persist = false;
                }
                NestedMeta::Meta(Meta::Path(path)) if path.is_ident("replicate") => {
                    attrs.replication = true;
                }
                NestedMeta::Meta(Meta::Path(path)) if path.is_ident("no_replication") => {
                    attrs.replication = false;
                }
                NestedMeta::Meta(Meta::Path(path)) if path.is_ident("track_history") => {
                    attrs.history_tracking = true;
                }
                NestedMeta::Meta(Meta::Path(path)) if path.is_ident("no_history") => {
                    attrs.history_tracking = false;
                }
                _ => {}
            }
        }
    }
}

/// Parse #[http(...)] attributes
fn parse_http_attributes(attrs: &mut FieldAttributes, attr: &Attribute) {
    if let Ok(Meta::List(MetaList { nested, .. })) = attr.parse_meta() {
        for nested_meta in nested {
            match nested_meta {
                NestedMeta::Meta(Meta::Path(path)) if path.is_ident("expose") => {
                    attrs.expose = true;
                }
                NestedMeta::Meta(Meta::NameValue(nv)) if nv.path.is_ident("expose") => {
                    if let Lit::Bool(lit_bool) = &nv.lit {
                        attrs.expose = lit_bool.value;
                    }
                }
                NestedMeta::Meta(Meta::NameValue(nv)) if nv.path.is_ident("validate") => {
                    if let Lit::Str(lit_str) = &nv.lit {
                        attrs.validation.push(lit_str.value());
                    }
                }
                NestedMeta::Meta(Meta::NameValue(nv)) if nv.path.is_ident("serialize") => {
                    if let Lit::Str(lit_str) = &nv.lit {
                        attrs.serialization = Some(lit_str.value());
                    }
                }
                _ => {}
            }
        }
    }
}

/// Parse #[permission(...)] attributes
fn parse_permission_attributes(attrs: &mut FieldAttributes, attr: &Attribute) {
    if let Ok(Meta::List(MetaList { nested, .. })) = attr.parse_meta() {
        for nested_meta in nested {
            match nested_meta {
                NestedMeta::Meta(Meta::NameValue(nv)) if nv.path.is_ident("read") => {
                    if let Lit::Str(lit_str) = &nv.lit {
                        attrs.read_permission = Some(lit_str.value());
                    }
                }
                NestedMeta::Meta(Meta::NameValue(nv)) if nv.path.is_ident("write") => {
                    if let Lit::Str(lit_str) = &nv.lit {
                        attrs.write_permission = Some(lit_str.value());
                    }
                }
                _ => {}
            }
        }
    }
}

/// Parse #[rbac(...)] attributes
fn parse_rbac_attributes(attrs: &mut FieldAttributes, attr: &Attribute) {
    if let Ok(Meta::List(MetaList { nested, .. })) = attr.parse_meta() {
        for nested_meta in nested {
            match nested_meta {
                NestedMeta::Meta(Meta::Path(path)) if path.is_ident("owner_field") => {
                    attrs.owner_field = true;
                }
                _ => {}
            }
        }
    }
}

/// Generate the DeclarativeModel implementation
pub fn derive_declarative_model(input: TokenStream) -> TokenStream {
    let input = parse2::<DeriveInput>(input).unwrap();
    let name = &input.ident;
    let name_str = name.to_string();
    
    let fields = match &input.data {
        Data::Struct(data_struct) => {
            match &data_struct.fields {
                Fields::Named(fields_named) => &fields_named.named,
                _ => {
                    return syn::Error::new_spanned(
                        name,
                        "DeclarativeModel only supports structs with named fields"
                    ).to_compile_error();
                }
            }
        }
        _ => {
            return syn::Error::new_spanned(
                name,
                "DeclarativeModel can only be derived for structs"
            ).to_compile_error();
        }
    };
    
    // Parse all field attributes
    let mut field_specs = HashMap::new();
    let mut field_names = Vec::new();
    
    for field in fields {
        if let Some(field_name) = &field.ident {
            let field_name_str = field_name.to_string();
            field_names.push(field_name_str.clone());
            
            let attrs = parse_field_attributes(field);
            field_specs.insert(field_name_str, attrs);
        }
    }
    
    // Generate field specification creation code
    let field_spec_creation = field_specs.iter().map(|(field_name, attrs)| {
        let retention = if attrs.retention == usize::MAX { 
            quote! { usize::MAX } 
        } else { 
            let retention_val = attrs.retention;
            quote! { #retention_val } 
        };
        
        let validation_vec = &attrs.validation;
        let serialization = match &attrs.serialization {
            Some(s) => quote! { Some(#s.to_string()) },
            None => quote! { None },
        };
        let read_permission = match &attrs.read_permission {
            Some(p) => quote! { Some(#p.to_string()) },
            None => quote! { None },
        };
        let write_permission = match &attrs.write_permission {
            Some(p) => quote! { Some(#p.to_string()) },
            None => quote! { None },
        };
        let foreign_key = match &attrs.foreign_key {
            Some(fk) => quote! { Some(#fk.to_string()) },
            None => quote! { None },
        };
        
        quote! {
            fields.insert(#field_name, ParsedFieldAttributes {
                primary_key: #(attrs.primary_key),
                unique: #(attrs.unique),
                indexed: #(attrs.indexed),
                foreign_key: #foreign_key,
                nullable: #(attrs.nullable),
                immutable: #(attrs.immutable),
                audited: #(attrs.audited),
                versioned: #(attrs.versioned),
                retention: #retention,
                snapshot_only: #(attrs.snapshot_only),
                expose: #(attrs.expose),
                validation: vec![#(#validation_vec.to_string()),*],
                serialization: #serialization,
                read_permission: #read_permission,
                write_permission: #write_permission,
                owner_field: #(attrs.owner_field),
            });
        }
    });
    
    let field_names_array = &field_names;
    
    // Generate LifecycleAware implementation
    let lifecycle_impl = quote! {
        impl lithair_core::lifecycle::LifecycleAware for #name {
            fn lifecycle_policy_for_field(&self, field_name: &str) -> Option<lithair_core::lifecycle::FieldPolicy> {
                let spec = Self::get_declarative_spec();
                let attrs = spec.fields.get(field_name)?;
                
                Some(lithair_core::lifecycle::FieldPolicy {
                    retention_limit: attrs.retention as u32,
                    unique: attrs.unique,
                    indexed: attrs.indexed,
                    snapshot_only: attrs.snapshot_only,
                    fk: attrs.foreign_key.is_some(),
                    immutable: attrs.immutable,
                    audited: attrs.audited,
                    computed: false,
                    version_limit: attrs.versioned,
                })
            }

            fn all_field_names(&self) -> Vec<&'static str> {
                vec![#(#field_names_array),*]
            }

            fn model_name(&self) -> &'static str {
                #name_str
            }

            fn is_field_immutable(&self, field_name: &str) -> bool {
                Self::get_declarative_spec().fields.get(field_name).map_or(false, |attrs| attrs.immutable)
            }

            fn is_field_audited(&self, field_name: &str) -> bool {
                Self::get_declarative_spec().fields.get(field_name).map_or(false, |attrs| attrs.audited)
            }

            fn field_version_limit(&self, field_name: &str) -> u32 {
                Self::get_declarative_spec().fields.get(field_name).map_or(0, |attrs| attrs.versioned)
            }
        }
    };
    
    // Generate the complete implementation
    let expanded = quote! {
        // Re-export the field attributes structure
        use std::collections::HashMap;
        
        #[derive(Debug, Clone, Default)]
        pub struct ParsedFieldAttributes {
            pub primary_key: bool,
            pub unique: bool,
            pub indexed: bool,
            pub foreign_key: Option<String>,
            pub nullable: bool,
            pub immutable: bool,
            pub audited: bool,
            pub versioned: u32,
            pub retention: usize,
            pub snapshot_only: bool,
            pub expose: bool,
            pub validation: Vec<String>,
            pub serialization: Option<String>,
            pub read_permission: Option<String>,
            pub write_permission: Option<String>,
            pub owner_field: bool,
        }
        
        #[derive(Debug, Clone)]
        pub struct DeclarativeSpec {
            pub model_name: String,
            pub fields: HashMap<String, ParsedFieldAttributes>,
        }
        
        impl #name {
            /// Get the declarative specification for this model
            pub fn get_declarative_spec() -> &'static DeclarativeSpec {
                use std::sync::OnceLock;
                static SPEC: OnceLock<DeclarativeSpec> = OnceLock::new();
                
                SPEC.get_or_init(|| {
                    let mut fields = HashMap::new();
                    #(#field_spec_creation)*
                    
                    DeclarativeSpec {
                        model_name: #name_str.to_string(),
                        fields,
                    }
                })
            }
        }
        
        #lifecycle_impl
    };
    
    expanded
}
