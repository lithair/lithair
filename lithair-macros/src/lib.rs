//! Procedural macros for Lithair framework
//!
//! This crate provides the macro implementations that generate boilerplate code
//! for Lithair applications, making the framework easy and pleasant to use.

#![allow(unexpected_cfgs)]
use proc_macro::TokenStream;

mod declarative_simple;
mod declarative_types;
mod lifecycle;
mod page;
mod rbac_role;
use quote::quote;
use syn::{parse_macro_input, DeriveInput, ItemImpl};

/// Derive macro for generating lifecycle-aware data models
///
/// This macro automatically generates:
/// - LifecycleAware trait implementation
/// - Field-level lifecycle policies
/// - Integration with Lithair lifecycle engine
///
/// # Example
///
/// ```rust,ignore
/// use lithair_macros::LifecycleAware;
///
/// #[derive(LifecycleAware)]
/// struct Product {
///     #[lifecycle(immutable)]
///     id: u64,
///     #[lifecycle(versions = 3, unique)]
///     name: String,
///     #[lifecycle(audited)]
///     price: f64,
/// }
/// ```
#[proc_macro_derive(LifecycleAware, attributes(lifecycle))]
pub fn derive_lifecycle_aware(input: TokenStream) -> TokenStream {
    lifecycle::derive_lifecycle_aware(input.into()).into()
}

/// Derive macro for DeclarativeModel trait
///
/// This macro parses field attributes and generates unified specifications
/// for database constraints, lifecycle policies, HTTP exposure, RBAC permissions,
/// and binary storage optimizations.
///
/// # Example
///
/// ```rust,ignore
/// #[derive(DeclarativeModel)]
/// struct Product {
///     #[db(primary_key, indexed)]
///     #[lifecycle(immutable)]
///     #[http(expose)]
///     #[persistence(binary_storage)]  // Lithair SUPERPOWER - binary mode
///     id: u64,
///     
///     #[db(unique, indexed)]
///     #[lifecycle(audited, retention = 365)]
///     #[http(expose, validate = "non_empty")]
///     #[permission(read = "ProductReadAny", write = "ProductWriteAny")]
///     #[persistence(binary_storage, compression = "lz4")]  // Binary with compression
///     name: String,
/// }
/// ```
#[proc_macro_derive(
    DeclarativeModel,
    attributes(db, lifecycle, http, permission, rbac, relation, persistence, server, firewall)
)]
pub fn derive_declarative_model(input: TokenStream) -> TokenStream {
    declarative_simple::derive_declarative_model(input.into()).into()
}

/// Attribute macro for lifecycle field annotations (disabled by default to keep field attributes inert)
#[cfg(feature = "attr_macros")]
#[proc_macro_attribute]
pub fn lifecycle(_args: TokenStream, input: TokenStream) -> TokenStream {
    input
}

/// Attribute macro for database field annotations (disabled by default)
#[cfg(feature = "attr_macros")]
#[proc_macro_attribute]
pub fn db(_args: TokenStream, input: TokenStream) -> TokenStream {
    input
}

/// Attribute macro for HTTP field annotations (disabled by default)
#[cfg(feature = "attr_macros")]
#[proc_macro_attribute]
pub fn http(_args: TokenStream, input: TokenStream) -> TokenStream {
    input
}

/// Attribute macro for permission field annotations (disabled by default)
#[cfg(feature = "attr_macros")]
#[proc_macro_attribute]
pub fn permission(_args: TokenStream, input: TokenStream) -> TokenStream {
    input
}

/// Attribute macro for RBAC field annotations (disabled by default)
#[cfg(feature = "attr_macros")]
#[proc_macro_attribute]
pub fn rbac(_args: TokenStream, input: TokenStream) -> TokenStream {
    input
}

/// Attribute macro for persistence optimization annotations (disabled by default)
///
/// Supports binary storage, compression, and serialization optimizations:
/// - binary_storage: Enable binary serialization instead of JSON
/// - compression: Compression algorithm ("lz4", "zstd", "rle")
/// - decimal_precision: Use Decimal type for financial precision
///
/// # Example
#[cfg(feature = "attr_macros")]
#[proc_macro_attribute]
pub fn persistence(_args: TokenStream, input: TokenStream) -> TokenStream {
    input
}

/// Attribute macro for server configuration annotations (disabled by default)
#[cfg(feature = "attr_macros")]
#[proc_macro_attribute]
pub fn server(_args: TokenStream, input: TokenStream) -> TokenStream {
    input
}

