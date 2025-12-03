//! Shared types for declarative model macros

use std::collections::HashMap;

/// Parsed field attributes from declarative annotations
#[derive(Debug, Clone, Default)]
#[allow(dead_code)]
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

/// Declarative specification for a model
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct DeclarativeSpec {
    pub model_name: String,
    pub fields: HashMap<String, ParsedFieldAttributes>,
}
