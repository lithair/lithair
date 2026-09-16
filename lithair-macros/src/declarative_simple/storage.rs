//! Strict, opt-in storage declarations. Native models emit no adapter reference.
use proc_macro2::TokenStream;
use quote::quote;
use syn::{punctuated::Punctuated, DeriveInput, Error, LitInt, LitStr, Path, Token};

pub(super) fn expand(input: &DeriveInput) -> syn::Result<(TokenStream, TokenStream)> {
    let attrs: Vec<_> = input.attrs.iter().filter(|a| a.path().is_ident("storage")).collect();
    let Some(attr) = attrs.first() else { return Ok((quote! {}, quote! {})) };
    if attrs.len() != 1 {
        return Err(Error::new_spanned(attrs[1], "duplicate #[storage(...)]; declare one backend"));
    }
    let mut backend = None;
    let mut collection = None;
    let mut namespace = None;
    let mut filters = None;
    let mut version = None;
    let mut migrations = None;
    attr.parse_nested_meta(|meta| {
        if meta.path.is_ident("native") || meta.path.is_ident("turso") {
            if backend.is_some() { return Err(meta.error("declare exactly one storage backend")); }
            backend = Some(meta.path.is_ident("turso"));
        } else if meta.path.is_ident("collection") || meta.path.is_ident("namespace") {
            let dest = if meta.path.is_ident("collection") { &mut collection } else { &mut namespace };
            if dest.is_some() { return Err(meta.error("duplicate storage option")); }
            let value: LitStr = meta.value()?.parse()?;
            if value.value().is_empty() || value.value().len() > 128 {
                return Err(Error::new_spanned(value, "storage name must contain 1..128 bytes"));
            }
            *dest = Some(value);
        } else if meta.path.is_ident("filters") {
            if filters.is_some() { return Err(meta.error("duplicate filters option")); }
            let content;
            syn::parenthesized!(content in meta.input);
            filters = Some(Punctuated::<LitStr, Token![,]>::parse_terminated(&content)?);
        } else if meta.path.is_ident("version") {
            if version.is_some() { return Err(meta.error("duplicate version option")); }
            let value: LitInt = meta.value()?.parse()?;
            let number = value.base10_parse::<u32>()?;
            if number == 0 { return Err(Error::new_spanned(value, "storage version must be positive")); }
            version = Some(number);
        } else if meta.path.is_ident("migrations") {
            if migrations.is_some() { return Err(meta.error("duplicate migrations option")); }
            let content;
            syn::parenthesized!(content in meta.input);
            migrations = Some(Punctuated::<Path, Token![,]>::parse_terminated(&content)?);
        } else {
            return Err(meta.error("unknown storage option; valid options: native, turso, collection, namespace, filters, version, migrations"));
        }
        Ok(())
    })?;
    let turso = backend
        .ok_or_else(|| Error::new_spanned(attr, "storage requires a backend: native or turso"))?;
    if !turso {
        if collection.is_some()
            || namespace.is_some()
            || filters.is_some()
            || version.is_some()
            || migrations.is_some()
        {
            return Err(Error::new_spanned(
                attr,
                "collection, namespace, filters, version and migrations require the turso backend",
            ));
        }
        return Ok((quote! {}, quote! {}));
    }
    let fields = match &input.data {
        syn::Data::Struct(s) => &s.fields,
        _ => return Err(Error::new_spanned(input, "Turso storage requires a struct")),
    };
    for attr in &input.attrs {
        if attr.path().is_ident("retention") || attr.path().is_ident("schema") {
            return Err(Error::new_spanned(
                attr,
                "Turso storage does not support native retention or schema migration annotations",
            ));
        }
    }
    let mut permissions = Vec::new();
    let mut primary_keys = 0;
    for field in fields {
        let parsed = super::parse_field_attributes(field)?;
        primary_keys += usize::from(parsed.primary_key);
        for attr in &field.attrs {
            if ["lifecycle", "persistence", "relation", "pinned", "rbac"]
                .iter()
                .any(|name| attr.path().is_ident(name))
            {
                return Err(Error::new_spanned(
                    attr,
                    "this annotation requires native storage; it is not supported by Turso",
                ));
            }
            if attr.path().is_ident("db") {
                attr.parse_nested_meta(|meta| {
                    if !meta.path.is_ident("primary_key") {
                        return Err(meta.error("Turso supports only #[db(primary_key)]; use storage filters for SQL equality queries"));
                    }
                    Ok(())
                })?;
            }
        }
        if parsed.owner_field || parsed.serialization.is_some() {
            return Err(Error::new_spanned(field, "Turso does not support owner-field or HTTP serialization annotations; use model permissions and serde"));
        }
        for permission in [parsed.read_permission, parsed.write_permission].into_iter().flatten() {
            if !permissions.contains(&permission) {
                permissions.push(permission);
            }
        }
    }
    if primary_keys > 1 {
        return Err(Error::new_spanned(input, "Turso requires a single primary key"));
    }
    let filters = filters.unwrap_or_default();
    let mut seen = std::collections::HashSet::new();
    for filter in &filters {
        let value = filter.value();
        if ["limit", "offset"].contains(&value.as_str()) {
            return Err(Error::new_spanned(
                filter,
                "limit and offset are reserved pagination parameters",
            ));
        }
        if !seen.insert(value.clone()) {
            return Err(Error::new_spanned(filter, "duplicate filter field"));
        }
        let field = fields
            .iter()
            .find(|f| f.ident.as_ref().is_some_and(|id| *id == value))
            .ok_or_else(|| {
                Error::new_spanned(filter, "filter must name an existing String field")
            })?;
        let is_string = matches!(&field.ty, syn::Type::Path(p) if p.path.segments.last().is_some_and(|s| s.ident == "String"));
        if !is_string
            || value.is_empty()
            || !value.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        {
            return Err(Error::new_spanned(
                filter,
                "SQL equality filters require a String field with an ASCII identifier",
            ));
        }
        // Serialized filter names must match the declared field. Keep this first
        // surface unambiguous instead of guessing arbitrary serde transformations.
        if input.attrs.iter().chain(field.attrs.iter()).any(|a| a.path().is_ident("serde")) {
            return Err(Error::new_spanned(
                filter,
                "SQL filter fields require default serde names and serialization",
            ));
        }
    }
    if migrations.is_some() && version.is_none() {
        return Err(Error::new_spanned(attr, "migrations require an explicit storage version"));
    }
    let version_metadata = if let Some(version) = version {
        let migrations = migrations.unwrap_or_default();
        if migrations.len() as u64 != u64::from(version) - 1 {
            return Err(Error::new_spanned(attr,
                "declare exactly version - 1 migrations, ordered from v1 to v2, v2 to v3, and so on"));
        }
        let migrations: Vec<_> = migrations.iter().collect();
        // Describe serialization, types, key and validation without the Rust
        // struct/module name. Model renames keep their stable collection identity.
        // This deliberately conservative descriptor cannot inspect custom serde
        // or validation function bodies; changes to those require a version bump.
        let serde_attrs = input.attrs.iter().filter(|a| a.path().is_ident("serde"));
        let schema_fields = fields.iter().map(|field| {
            let name = &field.ident;
            let ty = &field.ty;
            let attrs = field
                .attrs
                .iter()
                .filter(|a| ["serde", "db", "http"].iter().any(|name| a.path().is_ident(name)));
            quote! { #(#attrs)* #name: #ty }
        });
        quote! {
            const VERSION: u32 = #version;
            const SCHEMA: &'static str = stringify!(#(#serde_attrs)* { #(#schema_fields),* });
            const MIGRATIONS: &'static [::lithair_turso::Migration] = &[#(#migrations),*];
        }
    } else {
        quote! {}
    };
    let name = &input.ident;
    let collection = collection.unwrap_or_else(|| LitStr::new(&name.to_string(), name.span()));
    let namespace = namespace.unwrap_or_else(|| LitStr::new("default", name.span()));
    let filters: Vec<_> = filters.iter().collect();
    let implementation = quote! {
        impl ::lithair_turso::SqlModel for #name {
            #version_metadata
            const COLLECTION: &'static str = #collection;
            const NAMESPACE: &'static str = #namespace;
            const FILTER_FIELDS: &'static [&'static str] = &[#(#filters),*];
            const PERMISSIONS: &'static [&'static str] = &[#(#permissions),*];
        }
    };
    let hook = quote! {
        fn storage_factory() -> Option<::lithair_core::app::ModelFactory> {
            Some(::lithair_turso::model_factory::<Self>())
        }
    };
    Ok((implementation, hook))
}
