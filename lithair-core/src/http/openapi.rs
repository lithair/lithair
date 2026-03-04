//! OpenAPI 3.1 specification auto-generation from DeclarativeModel definitions
//!
//! Generates a complete OpenAPI spec at runtime using `ModelSpec` metadata
//! extracted from registered models. Each model produces CRUD endpoints
//! with proper schemas, types, and permission annotations.

use crate::schema::{FieldConstraints, ModelSpec};
use serde_json::{json, Value};

/// Information about a registered model needed for OpenAPI generation
pub struct OpenApiModelInfo {
    /// Model name (e.g. "Product")
    pub name: String,
    /// Base API path (e.g. "/api/products")
    pub base_path: String,
    /// Schema specification with field constraints and types
    pub spec: ModelSpec,
}

/// Map a Rust type string to OpenAPI type + format
fn rust_type_to_openapi(rust_type: &str) -> (String, Option<String>) {
    // Strip Option<> wrapper
    let inner = if rust_type.starts_with("Option<") && rust_type.ends_with('>') {
        &rust_type[7..rust_type.len() - 1]
    } else {
        rust_type
    };

    match inner {
        "String" | "string" => ("string".into(), None),
        "Uuid" | "uuid::Uuid" => ("string".into(), Some("uuid".into())),
        "i8" | "i16" | "i32" | "u8" | "u16" | "u32" => ("integer".into(), Some("int32".into())),
        "i64" | "u64" | "isize" | "usize" => ("integer".into(), Some("int64".into())),
        "i128" | "u128" => ("integer".into(), Some("int128".into())),
        "f32" => ("number".into(), Some("float".into())),
        "f64" => ("number".into(), Some("double".into())),
        "bool" => ("boolean".into(), None),
        "DateTime" | "NaiveDateTime" | "chrono::DateTime<Utc>"
        | "chrono::DateTime<chrono::Utc>" => ("string".into(), Some("date-time".into())),
        "NaiveDate" | "chrono::NaiveDate" => ("string".into(), Some("date".into())),
        "Vec<u8>" => ("string".into(), Some("byte".into())),
        _ if inner.starts_with("Vec<") => ("array".into(), None),
        _ if inner.starts_with("HashMap<") => ("object".into(), None),
        _ => ("string".into(), None),
    }
}

/// Build the JSON schema for a single field
fn field_to_schema(field_name: &str, constraints: &FieldConstraints) -> Value {
    let (type_str, format) = constraints
        .field_type
        .as_ref()
        .map(|t| rust_type_to_openapi(t))
        .unwrap_or_else(|| ("string".into(), None));

    let is_nullable = constraints.nullable
        || constraints
            .field_type
            .as_ref()
            .map(|t| t.starts_with("Option<"))
            .unwrap_or(false);

    let mut schema = json!({ "type": type_str });

    if let Some(fmt) = format {
        schema["format"] = json!(fmt);
    }

    if is_nullable {
        schema["nullable"] = json!(true);
    }

    // Add validation hints
    for rule in &constraints.validation_rules {
        if let Some(min) = rule.strip_prefix("min_length:") {
            if let Ok(n) = min.parse::<u64>() {
                schema["minLength"] = json!(n);
            }
        } else if let Some(max) = rule.strip_prefix("max_length:") {
            if let Ok(n) = max.parse::<u64>() {
                schema["maxLength"] = json!(n);
            }
        } else if rule == "email" {
            schema["format"] = json!("email");
        } else if rule == "url" || rule == "uri" {
            schema["format"] = json!("uri");
        }
    }

    // Add description for special fields
    if constraints.primary_key {
        schema["description"] = json!(format!("Primary key for {}", field_name));
        schema["readOnly"] = json!(true);
    }

    schema
}

/// Build the component schema for a model
fn model_to_schema(info: &OpenApiModelInfo) -> Value {
    let mut properties = serde_json::Map::new();
    let mut required = Vec::new();

    // Sort fields for deterministic output
    let mut fields: Vec<_> = info.spec.fields.iter().collect();
    fields.sort_by_key(|(name, _)| (*name).clone());

    for (field_name, constraints) in &fields {
        properties.insert(
            field_name.to_string(),
            field_to_schema(field_name, constraints),
        );

        // Non-nullable and non-primary-key fields are required for creation
        if !constraints.nullable
            && !constraints.primary_key
            && !constraints
                .field_type
                .as_ref()
                .map(|t| t.starts_with("Option<"))
                .unwrap_or(false)
        {
            required.push(json!(field_name));
        }
    }

    let mut schema = json!({
        "type": "object",
        "properties": properties,
    });

    if !required.is_empty() {
        schema["required"] = json!(required);
    }

    schema
}

