//! Schema evolution and migration macro implementation
//! 
//! This module implements schema versioning and migration support for Lithair,
//! enabling safe schema evolution in distributed Raft consensus environments.

use proc_macro2::TokenStream;
use quote::quote;
use syn::{parse2, DeriveInput, Data, Fields, Field, Attribute, Meta, MetaList, NestedMeta, Lit};

/// Schema migration strategy
#[derive(Debug, Clone)]
enum MigrationStrategy {
    Additive,     // Only add new fields (backward compatible)
    Breaking,     // Allow breaking changes (requires consensus)
    Versioned,    // Maintain multiple versions simultaneously
}

/// Schema version information
#[derive(Debug, Clone)]
struct SchemaVersion {
    version: u32,
    strategy: MigrationStrategy,
    description: Option<String>,
}

/// Field migration information
#[derive(Debug, Clone)]
struct FieldMigration {
    since_version: u32,
    default_value: Option<String>,
    migration_fn: Option<String>,
    deprecated_since: Option<u32>,
}

/// Parse schema-level attributes
fn parse_schema_attributes(input: &DeriveInput) -> SchemaVersion {
    let mut version = 1;
    let mut strategy = MigrationStrategy::Additive;
    let mut description = None;
    
    for attr in &input.attrs {
        if attr.path.is_ident("schema") {
            if let Ok(Meta::List(MetaList { nested, .. })) = attr.parse_meta() {
                for nested_meta in nested {
                    match nested_meta {
                        NestedMeta::Meta(Meta::NameValue(nv)) if nv.path.is_ident("version") => {
                            if let Lit::Int(lit_int) = &nv.lit {
                                version = lit_int.base10_parse().unwrap_or(1);
                            }
                        }
                        NestedMeta::Meta(Meta::NameValue(nv)) if nv.path.is_ident("migration_strategy") => {
                            if let Lit::Str(lit_str) = &nv.lit {
                                strategy = match lit_str.value().as_str() {
                                    "additive" => MigrationStrategy::Additive,
                                    "breaking" => MigrationStrategy::Breaking,
                                    "versioned" => MigrationStrategy::Versioned,
                                    _ => MigrationStrategy::Additive,
                                };
                            }
                        }
                        NestedMeta::Meta(Meta::NameValue(nv)) if nv.path.is_ident("description") => {
                            if let Lit::Str(lit_str) = &nv.lit {
                                description = Some(lit_str.value());
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    
    SchemaVersion { version, strategy, description }
}

/// Parse field migration attributes
fn parse_field_migration(field: &Field) -> Option<FieldMigration> {
    for attr in &field.attrs {
        if attr.path.is_ident("migration") {
            if let Ok(Meta::List(MetaList { nested, .. })) = attr.parse_meta() {
                let mut migration = FieldMigration {
                    since_version: 1,
                    default_value: None,
                    migration_fn: None,
                    deprecated_since: None,
                };
                
                for nested_meta in nested {
                    match nested_meta {
                        NestedMeta::Meta(Meta::NameValue(nv)) if nv.path.is_ident("since") => {
                            if let Lit::Str(lit_str) = &nv.lit {
                                // Parse version from string like "v2" or "2"
                                let version_str = lit_str.value().trim_start_matches('v');
                                migration.since_version = version_str.parse().unwrap_or(1);
                            }
                        }
                        NestedMeta::Meta(Meta::NameValue(nv)) if nv.path.is_ident("default") => {
                            if let Lit::Str(lit_str) = &nv.lit {
                                migration.default_value = Some(lit_str.value());
                            }
                        }
                        NestedMeta::Meta(Meta::NameValue(nv)) if nv.path.is_ident("migration_fn") => {
                            if let Lit::Str(lit_str) = &nv.lit {
                                migration.migration_fn = Some(lit_str.value());
                            }
                        }
                        NestedMeta::Meta(Meta::NameValue(nv)) if nv.path.is_ident("deprecated_since") => {
                            if let Lit::Int(lit_int) = &nv.lit {
                                migration.deprecated_since = Some(lit_int.base10_parse().unwrap_or(1));
                            }
                        }
                        _ => {}
                    }
                }
                
                return Some(migration);
            }
        }
    }
    None
}

/// Generate the SchemaEvolution implementation
pub fn derive_schema_evolution(input: TokenStream) -> TokenStream {
    let input = parse2::<DeriveInput>(input).unwrap();
    let name = &input.ident;
    let name_str = name.to_string();
    
    let schema_version = parse_schema_attributes(&input);
    
    let fields = match &input.data {
        Data::Struct(data_struct) => {
            match &data_struct.fields {
                Fields::Named(fields_named) => &fields_named.named,
                _ => {
                    return syn::Error::new_spanned(
                        name,
                        "SchemaEvolution only supports structs with named fields"
                    ).to_compile_error();
                }
            }
        }
        _ => {
            return syn::Error::new_spanned(
                name,
                "SchemaEvolution can only be derived for structs"
            ).to_compile_error();
        }
    };
    
    // Parse field migrations
    let mut field_migrations = Vec::new();
    for field in fields {
        if let Some(field_name) = &field.ident {
            if let Some(migration) = parse_field_migration(field) {
                field_migrations.push((field_name.to_string(), migration));
            }
        }
    }
    
    let version = schema_version.version;
    let strategy_str = match schema_version.strategy {
        MigrationStrategy::Additive => "additive",
        MigrationStrategy::Breaking => "breaking",
        MigrationStrategy::Versioned => "versioned",
    };
    
    // Generate migration event enum
    let migration_events = field_migrations.iter().map(|(field_name, migration)| {
        let event_name = format!("{}Added", field_name);
        let event_ident = syn::Ident::new(&event_name, proc_macro2::Span::call_site());
        let since_version = migration.since_version;
        
        quote! {
            #event_ident { since_version: #since_version },
        }
    });
    
    // Generate migration application logic
    let migration_applications = field_migrations.iter().map(|(field_name, migration)| {
        let event_name = format!("{}Added", field_name);
        let event_ident = syn::Ident::new(&event_name, proc_macro2::Span::call_site());
        let field_ident = syn::Ident::new(field_name, proc_macro2::Span::call_site());
        
        if let Some(default_val) = &migration.default_value {
            quote! {
                SchemaEvent::#event_ident { .. } => {
                    // Apply default value for new field
                    // This would typically update the state schema
                    println!("Migrating field {} with default value: {}", #field_name, #default_val);
                }
            }
        } else {
            quote! {
                SchemaEvent::#event_ident { .. } => {
                    // Apply migration for new field
                    println!("Migrating field {} since version {}", #field_name, #(migration.since_version));
                }
            }
        }
    });
    
    // Generate Raft consensus integration for schema changes
    let consensus_integration = quote! {
        /// Schema change event that requires Raft consensus
        #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
        pub enum SchemaChangeEvent {
            /// Propose a new schema version
            ProposeSchemaVersion {
                from_version: u32,
                to_version: u32,
                strategy: String,
                changes: Vec<String>,
            },
            /// Apply approved schema migration
            ApplyMigration {
                version: u32,
                field_changes: Vec<String>,
            },
            /// Rollback schema change (if supported)
            RollbackSchema {
                from_version: u32,
                to_version: u32,
            },
        }
        
        impl lithair_core::engine::Event for SchemaChangeEvent {
            type State = SchemaState;
            
            fn apply(&self, state: &mut Self::State) {
                match self {
                    SchemaChangeEvent::ProposeSchemaVersion { to_version, .. } => {
                        state.pending_schema_version = Some(*to_version);
                    }
                    SchemaChangeEvent::ApplyMigration { version, .. } => {
                        state.current_schema_version = *version;
                        state.pending_schema_version = None;
                    }
                    SchemaChangeEvent::RollbackSchema { to_version, .. } => {
                        state.current_schema_version = *to_version;
                        state.pending_schema_version = None;
                    }
                }
            }
        }
        
        /// Schema state for Raft consensus
        #[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
        pub struct SchemaState {
            pub current_schema_version: u32,
            pub pending_schema_version: Option<u32>,
            pub migration_history: Vec<(u32, String)>, // (version, description)
        }
    };
    
    let expanded = quote! {
        /// Schema migration events for this model
        #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
        pub enum SchemaEvent {
            #(#migration_events)*
        }
        
        impl lithair_core::engine::Event for SchemaEvent {
            type State = SchemaState;
            
            fn apply(&self, _state: &mut Self::State) {
                match self {
                    #(#migration_applications)*
                }
            }
        }
        
        impl #name {
            /// Get the current schema version
            pub const SCHEMA_VERSION: u32 = #version;
            
            /// Get the migration strategy
            pub const MIGRATION_STRATEGY: &'static str = #strategy_str;
            
            /// Check if this schema version is compatible with another
            pub fn is_compatible_with_version(other_version: u32) -> bool {
                match #strategy_str {
                    "additive" => other_version <= #version,
                    "breaking" => other_version == #version,
                    "versioned" => true, // All versions supported
                    _ => false,
                }
            }
            
            /// Get migration path from one version to another
            pub fn get_migration_path(from_version: u32, to_version: u32) -> Vec<String> {
                let mut migrations = Vec::new();
                
                if from_version < to_version {
                    // Forward migration
                    for version in (from_version + 1)..=to_version {
                        migrations.push(format!("Migrate to version {}", version));
                    }
                } else if from_version > to_version {
                    // Backward migration (if supported)
                    match #strategy_str {
                        "versioned" => {
                            for version in ((to_version + 1)..=from_version).rev() {
                                migrations.push(format!("Rollback from version {}", version));
                            }
                        }
                        _ => {
                            migrations.push("Backward migration not supported".to_string());
                        }
                    }
                }
                
                migrations
            }
            
            /// Create a schema change proposal for Raft consensus
            pub fn propose_schema_change(to_version: u32, changes: Vec<String>) -> SchemaChangeEvent {
                SchemaChangeEvent::ProposeSchemaVersion {
                    from_version: #version,
                    to_version,
                    strategy: #strategy_str.to_string(),
                    changes,
                }
            }
        }
        
        #consensus_integration
    };
    
    expanded
}
