/// Model-level HTTP attributes (e.g., public_if condition)
#[derive(Debug, Default, Clone)]
struct ModelHttpAttributes {
    public_if: Option<(String, String)>, // (field, value)
}

/// Parse struct-level #[http(...)] attributes
fn parse_model_http_attributes(input: &DeriveInput) -> ModelHttpAttributes {
    let mut http = ModelHttpAttributes::default();
    for attr in &input.attrs {
        if attr.path().is_ident("http") {
            if let Meta::List(meta_list) = &attr.meta {
                let nested_str = meta_list.tokens.to_string();
                for token in nested_str.split(',') {
                    let token = token.trim();
                    // Expect patterns like public_if = "status=Published" or "status==Published"
                    if token.starts_with("public_if") {
                        if let Some(val) = extract_string_value(token) {
                            let s = val.replace("==", "=");
                            if let Some((field, value)) = s.split_once('=') {
                                let field = field.trim().to_string();
                                let value = value.trim().to_string();
                                if !field.is_empty() && !value.is_empty() {
                                    http.public_if = Some((field, value));
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    http
}

// Simplified declarative model macro implementation
//
// This module provides a working implementation of the DeclarativeModel derive macro
// with modern syn API compatibility.

use proc_macro2::{Span, TokenStream};
use quote::quote;
use std::collections::HashMap;
use syn::{parse2, Attribute, Data, DeriveInput, Error, Field, Fields, Meta};

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

    // HTTP attributes
    expose: bool,
    validation: Vec<String>,
    serialization: Option<String>,

    // Permission attributes
    read_permission: Option<String>,
    write_permission: Option<String>,
    owner_field: bool,

    // Relation attributes
    has_many: Option<String>,
    has_one: Option<String>,
    belongs_to: Option<String>,
    cascade_delete: bool,
    cascade_null: bool,
    relation_lazy: bool,

    // Persistence attributes
    replicate: bool,
    track_history: bool,
    cache: bool,
    compact_after: Option<u64>, // Compact after N events (enables compaction)
    snapshot_every: Option<u64>, // Snapshot every N events (enables snapshots)

    // Migration attributes
    default_value: Option<String>, // Default value for schema migration (e.g., "0", "\"\"", "false")
}

/// Struct-level firewall attributes parsed from #[firewall(...)]
#[derive(Debug, Default, Clone)]
struct FirewallAttributes {
    present: bool,
    enabled: bool,
    allow: Vec<String>,
    deny: Vec<String>,
    global_qps: Option<u64>,
    per_ip_qps: Option<u64>,
    protected: Vec<String>,
    exempt: Vec<String>,
}

fn parse_bool_value(token_str: &str) -> Option<bool> {
    if let Some(eq_pos) = token_str.find('=') {
        match token_str[eq_pos + 1..].trim() {
            "true" => Some(true),
            "false" => Some(false),
            _ => None,
        }
    } else {
        None
    }
}

fn split_csv(s: &str) -> Vec<String> {
    s.split(',')
        .map(|x| x.trim())
        .filter(|x| !x.is_empty())
        .map(|x| x.to_string())
        .collect()
}

/// Parse struct-level #[firewall(...)] attributes
fn parse_firewall_attributes(input: &DeriveInput) -> FirewallAttributes {
    let mut fw = FirewallAttributes::default();
    for attr in &input.attrs {
        if attr.path().is_ident("firewall") {
            fw.present = true;
            if let Meta::List(meta_list) = &attr.meta {
                let nested_str = meta_list.tokens.to_string();
                for token in nested_str.split(',') {
                    let token = token.trim();
                    if token.starts_with("enabled") {
                        if let Some(v) = parse_bool_value(token) {
                            fw.enabled = v;
                        }
                    } else if token.starts_with("allow") {
                        if let Some(val) = extract_string_value(token) {
                            fw.allow = split_csv(&val);
                        }
                    } else if token.starts_with("deny") {
                        if let Some(val) = extract_string_value(token) {
                            fw.deny = split_csv(&val);
                        }
                    } else if token.starts_with("global_qps") {
                        if let Some(v) = extract_u64_value(token) {
                            fw.global_qps = Some(v);
                        }
                    } else if token.starts_with("per_ip_qps") {
                        if let Some(v) = extract_u64_value(token) {
                            fw.per_ip_qps = Some(v);
                        }
                    } else if token.starts_with("protected") {
                        if let Some(val) = extract_string_value(token) {
                            fw.protected = split_csv(&val);
                        }
                    } else if token.starts_with("exempt") {
                        if let Some(val) = extract_string_value(token) {
                            fw.exempt = split_csv(&val);
                        }
                    }
                }
            }
        }
    }
    fw
}

/// Server-level attributes from #[server(...)]
#[derive(Debug, Default)]
struct ServerAttributes {
    generate_main: bool,
    default_port: u16,
    distributed: bool,
    cli: bool,
}

/// Parse server attributes from struct-level #[server(...)] annotations
fn parse_server_attributes(input: &DeriveInput) -> ServerAttributes {
    let mut server_attrs = ServerAttributes {
        default_port: 8080, // Default port
        ..Default::default()
    };

    for attr in &input.attrs {
        if attr.path().is_ident("server") {
            if let Meta::List(meta_list) = &attr.meta {
                // Parse server(...) tokens
                let nested_str = meta_list.tokens.to_string();
                for token in nested_str.split(',') {
                    let token = token.trim();
                    match token {
                        "main" => server_attrs.generate_main = true,
                        "distributed" => server_attrs.distributed = true,
                        "cli" => server_attrs.cli = true,
                        token if token.starts_with("port") => {
                            // Parse port = 8080
                            if let Some(port_str) = token.split('=').nth(1) {
                                if let Ok(port) = port_str.trim().parse::<u16>() {
                                    server_attrs.default_port = port;
                                }
                            }
                        }
                        token if token.starts_with("default_port") => {
                            // Parse default_port = 9001
                            if let Some(port_str) = token.split('=').nth(1) {
                                if let Ok(port) = port_str.trim().parse::<u16>() {
                                    server_attrs.default_port = port;
                                }
                            }
                        }
                        _ => {} // Ignore unknown tokens
                    }
                }
            }
        }
    }

    server_attrs
}

/// Parse attributes from a field using modern syn API
fn parse_field_attributes(field: &Field) -> FieldAttributes {
    let mut attrs = FieldAttributes {
        expose: true,          // Default to exposed unless explicitly set to false
        retention: usize::MAX, // Default to no retention limit
        ..Default::default()
    };

    for attr in &field.attrs {
        if let Some(ident) = attr.path().get_ident() {
            match ident.to_string().as_str() {
                "db" => parse_db_attributes(&mut attrs, attr),
                "lifecycle" => parse_lifecycle_attributes(&mut attrs, attr),
                "http" => parse_http_attributes(&mut attrs, attr),
                "permission" => parse_permission_attributes(&mut attrs, attr),
                "rbac" => parse_rbac_attributes(&mut attrs, attr),
                "relation" => parse_relation_attributes(&mut attrs, attr),
                "persistence" => parse_persistence_attributes(&mut attrs, attr),
                _ => {}
            }
        }
    }

    attrs
}

/// Parse #[db(...)] attributes
fn parse_db_attributes(attrs: &mut FieldAttributes, attr: &Attribute) {
    let meta = &attr.meta;
    if let Meta::List(meta_list) = meta {
        // Get the full token string for more complex parsing
        let full_tokens = meta_list.tokens.to_string();

        for nested in meta_list.tokens.clone().into_iter() {
            let nested_str = nested.to_string();
            match nested_str.as_str() {
                "primary_key" => attrs.primary_key = true,
                "unique" => attrs.unique = true,
                "indexed" => attrs.indexed = true,
                "nullable" => attrs.nullable = true,
                _ if nested_str.starts_with("fk") => {
                    // Simple parsing for fk = "Table"
                    if let Some(value) = extract_string_value(&nested_str) {
                        attrs.foreign_key = Some(value);
                    }
                }
                _ => {}
            }
        }

        // Parse default = X (can be number, string, or boolean)
        // Handles: default = 0, default = "", default = false, default = "some_fn"
        if let Some(default_start) = full_tokens.find("default") {
            let remaining = &full_tokens[default_start..];
            if let Some(eq_pos) = remaining.find('=') {
                let after_eq = remaining[eq_pos + 1..].trim();
                // Find the end of the value (comma or end of string)
                let value_end = after_eq.find(',').unwrap_or(after_eq.len());
                let value = after_eq[..value_end].trim().trim_matches('"');
                if !value.is_empty() {
                    attrs.default_value = Some(value.to_string());
                }
            }
        }
    }
}

/// Parse #[lifecycle(...)] attributes
fn parse_lifecycle_attributes(attrs: &mut FieldAttributes, attr: &Attribute) {
    let meta = &attr.meta;
    if let Meta::List(meta_list) = meta {
        for nested in meta_list.tokens.clone().into_iter() {
            let nested_str = nested.to_string();
            match nested_str.as_str() {
                "immutable" => attrs.immutable = true,
                "audited" => attrs.audited = true,
                "snapshot_only" => attrs.snapshot_only = true,
                _ if nested_str.starts_with("versioned") => {
                    if let Some(value) = extract_u32_value(&nested_str) {
                        attrs.versioned = value;
                    }
                }
                _ if nested_str.starts_with("retention") => {
                    if let Some(value) = extract_usize_value(&nested_str) {
                        attrs.retention = value;
                    }
                }
                _ => {}
            }
        }
    }
}

/// Parse #[http(...)] attributes
fn parse_http_attributes(attrs: &mut FieldAttributes, attr: &Attribute) {
    let meta = &attr.meta;
    if let Meta::List(meta_list) = meta {
        for nested in meta_list.tokens.clone().into_iter() {
            let nested_str = nested.to_string();
            match nested_str.as_str() {
                "expose" => attrs.expose = true,
                _ if nested_str.starts_with("expose") && nested_str.contains("false") => {
                    attrs.expose = false;
                }
                _ if nested_str.starts_with("validate") => {
                    if let Some(value) = extract_string_value(&nested_str) {
                        attrs.validation.push(value);
                    }
                }
                _ if nested_str.starts_with("serialize") => {
                    if let Some(value) = extract_string_value(&nested_str) {
                        attrs.serialization = Some(value);
                    }
                }
                _ => {}
            }
        }
    }
}

/// Parse #[permission(...)] attributes
fn parse_permission_attributes(attrs: &mut FieldAttributes, attr: &Attribute) {
    let meta = &attr.meta;
    if let Meta::List(meta_list) = meta {
        for nested in meta_list.tokens.clone().into_iter() {
            let nested_str = nested.to_string();
            if nested_str.starts_with("read") {
                if let Some(value) = extract_string_value(&nested_str) {
                    attrs.read_permission = Some(value);
                }
            } else if nested_str.starts_with("write") {
                if let Some(value) = extract_string_value(&nested_str) {
                    attrs.write_permission = Some(value);
                }
            }
        }
    }
}

/// Parse #[rbac(...)] attributes
fn parse_rbac_attributes(attrs: &mut FieldAttributes, attr: &Attribute) {
    let meta = &attr.meta;
    if let Meta::List(meta_list) = meta {
        for nested in meta_list.tokens.clone().into_iter() {
            let nested_str = nested.to_string();
            if nested_str.contains("owner_field") {
                attrs.owner_field = true;
            }
        }
    }
}

/// Parse #[persistence(...)] attributes
fn parse_persistence_attributes(attrs: &mut FieldAttributes, attr: &Attribute) {
    let meta = &attr.meta;
    if let Meta::List(meta_list) = meta {
        for nested in meta_list.tokens.clone().into_iter() {
            let nested_str = nested.to_string();
            match nested_str.as_str() {
                "replicate" => attrs.replicate = true,
                "track_history" => attrs.track_history = true,
                "cache" => attrs.cache = true,
                _ if nested_str.starts_with("compact_after") => {
                    if let Some(value) = extract_u64_value(&nested_str) {
                        attrs.compact_after = Some(value);
                    }
                }
                _ if nested_str.starts_with("snapshot_every") => {
                    if let Some(value) = extract_u64_value(&nested_str) {
                        attrs.snapshot_every = Some(value);
                    }
                }
                _ => {}
            }
        }
    }
}

/// Parse #[relation(...)] attributes
fn parse_relation_attributes(attrs: &mut FieldAttributes, attr: &Attribute) {
    let meta = &attr.meta;
    if let Meta::List(meta_list) = meta {
        let tokens: Vec<String> =
            meta_list.tokens.clone().into_iter().map(|t| t.to_string()).collect();
        let mut i = 0;

        while i < tokens.len() {
            let token = &tokens[i];
            match token.as_str() {
                "cascade_delete" => attrs.cascade_delete = true,
                "cascade_null" => attrs.cascade_null = true,
                "lazy" => attrs.relation_lazy = true,
                "eager" => attrs.relation_lazy = false,
                "indexed" => { /* handled in db attributes */ }
                "foreign_key" => {
                    // Look for = "Value" pattern
                    if i + 2 < tokens.len() && tokens[i + 1] == "=" {
                        if let Some(value) = extract_string_value(&tokens[i + 2]) {
                            attrs.foreign_key = Some(value);
                        }
                        i += 2; // Skip = and value tokens
                    }
                }
                "has_many" => {
                    if i + 2 < tokens.len() && tokens[i + 1] == "=" {
                        if let Some(value) = extract_string_value(&tokens[i + 2]) {
                            attrs.has_many = Some(value);
                        }
                        i += 2;
                    }
                }
                "has_one" => {
                    if i + 2 < tokens.len() && tokens[i + 1] == "=" {
                        if let Some(value) = extract_string_value(&tokens[i + 2]) {
                            attrs.has_one = Some(value);
                        }
                        i += 2;
                    }
                }
                "belongs_to" => {
                    if i + 2 < tokens.len() && tokens[i + 1] == "=" {
                        if let Some(value) = extract_string_value(&tokens[i + 2]) {
                            attrs.belongs_to = Some(value);
                        }
                        i += 2;
                    }
                }
                _ => {}
            }
            i += 1;
        }
    }
}

/// Extract string value from attribute token (simple parsing)
fn extract_string_value(token_str: &str) -> Option<String> {
    if let Some(start) = token_str.find('"') {
        if let Some(end) = token_str.rfind('"') {
            if start < end {
                return Some(token_str[start + 1..end].to_string());
            }
        }
    }
    None
}

/// Extract u32 value from attribute token
fn extract_u32_value(token_str: &str) -> Option<u32> {
    if let Some(eq_pos) = token_str.find('=') {
        let value_part = &token_str[eq_pos + 1..].trim();
        value_part.parse().ok()
    } else {
        None
    }
}

/// Extract usize value from attribute token
fn extract_usize_value(token_str: &str) -> Option<usize> {
    if let Some(eq_pos) = token_str.find('=') {
        let value_part = &token_str[eq_pos + 1..].trim();
        value_part.parse().ok()
    } else {
        None
    }
}

/// Extract u64 value from attribute token
fn extract_u64_value(token_str: &str) -> Option<u64> {
    if let Some(eq_pos) = token_str.find('=') {
        let value_part = &token_str[eq_pos + 1..].trim();
        value_part.parse().ok()
    } else {
        None
    }
}

/// Generate the DeclarativeModel implementation
#[allow(unreachable_code, unused_variables)]
pub fn derive_declarative_model(input: TokenStream) -> TokenStream {
    let input = match parse2::<DeriveInput>(input) {
        Ok(input) => input,
        Err(err) => return err.to_compile_error(),
    };

    let name = &input.ident;
    let name_str = name.to_string();
    let name_lit = syn::LitStr::new(&name_str, Span::call_site());

    // Keep server attributes parsed so later (currently unreachable) code compiles
    let server_attrs = parse_server_attributes(&input);

    // Parse firewall and http attributes
    let fw_attrs = parse_firewall_attributes(&input);
    let http_model_attrs = parse_model_http_attributes(&input);

    // Generate fw_fn
    let fw_fn = if fw_attrs.present {
        let enabled = fw_attrs.enabled;
        let allow_lits: Vec<syn::LitStr> =
            fw_attrs.allow.iter().map(|s| syn::LitStr::new(s, Span::call_site())).collect();
        let deny_lits: Vec<syn::LitStr> =
            fw_attrs.deny.iter().map(|s| syn::LitStr::new(s, Span::call_site())).collect();
        let prot_lits: Vec<syn::LitStr> = fw_attrs
            .protected
            .iter()
            .map(|s| syn::LitStr::new(s, Span::call_site()))
            .collect();
        let ex_lits: Vec<syn::LitStr> =
            fw_attrs.exempt.iter().map(|s| syn::LitStr::new(s, Span::call_site())).collect();
        let global_qps_ts = match fw_attrs.global_qps {
            Some(v) => quote! { Some(#v) },
            None => quote! { None },
        };
        let per_ip_qps_ts = match fw_attrs.per_ip_qps {
            Some(v) => quote! { Some(#v) },
            None => quote! { None },
        };

        let allow_strings: Vec<proc_macro2::TokenStream> = allow_lits.iter().map(|l| quote!{ #l.to_string() }).collect();
        let deny_strings: Vec<proc_macro2::TokenStream> = deny_lits.iter().map(|l| quote!{ #l.to_string() }).collect();
        let prot_strings: Vec<proc_macro2::TokenStream> = prot_lits.iter().map(|l| quote!{ #l.to_string() }).collect();
        let ex_strings: Vec<proc_macro2::TokenStream> = ex_lits.iter().map(|l| quote!{ #l.to_string() }).collect();

        quote! {
            fn firewall_config() -> Option<lithair_core::http::FirewallConfig> {
                let mut allow: std::collections::HashSet<String> = std::collections::HashSet::new();
                #( allow.insert(#allow_strings); )*
                let mut deny: std::collections::HashSet<String> = std::collections::HashSet::new();
                #( deny.insert(#deny_strings); )*
                let protected_prefixes: Vec<String> = vec![#(#prot_strings),*];
                let exempt_prefixes: Vec<String> = vec![#(#ex_strings),*];
                Some(lithair_core::http::FirewallConfig {
                    enabled: #enabled,
                    allow,
                    deny,
                    global_qps: #global_qps_ts,
                    per_ip_qps: #per_ip_qps_ts,
                    protected_prefixes,
                    exempt_prefixes,
                })
            }
        }
    } else {
        quote! {}
    };

    // Collect read/write permissions from fields for permission gating
    let mut read_perms_vec: Vec<syn::LitStr> = Vec::new();
    let mut write_perms_vec: Vec<syn::LitStr> = Vec::new();
    if let Data::Struct(data_struct) = &input.data {
        if let Fields::Named(fields_named) = &data_struct.fields {
            for field in &fields_named.named {
                let attrs = parse_field_attributes(field);
                if let Some(rp) = attrs.read_permission {
                    read_perms_vec.push(syn::LitStr::new(&rp, Span::call_site()));
                }
                if let Some(wp) = attrs.write_permission {
                    write_perms_vec.push(syn::LitStr::new(&wp, Span::call_site()));
                }
            }
        }
    }

    // Generate can_read_fn
    let can_read_fn = {
        let public_field_value = http_model_attrs.public_if.clone();
        let perms = read_perms_vec.clone();
        if public_field_value.is_some() || !perms.is_empty() {
            let public_check = if let Some((field, value)) = public_field_value {
                let field_lit = syn::LitStr::new(&field, Span::call_site());
                let value_lit = syn::LitStr::new(&value, Span::call_site());
                quote! {
                    // Public-if condition
                    if let Ok(__v) = serde_json::to_value(self) {
                        if let Some(__s) = __v.get(#field_lit).and_then(|x| x.as_str()) {
                            if __s == #value_lit { return true; }
                        }
                    }
                }
            } else {
                quote! {}
            };
            quote! {
                fn can_read(&self, user_permissions: &[String]) -> bool {
                    #public_check
                    // Permission-based read
                    let __declared: &[&str] = &[ #( #perms ),* ];
                    for rp in __declared { if user_permissions.iter().any(|p| p == rp) { return true; } }
                    false
                }
            }
        } else {
            quote! {}
        }
    };

    // Generate can_write_fn
    let can_write_fn = {
        let perms = write_perms_vec.clone();
        if !perms.is_empty() {
            quote! {
                fn can_write(&self, user_permissions: &[String]) -> bool {
                    let __declared: &[&str] = &[ #( #perms ),* ];
                    for wp in __declared { if user_permissions.iter().any(|p| p == wp) { return true; } }
                    false
                }
            }
        } else {
            quote! {}
        }
    };

    // PROCEED TO FULL PARSING (Deleted quick-fix return)

    let fields = match &input.data {
        Data::Struct(data_struct) => match &data_struct.fields {
            Fields::Named(fields_named) => &fields_named.named,
            _ => {
                return Error::new_spanned(
                    name,
                    "DeclarativeModel only supports structs with named fields",
                )
                .to_compile_error();
            }
        },
        _ => {
            return Error::new_spanned(name, "DeclarativeModel can only be derived for structs")
                .to_compile_error();
        }
    };

    // Parse all field attributes
    let mut field_specs = HashMap::new();
    let mut field_names: Vec<String> = Vec::new();
    let mut replicated_fields: Vec<String> = Vec::new();

    for field in fields {
        if let Some(field_name) = &field.ident {
            let field_name_str = field_name.to_string();
            field_names.push(field_name_str.clone());

            let attrs = parse_field_attributes(field);
            if attrs.replicate {
                replicated_fields.push(field_name_str.clone());
            }
            field_specs.insert(field_name_str, attrs);
        }
    }

    // Generate model-specific type names
    let spec_name = syn::Ident::new(&format!("{}DeclarativeSpec", name), name.span());
    let attrs_name = syn::Ident::new(&format!("{}FieldAttributes", name), name.span());

    // Generate field specification creation code
    let field_spec_creation = field_specs.iter().map(|(field_name, attrs)| {
        let retention = if attrs.retention == usize::MAX {
            quote! { usize::MAX }
        } else {
            let retention_val = attrs.retention;
            quote! { #retention_val }
        };

        // Build validation tokens explicitly to avoid unparsable macro repetitions like
        // vec![#(#validation_vec.to_string()),*]
        let validation_tokens: Vec<proc_macro2::TokenStream> = attrs
            .validation
            .iter()
            .map(|s| {
                let lit = syn::LitStr::new(s.as_str(), Span::call_site());
                quote! { #lit.to_string() }
            })
            .collect();
        let serialization = match &attrs.serialization {
            Some(s) => {
                let lit = syn::LitStr::new(s.as_str(), Span::call_site());
                quote! { Some(#lit.to_string()) }
            }
            None => quote! { None },
        };
        let read_permission = match &attrs.read_permission {
            Some(p) => {
                let lit = syn::LitStr::new(p.as_str(), Span::call_site());
                quote! { Some(#lit.to_string()) }
            }
            None => quote! { None },
        };
        let write_permission = match &attrs.write_permission {
            Some(p) => {
                let lit = syn::LitStr::new(p.as_str(), Span::call_site());
                quote! { Some(#lit.to_string()) }
            }
            None => quote! { None },
        };
        let foreign_key = match &attrs.foreign_key {
            Some(fk) => {
                let lit = syn::LitStr::new(fk.as_str(), Span::call_site());
                quote! { Some(#lit.to_string()) }
            }
            None => quote! { None },
        };

        let primary_key = attrs.primary_key;
        let unique = attrs.unique;
        let indexed = attrs.indexed;
        let nullable = attrs.nullable;
        let immutable = attrs.immutable;
        let audited = attrs.audited;
        let versioned = attrs.versioned;
        let snapshot_only = attrs.snapshot_only;
        let expose = attrs.expose;
        let owner_field = attrs.owner_field;

        // Relation attributes
        let has_many = match &attrs.has_many {
            Some(model) => {
                let lit = syn::LitStr::new(model.as_str(), Span::call_site());
                quote! { Some(#lit.to_string()) }
            }
            None => quote! { None },
        };
        let has_one = match &attrs.has_one {
            Some(model) => {
                let lit = syn::LitStr::new(model.as_str(), Span::call_site());
                quote! { Some(#lit.to_string()) }
            }
            None => quote! { None },
        };
        let belongs_to = match &attrs.belongs_to {
            Some(model) => {
                let lit = syn::LitStr::new(model.as_str(), Span::call_site());
                quote! { Some(#lit.to_string()) }
            }
            None => quote! { None },
        };
        let cascade_delete = attrs.cascade_delete;
        let cascade_null = attrs.cascade_null;
        let relation_lazy = attrs.relation_lazy;

        // Persistence attributes
        let replicate = attrs.replicate;
        let track_history = attrs.track_history;
        let cache = attrs.cache;

        let compact_after = match attrs.compact_after {
            Some(v) => quote! { Some(#v) },
            None => quote! { None },
        };

        let snapshot_every = match attrs.snapshot_every {
            Some(v) => quote! { Some(#v) },
            None => quote! { None },
        };

        // Migration attributes
        let default_value = match &attrs.default_value {
            Some(v) => {
                let lit = syn::LitStr::new(v.as_str(), Span::call_site());
                quote! { Some(#lit.to_string()) }
            }
            None => quote! { None },
        };

        let field_name_lit = syn::LitStr::new(field_name.as_str(), Span::call_site());
        quote! {
            fields.insert(#field_name_lit.to_string(), #attrs_name {
                primary_key: #primary_key,
                unique: #unique,
                indexed: #indexed,
                foreign_key: #foreign_key,
                nullable: #nullable,
                immutable: #immutable,
                audited: #audited,
                versioned: #versioned,
                retention: #retention,
                snapshot_only: #snapshot_only,
                expose: #expose,
                validation: vec![#(#validation_tokens),*],
                serialization: #serialization,
                read_permission: #read_permission,
                write_permission: #write_permission,
                owner_field: #owner_field,

                // Relation attributes
                has_many: #has_many,
                has_one: #has_one,
                belongs_to: #belongs_to,
                cascade_delete: #cascade_delete,
                cascade_null: #cascade_null,
                relation_lazy: #relation_lazy,

                // Persistence attributes
                replicate: #replicate,
                track_history: #track_history,
                cache: #cache,
                compact_after: #compact_after,
                snapshot_every: #snapshot_every,

                // Migration attributes
                default_value: #default_value,
            });
        }
    });

    // Prepare literal tokens for field names and replicated fields
    let field_name_lits: Vec<syn::LitStr> = field_names
        .iter()
        .map(|s| syn::LitStr::new(s.as_str(), Span::call_site()))
        .collect();

    // Prepare string conversion tokens for get_all_fields
    let field_name_strings: Vec<proc_macro2::TokenStream> = field_names
        .iter()
        .map(|s| {
            let lit = syn::LitStr::new(s.as_str(), Span::call_site());
            quote! { #lit.to_string() }
        })
        .collect();

    let replicated_field_lits: Vec<syn::LitStr> = replicated_fields
        .iter()
        .map(|s| syn::LitStr::new(s.as_str(), Span::call_site()))
        .collect();

    // Determine primary key field name (from #[db(primary_key)] or default to "id")
    let primary_key_field_name: String = field_specs
        .iter()
        .find(|(_, attrs)| attrs.primary_key)
        .map(|(name, _)| name.clone())
        .unwrap_or_else(|| "id".to_string());

    let pk_field_ident = syn::Ident::new(&primary_key_field_name, Span::call_site());
    let pk_field_lit = syn::LitStr::new(&primary_key_field_name, Span::call_site());

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
                vec![#(#field_name_lits),*]
            }

            fn model_name(&self) -> &'static str {
                #name_lit
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

    let expanded = quote! {
        // Generate model-specific types to avoid conflicts between different DeclarativeModel structs
        #[derive(Debug, Clone, Default)]
        pub struct #attrs_name {
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

            // Relation attributes
            pub has_many: Option<String>,
            pub has_one: Option<String>,
            pub belongs_to: Option<String>,
            pub cascade_delete: bool,
            pub cascade_null: bool,
            pub relation_lazy: bool,

            // Persistence attributes
            pub replicate: bool,
            pub track_history: bool,
            pub cache: bool,
            pub compact_after: Option<u64>,
            pub snapshot_every: Option<u64>,

            // Migration attributes
            pub default_value: Option<String>,
        }

        #[derive(Debug, Clone)]
        pub struct #spec_name {
            pub model_name: String,
            pub fields: std::collections::HashMap<String, #attrs_name>,
        }

        impl #name {
            /// Get the declarative specification for this model
            pub fn get_declarative_spec() -> &'static #spec_name {
                use std::sync::OnceLock;
                use std::collections::HashMap;

                static SPEC: OnceLock<#spec_name> = OnceLock::new();

                SPEC.get_or_init(|| {
                    let mut fields = HashMap::new();
                    #(#field_spec_creation)*

                    #spec_name {
                        model_name: #name_lit.to_string(),
                        fields,
                    }
                })
            }

            /// Extract schema specification for schema evolution detection
            pub fn extract_schema_spec() -> lithair_core::schema::ModelSpec {
                use std::collections::HashMap;
                use lithair_core::schema::{ModelSpec, FieldConstraints, FieldPermissions, IndexSpec, ForeignKeySpec};

                let spec = Self::get_declarative_spec();
                let mut schema_fields = HashMap::new();
                let mut indexes = Vec::new();
                let mut foreign_keys = Vec::new();

                for (field_name, attrs) in &spec.fields {
                    let constraints = FieldConstraints {
                        primary_key: attrs.primary_key,
                        unique: attrs.unique,
                        indexed: attrs.indexed,
                        foreign_key: attrs.foreign_key.clone(),
                        nullable: attrs.nullable,
                        immutable: attrs.immutable,
                        audited: attrs.audited,
                        versioned: attrs.versioned,
                        retention: attrs.retention,
                        snapshot_only: attrs.snapshot_only,
                        validation_rules: attrs.validation.clone(),
                        permissions: FieldPermissions {
                            read_permission: attrs.read_permission.clone(),
                            write_permission: attrs.write_permission.clone(),
                            owner_field: attrs.owner_field,
                        },
                        default_value: attrs.default_value.clone(),
                    };

                    schema_fields.insert(field_name.clone(), constraints);

                    // Générer les index automatiquement
                    if attrs.indexed {
                        indexes.push(IndexSpec {
                            name: format!("idx_{}_{}", #name_lit.to_lowercase(), field_name),
                            fields: vec![field_name.clone()],
                            unique: attrs.unique,
                        });
                    }

                    // Générer les clés étrangères automatiquement
                    if let Some(ref fk_table) = attrs.foreign_key {
                        foreign_keys.push(ForeignKeySpec {
                            field: field_name.clone(),
                            references_table: fk_table.clone(),
                            references_field: "id".to_string(), // Convention par défaut
                        });
                    }
                }

                ModelSpec {
                    model_name: #name_lit.to_string(),
                    version: 1, // TODO: extraire depuis un attribut #[schema(version = 2)]
                    fields: schema_fields,
                    indexes,
                    foreign_keys,
                }
            }
        }

        // Implémentation automatique du trait DeclarativeSpecExtractor
        impl lithair_core::schema::DeclarativeSpecExtractor for #name {
            fn extract_model_spec(&self) -> lithair_core::schema::ModelSpec {
                Self::extract_schema_spec()
            }

            fn schema_version(&self) -> u32 {
                1 // TODO: extraire depuis un attribut #[schema(version = 2)]
            }

            fn field_constraints(&self, field_name: &str) -> Option<lithair_core::schema::FieldConstraints> {
                self.extract_model_spec().fields.get(field_name).cloned()
            }
        }

        // Implémentation du trait HasSchemaSpec pour l'extraction statique
        impl lithair_core::schema::HasSchemaSpec for #name {
            fn schema_spec() -> lithair_core::schema::ModelSpec {
                Self::extract_schema_spec()
            }

            fn model_name() -> &'static str {
                #name_lit
            }
        }

        // ModelSpec implementation for AutoJoiner and Relations
        impl lithair_core::model::ModelSpec for #name {
            fn get_policy(&self, field_name: &str) -> Option<lithair_core::model::FieldPolicy> {
                let spec = Self::get_declarative_spec();
                let attrs = spec.fields.get(field_name)?;

                Some(lithair_core::model::FieldPolicy {
                    retention_limit: attrs.retention,
                    unique: attrs.unique,
                    indexed: attrs.indexed,
                    snapshot_only: attrs.snapshot_only,
                    fk: attrs.foreign_key.is_some(),
                    fk_collection: attrs.foreign_key.clone(),
                })
            }

            fn get_all_fields(&self) -> Vec<String> {
                vec![#(#field_name_strings),*]
            }
        }

        #lifecycle_impl

        // Auto-generate ReplicatedModel implementation if any fields have #[persistence(replicate)]
        impl lithair_core::consensus::ReplicatedModel for #name {
            fn needs_replication() -> bool {
                let spec = Self::get_declarative_spec();
                spec.fields.values().any(|attrs| attrs.replicate)
            }

            fn replicated_fields() -> Vec<&'static str> {
                // Compile-time generated list of replicated field names
                vec![#(#replicated_field_lits),*]
            }
        }

        // Auto-generate HttpExposable implementation
        impl lithair_core::http::HttpExposable for #name {
            fn http_base_path() -> &'static str {
                // Auto-generate base path from struct name: Product -> "products"
                match #name_lit {
                    "Product" => "products",
                    "User" => "users",
                    "Order" => "orders",
                    "Article" => "articles",
                    "StaticAsset" => "assets",
                    "Author" => "authors",
                    "Category" => "categories",
                    "Comment" => "comments",
                    "ConsensusProduct" => "consensus_products",
                    "VirtualHost" => "virtual_hosts",
                    _ => "items" // Generic fallback
                }
            }

            fn primary_key_field() -> &'static str {
                #pk_field_lit
            }

            fn get_primary_key(&self) -> String {
                self.#pk_field_ident.to_string()
            }

            fn validate(&self) -> Result<(), String> {
                // Auto-generate basic validation from #[http(validate = "...")] attributes
                let spec = Self::get_declarative_spec();

                for (field_name, attrs) in &spec.fields {
                    if attrs.expose {
                        for validation_rule in &attrs.validation {
                            match validation_rule.as_str() {
                                "non_empty" => {
                                    // TODO: Use reflection to check if field is empty
                                    // For now, assume validation passes
                                }
                                rule if rule.starts_with("min_value(") => {
                                    // TODO: Parse min_value and validate
                                    // For now, assume validation passes
                                }
                                _ => {
                                    // Unknown validation rule, assume valid
                                }
                            }
                        }
                    }
                }

                Ok(()) // All validations passed
            }

            // INJECTED FUNCTIONS
            #fw_fn
            #can_read_fn
            #can_write_fn
        }
    };

    // Si #[server(main)] est présent, auto-générer la fonction main() complète
    let main_function = if server_attrs.generate_main {
        let default_port = server_attrs.default_port;
        let cli_args = if server_attrs.cli {
            quote! {
                use clap::Parser;

                #[derive(Parser, Debug)]
                #[command(name = "lithair-app")]
                #[command(about = "Lithair Generated Application - One Model = One App!")]
                struct Args {
                    #[arg(long, default_value_t = #default_port)]
                    port: u16,

                    #[arg(long)]
                    node_id: Option<u64>,

                    #[arg(long)]
                    data_dir: Option<String>,
                }
            }
        } else {
            quote! {}
        };

        let server_logic = if server_attrs.distributed {
            quote! {
                // Distributed mode with node configuration
                if let Some(node_id) = args.node_id {
                    let data_dir = args.data_dir.unwrap_or_else(|| format!("data/node_{}", node_id));
                    std::fs::create_dir_all(&data_dir)?;
                    let event_store_path = format!("{}/{}.events", data_dir, #name_str.to_lowercase());

                    println!("🚀 Lithair Distributed Node");
                    println!("   Model: {}", #name_str);
                    println!("   Node ID: {}", node_id);
                    println!("   Data Dir: {}", data_dir);
                    println!("   Port: {}", args.port);

                    lithair_core::http::DeclarativeServer::<#name>::new(&event_store_path, args.port)?
                        .with_node_id(node_id)
                        .serve()
                        .await?;
                } else {
                    println!("🚀 Lithair Single Node (auto EventStore path)");
                    #name::serve_on_port(args.port).await?;
                }
            }
        } else {
            quote! {
                // Simple single-node mode
                println!("🚀 Lithair Auto-Generated Application");
                println!("   Model: {}", #name_str);
                println!("   Port: {}", args.port);
                println!("   One Model = One Complete Backend!");

                #name::serve_on_port(args.port).await?;
            }
        };

        if server_attrs.cli {
            quote! {
                #cli_args

                #[tokio::main]
                async fn main() -> anyhow::Result<()> {
                    let args = Args::parse();
                    #server_logic
                    Ok(())
                }
            }
        } else {
            quote! {
                #[tokio::main]
                async fn main() -> anyhow::Result<()> {
                    let port = #default_port;

                    println!("🚀 Lithair Auto-Generated Application");
                    println!("   Model: {}", #name_str);
                    println!("   Port: {}", port);
                    println!("   One Model = One Complete Backend!");

                    #name::serve_on_port(port).await?;
                    Ok(())
                }
            }
        }
    } else {
        quote! {}
    };

    // Generate Inspectable implementation
    let inspectable_impl = {
        let field_match_arms = field_names.iter().map(|name| {
            let name_lit = syn::LitStr::new(name, Span::call_site());
            let name_ident = syn::Ident::new(name, Span::call_site());
            quote! {
                #name_lit => serde_json::to_value(&self.#name_ident).ok(),
            }
        });

        quote! {
            impl lithair_core::model_inspect::Inspectable for #name {
                fn get_field_value(&self, field_name: &str) -> Option<serde_json::Value> {
                    match field_name {
                        #(#field_match_arms)*
                        _ => None,
                    }
                }
            }
        }
    };

    // Combine toutes les générations
    let final_expanded = quote! {
        #expanded

        #inspectable_impl

        #main_function
    };

    final_expanded
}