/// Build CRUD path operations for a model
fn model_to_paths(info: &OpenApiModelInfo) -> Vec<(String, Value)> {
    let model_name = &info.name;
    let base = &info.base_path;
    let schema_ref = format!("#/components/schemas/{}", model_name);
    let tag = model_name.clone();

    // Determine read permission annotation
    let read_perm: Option<String> = info
        .spec
        .fields
        .values()
        .filter_map(|c| c.permissions.read_permission.as_ref())
        .next()
        .cloned();

    let perm_note = read_perm
        .as_ref()
        .map(|p| format!(" Requires `{}` permission.", p))
        .unwrap_or_default();

    let mut paths = Vec::new();

    // Collection endpoint: GET (list) + POST (create)
    let collection_path = base.clone();
    paths.push((
        collection_path,
        json!({
            "get": {
                "tags": [&tag],
                "summary": format!("List all {}", model_name),
                "description": format!("Returns all {} items.{}", model_name, perm_note),
                "operationId": format!("list{}", model_name),
                "parameters": [
                    { "name": "skip", "in": "query", "schema": { "type": "integer", "default": 0 }, "description": "Number of items to skip" },
                    { "name": "take", "in": "query", "schema": { "type": "integer" }, "description": "Maximum number of items to return" },
                    { "name": "sort", "in": "query", "schema": { "type": "string" }, "description": "Sort field (prefix with - for descending)" }
                ],
                "responses": {
                    "200": {
                        "description": "Paginated list",
                        "content": {
                            "application/json": {
                                "schema": {
                                    "type": "object",
                                    "properties": {
                                        "data": { "type": "array", "items": { "$ref": &schema_ref } },
                                        "total": { "type": "integer" },
                                        "skip": { "type": "integer" },
                                        "take": { "type": "integer", "nullable": true },
                                        "has_more": { "type": "boolean" }
                                    }
                                }
                            }
                        }
                    }
                }
            },
            "post": {
                "tags": [&tag],
                "summary": format!("Create a new {}", model_name),
                "operationId": format!("create{}", model_name),
                "requestBody": {
                    "required": true,
                    "content": {
                        "application/json": {
                            "schema": { "$ref": &schema_ref }
                        }
                    }
                },
                "responses": {
                    "201": {
                        "description": "Created",
                        "content": {
                            "application/json": {
                                "schema": { "$ref": &schema_ref }
                            }
                        }
                    },
                    "400": { "description": "Invalid input" },
                    "403": { "description": "Insufficient permissions" }
                }
            }
        }),
    ));

    // Item endpoint: GET + PUT + DELETE
    let item_path = format!("{}/{{id}}", base);
    paths.push((
        item_path,
        json!({
            "get": {
                "tags": [&tag],
                "summary": format!("Get {} by ID", model_name),
                "operationId": format!("get{}", model_name),
                "parameters": [
                    { "name": "id", "in": "path", "required": true, "schema": { "type": "string" } }
                ],
                "responses": {
                    "200": {
                        "description": "Found",
                        "content": {
                            "application/json": {
                                "schema": { "$ref": &schema_ref }
                            }
                        }
                    },
                    "404": { "description": "Not found" }
                }
            },
            "put": {
                "tags": [&tag],
                "summary": format!("Update {}", model_name),
                "operationId": format!("update{}", model_name),
                "parameters": [
                    { "name": "id", "in": "path", "required": true, "schema": { "type": "string" } }
                ],
                "requestBody": {
                    "required": true,
                    "content": {
                        "application/json": {
                            "schema": { "$ref": &schema_ref }
                        }
                    }
                },
                "responses": {
                    "200": {
                        "description": "Updated",
                        "content": {
                            "application/json": {
                                "schema": { "$ref": &schema_ref }
                            }
                        }
                    },
                    "404": { "description": "Not found" },
                    "400": { "description": "Invalid input" }
                }
            },
            "delete": {
                "tags": [&tag],
                "summary": format!("Delete {}", model_name),
                "operationId": format!("delete{}", model_name),
                "parameters": [
                    { "name": "id", "in": "path", "required": true, "schema": { "type": "string" } }
                ],
                "responses": {
                    "200": { "description": "Deleted" },
                    "404": { "description": "Not found" }
                }
            }
        }),
    ));

    paths
}

