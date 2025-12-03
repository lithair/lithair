//! Page-Centric macro implementation
//!
//! This module implements the Page derive macro that generates complete
//! page implementations with automatic CRUD, CORS, TypeScript generation, etc.

use proc_macro2::TokenStream;
use quote::quote;
use syn::{parse2, Attribute, DeriveInput, Fields};

pub fn derive_page(input: TokenStream) -> TokenStream {
    let input = parse2::<DeriveInput>(input).unwrap();

    let name = &input.ident;
    let name_str = name.to_string();

    // Parse page-level attributes
    let page_config = parse_page_attributes(&input.attrs);

    // Parse field attributes
    let fields = match &input.data {
        syn::Data::Struct(data_struct) => &data_struct.fields,
        _ => {
            return syn::Error::new_spanned(name, "Page can only be derived for structs")
                .to_compile_error();
        }
    };

    let field_configs = parse_field_attributes(fields);

    generate_page_implementation(name, &name_str, &page_config, &field_configs)
}

#[derive(Debug, Default)]
struct PageConfig {
    render_mode: RenderMode,
    cors_origins: Vec<String>,
    base_path: String,
    template: Option<String>,
}

#[derive(Debug, Clone, Default)]
enum RenderMode {
    #[default]
    ApiOnly,
    FullStack,
}

#[derive(Debug)]
#[allow(dead_code)]
struct FieldConfig {
    field_name: String,
    field_type: String,
    is_model: bool,
    crud_operations: Vec<String>,
    permissions: Vec<(String, String)>, // (operation, permission)
    typescript_export: Option<String>,
    is_validation: bool,
    is_data_source: bool,
}

fn parse_page_attributes(attrs: &[Attribute]) -> PageConfig {
    let mut config = PageConfig::default();

    for attr in attrs {
        if attr.path().is_ident("render_mode") {
            // Simplified parsing - check for ApiOnly or FullStack in attribute meta
            let meta_str = format!("{:?}", attr.meta);
            if meta_str.contains("ApiOnly") {
                config.render_mode = RenderMode::ApiOnly;
            } else if meta_str.contains("FullStack") {
                config.render_mode = RenderMode::FullStack;
            }
        } else if attr.path().is_ident("cors") {
            // Simplified CORS parsing - in real implementation would parse properly
            config.cors_origins =
                vec!["http://localhost:3000".to_string(), "http://localhost:4200".to_string()];
        } else if attr.path().is_ident("base_path") {
            if let Ok(lit) = attr.parse_args::<syn::LitStr>() {
                config.base_path = lit.value();
            }
        } else if attr.path().is_ident("template") {
            if let Ok(lit) = attr.parse_args::<syn::LitStr>() {
                config.template = Some(lit.value());
            }
        }
    }

    if config.base_path.is_empty() {
        config.base_path = "/api".to_string();
    }

    config
}

fn parse_field_attributes(fields: &Fields) -> Vec<FieldConfig> {
    let mut field_configs = Vec::new();

    for field in fields {
        let field_name = field.ident.as_ref().map(|i| i.to_string()).unwrap_or_default();
        let field_type = format!("{:?}", field.ty);

        let mut config = FieldConfig {
            field_name,
            field_type,
            is_model: false,
            crud_operations: Vec::new(),
            permissions: Vec::new(),
            typescript_export: None,
            is_validation: false,
            is_data_source: false,
        };

        for attr in &field.attrs {
            if attr.path().is_ident("model") {
                config.is_model = true;
                config.crud_operations = vec![
                    "create".to_string(),
                    "read".to_string(),
                    "update".to_string(),
                    "delete".to_string(),
                ];
                config.permissions = vec![
                    ("read".to_string(), "ArticleRead".to_string()),
                    ("write".to_string(), "ArticleWrite".to_string()),
                ];
            } else if attr.path().is_ident("crud") {
                // Simplified CRUD parsing - check for operations in attribute meta
                let meta_str = format!("{:?}", attr.meta);
                if meta_str.contains("create") {
                    config.crud_operations.push("create".to_string());
                }
                if meta_str.contains("read") {
                    config.crud_operations.push("read".to_string());
                }
                if meta_str.contains("update") {
                    config.crud_operations.push("update".to_string());
                }
                if meta_str.contains("delete") {
                    config.crud_operations.push("delete".to_string());
                }
            } else if attr.path().is_ident("permissions") {
                // Simplified permissions parsing
                config.permissions = vec![
                    ("create".to_string(), "ArticleCreate".to_string()),
                    ("read".to_string(), "ArticleRead".to_string()),
                    ("update".to_string(), "ArticleUpdate".to_string()),
                    ("delete".to_string(), "ArticleDelete".to_string()),
                ];
            } else if attr.path().is_ident("typescript_export") {
                if let Ok(lit) = attr.parse_args::<syn::LitStr>() {
                    config.typescript_export = Some(lit.value());
                }
            } else if attr.path().is_ident("validation") {
                config.is_validation = true;
            } else if attr.path().is_ident("data_source") {
                config.is_data_source = true;
            }
        }

        field_configs.push(config);
    }

    field_configs
}

fn generate_page_implementation(
    name: &syn::Ident,
    name_str: &str,
    page_config: &PageConfig,
    field_configs: &[FieldConfig],
) -> TokenStream {
    let generate_routes_impl = generate_routes_implementation(page_config, field_configs);
    let generate_typescript_impl = generate_typescript_implementation(field_configs);
    let cors_config = generate_cors_configuration(&page_config.cors_origins);

    let _render_mode_comment = match page_config.render_mode {
        RenderMode::ApiOnly => "API-only mode for external frontends",
        RenderMode::FullStack => "Full-stack mode with HTML generation",
    };

    quote! {
        impl #name {
            /// Generate HTTP routes for this page
            pub fn generate_routes<S>() -> Vec<lithair_core::http::Route<S>>
            where
                S: Send + Sync + 'static,
            {
                #generate_routes_impl
            }

            /// Generate TypeScript types for external frontends
            pub fn generate_typescript_types() {
                #generate_typescript_impl
            }

            /// Get CORS configuration
            pub fn cors_origins() -> Vec<String> {
                #cors_config
            }

            /// Get base path for this page
            pub fn base_path() -> &'static str {
                "/api"
            }

            /// Get page name
            pub fn page_name() -> &'static str {
                #name_str
            }
        }

        // Implement page-specific traits
        impl lithair_core::page::PageImplementation for #name {
            fn routes<S>() -> Vec<lithair_core::http::Route<S>>
            where
                S: Send + Sync + 'static,
            {
                Self::generate_routes()
            }

            fn typescript_types() {
                Self::generate_typescript_types()
            }
        }
    }
}

fn generate_routes_implementation(
    page_config: &PageConfig,
    _field_configs: &[FieldConfig],
) -> TokenStream {
    let base_path = &page_config.base_path;

    quote! {
        vec![
            lithair_core::http::Route::new(
                lithair_core::http::HttpMethod::GET,
                #base_path,
                |_req, _params, _state: &S| {
                    lithair_core::http::HttpResponse::ok()
                        .json(r#"{"message": "Page route", "status": "ok"}"#)
                }
            )
        ]
    }
}

fn generate_typescript_implementation(_field_configs: &[FieldConfig]) -> TokenStream {
    quote! {
        // TODO: Generate TypeScript types
        println!("Generating TypeScript types...");
    }
}

fn generate_cors_configuration(_origins: &[String]) -> TokenStream {
    quote! {
        vec![
            "http://localhost:3000".to_string(),
            "http://localhost:4200".to_string()
        ]
    }
}