/// Derive macro for generating events and serialization for data models
///
/// This macro automatically generates:
/// - Event types for Create, Update, Delete operations
/// - Serialization implementations
///
/// # Example
///
/// ```rust,ignore
/// use lithair_macros::RaftstoneModel;
///
/// #[derive(RaftstoneModel)]
/// struct Product {
///     id: u64,
///     name: String,
///     price: f64,
/// }
/// ```
#[proc_macro_derive(RaftstoneModel)]
pub fn derive_lithair_model(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    let name = &input.ident;
    let name_str = name.to_string();

    // TODO: Extract struct fields and generate appropriate events
    let _fields = match &input.data {
        syn::Data::Struct(data_struct) => &data_struct.fields,
        _ => {
            return syn::Error::new_spanned(name, "RaftstoneModel can only be derived for structs")
                .to_compile_error()
                .into();
        }
    };

    // For now, generate a placeholder implementation
    let expanded = quote! {
        // TODO: Generate actual event types and implementations
        impl lithair_core::macros::GeneratedModel for #name {
            fn model_name() -> &'static str {
                #name_str
            }

            fn field_names() -> &'static [&'static str] {
                // TODO: Extract actual field names
                &[]
            }
        }
    };

    TokenStream::from(expanded)
}

/// Attribute macro for generating HTTP routes from API implementations
///
/// This macro automatically generates:
/// - HTTP route handlers
/// - JSON request/response serialization
/// - Route registration code
///
/// # Example
///
/// ```rust,ignore
/// use lithair_core::RaftstoneApi;
///
/// #[RaftstoneApi]
/// impl MyApp {
///     fn create_user(&mut self, name: String, email: String) -> Result<User, String> {
///         // Your business logic here
///     }
///     
///     fn get_users(&self) -> Vec<User> {
///         // Your query logic here
///     }
/// }
///
/// // This generates:
/// // - POST /users route for create_user
/// // - GET /users route for get_users
/// // - JSON serialization handling
/// ```
#[proc_macro_attribute]
pub fn lithair_api(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as ItemImpl);

    // For now, just return the original implementation
    // TODO: Generate route registration and HTTP handler code

    let expanded = quote! {
        #input

        // TODO: Generate route registration implementation
        // impl lithair_core::macros::GeneratedApi<SomeStateType> for SomeType {
        //     fn routes() -> Vec<lithair_core::http::Route<SomeStateType>> {
        //         vec![]
        //     }
        // }
    };

    TokenStream::from(expanded)
}

/// Derive macro for schema evolution and migration support (placeholder)
#[proc_macro_derive(SchemaEvolution, attributes(schema, migration))]
pub fn derive_schema_evolution(input: TokenStream) -> TokenStream {
    // Placeholder implementation - pass through unchanged
    input
}

/// Derive macro for Page-Centric development
///
/// This macro generates complete page implementations with:
/// - Automatic CRUD API generation
/// - CORS configuration for external frontends
/// - TypeScript type generation
/// - RBAC integration
/// - Validation handling
///
/// # Example
///
/// ```rust,ignore
/// #[derive(Page)]
/// #[render_mode(ApiOnly)]
/// #[cors(origins = ["http://localhost:3000"])]
/// #[base_path("/api/articles")]
/// struct ArticlePage {
///     #[model]
///     article: Article,
///     
///     #[crud(create, read, update, delete)]
///     #[permissions(read = "ArticleRead", write = "ArticleWrite")]
///     operations: Auto,
/// }
/// ```
#[proc_macro_derive(
    Page,
    attributes(
        render_mode,
        cors,
        base_path,
        model,
        crud,
        permissions,
        typescript_export,
        validation,
        data_source,
        template
    )
)]
pub fn derive_page(input: TokenStream) -> TokenStream {
    page::derive_page(input.into()).into()
}

/// Derive macro for RBAC roles with declarative permissions
///
/// This macro automatically generates the `has_permission` method
/// based on `#[permissions(...)]` attributes on enum variants.
///
/// # Example
///
/// ```rust,ignore
/// #[derive(RbacRole)]
/// #[permission_type(ProductPermission)]
/// enum UserRole {
///     #[permissions(ProductRead)]
///     Customer,
///     
///     #[permissions(ProductRead, ProductWrite)]
///     Employee,
///     
///     #[permissions(all)]
///     Administrator,
/// }
/// ```
#[proc_macro_derive(RbacRole, attributes(permissions, permission, permission_type))]
pub fn derive_rbac_role(input: TokenStream) -> TokenStream {
    rbac_role::derive_rbac_role(input.into())
        .unwrap_or_else(|e| e.to_compile_error())
        .into()
}

// TODO: Add more helper macros as needed
// - #[RaftstoneEvent] for custom events
// - #[RaftstoneMiddleware] for middleware
// - #[RaftstonePlugin] for plugins