/// Generate a complete OpenAPI 3.1 specification from registered models
pub fn generate_openapi_spec(models: &[OpenApiModelInfo]) -> Value {
    let mut paths = serde_json::Map::new();
    let mut schemas = serde_json::Map::new();

    for model in models {
        // Generate schema component
        schemas.insert(model.name.clone(), model_to_schema(model));

        // Generate path operations
        for (path, operations) in model_to_paths(model) {
            paths.insert(path, operations);
        }
    }

    json!({
        "openapi": "3.1.0",
        "info": {
            "title": "Lithair API",
            "description": "Auto-generated API documentation from DeclarativeModel definitions",
            "version": "0.2.0"
        },
        "paths": paths,
        "components": {
            "schemas": schemas
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{FieldPermissions, ModelSpec};
    use std::collections::HashMap;

    fn sample_model() -> OpenApiModelInfo {
        let mut fields = HashMap::new();
        fields.insert(
            "id".to_string(),
            FieldConstraints {
                primary_key: true,
                unique: true,
                indexed: true,
                foreign_key: None,
                nullable: false,
                immutable: true,
                audited: false,
                versioned: 0,
                retention: 0,
                snapshot_only: false,
                validation_rules: vec![],
                permissions: FieldPermissions {
                    read_permission: Some("Public".to_string()),
                    write_permission: None,
                    owner_field: false,
                },
                default_value: None,
                field_type: Some("Uuid".to_string()),
            },
        );
        fields.insert(
            "title".to_string(),
            FieldConstraints {
                primary_key: false,
                unique: false,
                indexed: false,
                foreign_key: None,
                nullable: false,
                immutable: false,
                audited: false,
                versioned: 0,
                retention: 0,
                snapshot_only: false,
                validation_rules: vec!["min_length:1".to_string()],
                permissions: FieldPermissions {
                    read_permission: Some("Public".to_string()),
                    write_permission: None,
                    owner_field: false,
                },
                default_value: None,
                field_type: Some("String".to_string()),
            },
        );
        fields.insert(
            "done".to_string(),
            FieldConstraints {
                primary_key: false,
                unique: false,
                indexed: false,
                foreign_key: None,
                nullable: false,
                immutable: false,
                audited: false,
                versioned: 0,
                retention: 0,
                snapshot_only: false,
                validation_rules: vec![],
                permissions: FieldPermissions {
                    read_permission: None,
                    write_permission: None,
                    owner_field: false,
                },
                default_value: None,
                field_type: Some("bool".to_string()),
            },
        );

        OpenApiModelInfo {
            name: "Todo".to_string(),
            base_path: "/api/todos".to_string(),
            spec: ModelSpec {
                model_name: "Todo".to_string(),
                version: 1,
                fields,
                indexes: vec![],
                foreign_keys: vec![],
            },
        }
    }

    #[test]
    fn test_generate_openapi_spec() {
        let spec = generate_openapi_spec(&[sample_model()]);
        assert_eq!(spec["openapi"], "3.1.0");
        assert!(spec["paths"]["/api/todos"]["get"].is_object());
        assert!(spec["paths"]["/api/todos"]["post"].is_object());
        assert!(spec["paths"]["/api/todos/{id}"]["get"].is_object());
        assert!(spec["paths"]["/api/todos/{id}"]["put"].is_object());
        assert!(spec["paths"]["/api/todos/{id}"]["delete"].is_object());
        assert!(spec["components"]["schemas"]["Todo"].is_object());
    }

    #[test]
    fn test_rust_type_mapping() {
        assert_eq!(rust_type_to_openapi("String"), ("string".into(), None));
        assert_eq!(
            rust_type_to_openapi("Uuid"),
            ("string".into(), Some("uuid".into()))
        );
        assert_eq!(
            rust_type_to_openapi("i64"),
            ("integer".into(), Some("int64".into()))
        );
        assert_eq!(rust_type_to_openapi("bool"), ("boolean".into(), None));
        assert_eq!(
            rust_type_to_openapi("Option<String>"),
            ("string".into(), None)
        );
        assert_eq!(
            rust_type_to_openapi("f64"),
            ("number".into(), Some("double".into()))
        );
    }

    #[test]
    fn test_field_schema_nullable() {
        let constraints = FieldConstraints {
            primary_key: false,
            unique: false,
            indexed: false,
            foreign_key: None,
            nullable: true,
            immutable: false,
            audited: false,
            versioned: 0,
            retention: 0,
            snapshot_only: false,
            validation_rules: vec![],
            permissions: FieldPermissions {
                read_permission: None,
                write_permission: None,
                owner_field: false,
            },
            default_value: None,
            field_type: Some("Option<String>".to_string()),
        };

        let schema = field_to_schema("notes", &constraints);
        assert_eq!(schema["type"], "string");
        assert_eq!(schema["nullable"], true);
    }
}
