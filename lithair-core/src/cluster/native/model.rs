use super::{state::Change, Reply};
use crate::{http::HttpExposable, lifecycle::RetentionAware, schema::HasSchemaSpec};
use serde_json::{json, Value};
use std::collections::BTreeMap;

type Records = BTreeMap<String, Value>;
type Prepare = fn(&Records, &Mutation) -> Result<(Change, Reply), Reply>;

/// A generated native model with a stable storage identity, independent of the
/// Rust crate/module name and its public route. No per-model event directory is
/// opened in consensus mode; the group owns one atomic application checkpoint.
pub struct Model {
    pub(super) id: String,
    pub(super) path: String,
    pub(super) contract: Value,
    pub(super) prepare: Prepare,
    pub(super) readable: fn(&Value) -> bool,
}

pub(super) struct Mutation {
    pub method: String,
    pub key: Option<String>,
    pub data: Value,
}

impl Model {
    pub fn of<T>(identity: impl Into<String>, path: impl Into<String>) -> anyhow::Result<Self>
    where
        T: HttpExposable + RetentionAware + HasSchemaSpec,
    {
        anyhow::ensure!(
            T::storage_factory().is_none(),
            "Turso/external storage is not supported by native consensus"
        );
        anyhow::ensure!(
            T::native_cluster_compatible(),
            "native consensus requires an ordinary generated serde declaration"
        );
        anyhow::ensure!(
            !T::retention_config().is_configured(),
            "native consensus does not support model eviction/retention policies"
        );
        anyhow::ensure!(
            T::firewall_config().is_none(),
            "configure the server firewall explicitly for native consensus"
        );
        let spec = T::schema_spec();
        anyhow::ensure!(
            spec.foreign_keys.is_empty()
                && spec.fields.values().all(|f| f.foreign_key.is_none()
                    && !f.audited
                    && f.versioned == 0
                    && (f.retention == 0 || f.retention == usize::MAX)
                    && !f.snapshot_only
                    && !f.permissions.owner_field),
            "native consensus does not yet support relations, history or owner policies"
        );
        let id = identity.into();
        let path = path.into();
        anyhow::ensure!(
            valid_id(&id),
            "model identity must contain 1-128 ASCII letters, digits, dots, underscores or hyphens"
        );
        anyhow::ensure!(
            path.starts_with("/api/")
                && !path.ends_with('/')
                && path.split('/').skip(1).all(valid_id),
            "model route must be an unambiguous /api/... path"
        );
        anyhow::ensure!(
            spec.fields.contains_key(T::primary_key_field()),
            "primary key must be declared in the schema"
        );
        // Only sorted field definitions enter the fingerprint. Derived index
        // vectors iterate HashMaps and have no stable order; they are redundant.
        let fields: BTreeMap<_, _> = spec.fields.into_iter().collect();
        let contract = json!({"schema_version":spec.version,"fields":fields,"primary_key":T::primary_key_field()});
        Ok(Self {
            id,
            path,
            contract,
            prepare: prepare::<T>,
            readable: |value| {
                serde_json::from_value::<T>(value.clone()).is_ok_and(|item| item.can_read(&[]))
            },
        })
    }
}
fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && value.len() <= 128
        && value.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}
pub(super) fn reserved_key(value: &str) -> bool {
    matches!(value, "count" | "random-id" | "_schema" | "stream" | "_bulk")
}
fn prepare<T: HttpExposable + HasSchemaSpec>(
    records: &Records,
    request: &Mutation,
) -> Result<(Change, Reply), Reply> {
    let existing = request.key.as_ref().and_then(|id| records.get(id));
    if request.method != "POST" && existing.is_none() {
        return Err(Reply::error(404, "Entity not found"));
    }
    if let Some(existing) = existing {
        let item: T = serde_json::from_value(existing.clone())
            .map_err(|_| Reply::error(500, "Invalid stored model"))?;
        if !item.can_write(&[]) {
            return Err(Reply::error(403, "Insufficient permissions"));
        }
    }
    if request.method == "DELETE" {
        return Ok((
            Change::Delete {
                key: request.key.clone().ok_or_else(|| Reply::error(400, "Missing key"))?,
            },
            Reply::empty(),
        ));
    }
    let input = if request.method == "PATCH" {
        let mut value = existing.cloned().ok_or_else(|| Reply::error(404, "Entity not found"))?;
        let changes = request
            .data
            .as_object()
            .ok_or_else(|| Reply::error(400, "PATCH requires an object"))?;
        let object =
            value.as_object_mut().ok_or_else(|| Reply::error(500, "Invalid stored model"))?;
        for (key, value) in changes {
            object.insert(key.clone(), value.clone());
        }
        value
    } else {
        request.data.clone()
    };
    // Defaults (including generated IDs/timestamps) run exactly once here on
    // the leader. Followers receive only canonical JSON and a durable result.
    let mut item: T =
        serde_json::from_value(input).map_err(|_| Reply::error(400, "Invalid JSON model"))?;
    if !item.can_write(&[]) {
        return Err(Reply::error(403, "Insufficient permissions"));
    }
    item.validate()
        .and_then(|_| item.apply_lifecycle())
        .map_err(|e| Reply::error(400, &e))?;
    let key = item.get_primary_key();
    if !valid_id(&key) || reserved_key(&key) {
        return Err(Reply::error(400, "Primary key must be a nonempty URI-safe identifier"));
    }
    if request.key.as_ref().is_some_and(|id| *id != key) {
        return Err(Reply::error(
            400,
            "The primary key must match the route and cannot be changed",
        ));
    }
    if request.method == "POST" && records.contains_key(&key) {
        return Err(Reply::error(409, "Duplicate primary key"));
    }
    let value =
        serde_json::to_value(item).map_err(|_| Reply::error(400, "Model cannot be serialized"))?;
    for (field, constraints) in T::schema_spec().fields {
        if constraints.immutable && existing.is_some_and(|old| old.get(&field) != value.get(&field))
        {
            return Err(Reply::error(400, &format!("Immutable field: {field}")));
        }
        if constraints.unique
            && value.get(&field).is_some_and(|v| !v.is_null())
            && records
                .iter()
                .any(|(id, old)| id != &key && old.get(&field) == value.get(&field))
        {
            return Err(Reply::error(409, &format!("Unique constraint: {field}")));
        }
    }
    Ok((
        Change::Put { key, value: value.clone() },
        Reply { status: if request.method == "POST" { 201 } else { 200 }, body: Some(value) },
    ))
}
