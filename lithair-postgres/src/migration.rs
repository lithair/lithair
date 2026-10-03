//! Startup document evolution, isolated by namespace/collection and transactional.
//! Same contract as `lithair-turso`; several nodes preparing one partition at
//! once are serialized by an advisory lock and migrate it exactly once.
use crate::{Error, Result, SqlModel, Store, MAX_DOCUMENT_BYTES, MAX_PAGE_SIZE};
use serde_json::Value;
use std::panic::{catch_unwind, AssertUnwindSafe};
use tokio_postgres::Transaction;

/// A trusted synchronous document transformation. Entry zero in `MIGRATIONS`
/// upgrades version 1 to 2, entry one upgrades 2 to 3, and so on. Keep the full
/// history in order. Callbacks must be deterministic and have no external side
/// effects: a serialization retry may run them again, and rollback cannot undo
/// file, network or process changes.
pub type Migration = fn(&mut Value) -> Result<()>;

pub(crate) fn validate_declaration<T: SqlModel>() -> Result<()> {
    if T::VERSION == 0 || T::MIGRATIONS.len() as u64 != u64::from(T::VERSION) - 1 {
        return Err(Error::Schema(
            "declare a positive version and every migration from version 1".into(),
        ));
    }
    if T::VERSION > 1 && T::SCHEMA.is_empty() {
        return Err(Error::Schema("versioned models require a nonempty schema descriptor".into()));
    }
    Ok(())
}

impl<T: SqlModel> Store<T> {
    /// Prepare a partition before use. The generated HTTP factory awaits this
    /// before serving; repository operations also prepare lazily. All pending
    /// steps, document validation and metadata commit together. An admitted
    /// migration finishes even if its caller is dropped.
    pub async fn prepare(&self) -> Result<()> {
        self.ensure_prepared().await?;
        self.database.transaction(|tx, _| Box::pin(self.check_schema(tx))).await
    }

    pub(crate) async fn ensure_prepared(&self) -> Result<()> {
        if self.cache.is_some() {
            // Copies are only trusted while invalidations arrive (RFC 304).
            self.database.caches.ensure_listening().await?;
        }
        self.prepared
            .get_or_try_init(|| async {
                let store = self.clone();
                tokio::spawn(async move {
                    let store = &store;
                    let changed = store
                        .database
                        .transaction(|tx, _| Box::pin(store.prepare_transaction(tx)))
                        .await?;
                    if changed {
                        store.database.caches.invalidate(&store.namespace, T::COLLECTION, None);
                    }
                    Ok::<_, Error>(())
                })
                .await?
            })
            .await?;
        // Another process or handle may have advanced the partition since this
        // handle prepared. Every operation checks inside its own transaction.
        Ok(())
    }

    async fn stored_schema(&self, tx: &Transaction<'_>) -> Result<Option<(u32, String)>> {
        let row = tx
            .query_opt(
                "SELECT version, schema FROM lithair.schemas WHERE namespace = $1 AND collection = $2",
                &[&self.namespace, &T::COLLECTION],
            )
            .await?;
        match row {
            Some(row) => {
                let version = u32::try_from(row.get::<_, i32>(0))
                    .ok()
                    .filter(|v| *v > 0)
                    .ok_or_else(|| Error::Schema("invalid stored schema version".into()))?;
                Ok(Some((version, row.get(1))))
            }
            None => Ok(None),
        }
    }

    pub(crate) async fn check_schema(&self, tx: &Transaction<'_>) -> Result<()> {
        match self.stored_schema(tx).await? {
            Some((version, schema)) if version == T::VERSION && schema == T::SCHEMA => Ok(()),
            _ => Err(Error::Schema(format!(
                "{}/{} is incompatible with this model handle (expected version {}); reopen with the current model",
                self.namespace, T::COLLECTION, T::VERSION,
            ))),
        }
    }

    /// `true` when the partition changed (migrated or newly recorded); its
    /// copies on every node are then flushed by a notification.
    async fn prepare_transaction(&self, tx: &Transaction<'_>) -> Result<bool> {
        // One preparer per partition across all processes.
        tx.execute(
            "SELECT pg_advisory_xact_lock(hashtextextended($1 || '/' || $2, 0))",
            &[&self.namespace, &T::COLLECTION],
        )
        .await?;
        let stored = self.stored_schema(tx).await?;
        if let Some((version, schema)) = &stored {
            if *version > T::VERSION {
                return Err(Error::Schema(format!(
                    "{}/{} is at version {version}; refusing downgrade to {}",
                    self.namespace,
                    T::COLLECTION,
                    T::VERSION,
                )));
            }
            if *version == T::VERSION {
                if schema == T::SCHEMA {
                    return Ok(false);
                }
                if !schema.is_empty() {
                    return Err(Error::Schema(format!(
                        "{}/{} schema changed without increasing version {}",
                        self.namespace,
                        T::COLLECTION,
                        T::VERSION,
                    )));
                }
            }
        }
        let from = stored.as_ref().map_or(1, |(version, _)| *version);
        if !T::SCHEMA.is_empty() {
            self.migrate_documents(tx, from).await?;
        }
        tx.execute(
            "INSERT INTO lithair.schemas (namespace, collection, version, schema)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT (namespace, collection)
             DO UPDATE SET version = excluded.version, schema = excluded.schema",
            &[&self.namespace, &T::COLLECTION, &(T::VERSION as i32), &T::SCHEMA],
        )
        .await?;
        crate::cache::notify(tx, &self.namespace, T::COLLECTION, None).await?;
        Ok(true)
    }

    async fn migrate_documents(&self, tx: &Transaction<'_>, from: u32) -> Result<()> {
        let mut after: Option<String> = None;
        loop {
            // The primary key never changes, so advancing by ID visits each
            // original record once, without OFFSET.
            let page = tx
                .query(
                    "SELECT id, body FROM lithair.documents
                     WHERE namespace = $1 AND collection = $2 AND ($3::text IS NULL OR id > $3)
                     ORDER BY id LIMIT $4",
                    &[&self.namespace, &T::COLLECTION, &after, &i64::from(MAX_PAGE_SIZE)],
                )
                .await?;
            if page.is_empty() {
                return Ok(());
            }
            for row in page {
                let id: String = row.get(0);
                let migrated = self.migrate_document(&id, row.get(1), from)?;
                tx.execute(
                    "UPDATE lithair.documents SET body = $4
                     WHERE namespace = $1 AND collection = $2 AND id = $3",
                    &[&self.namespace, &T::COLLECTION, &id, &migrated],
                )
                .await?;
                after = Some(id);
            }
        }
    }

    fn migrate_document(&self, id: &str, mut document: Value, from: u32) -> Result<Value> {
        // Catch user callback/validation panics while the transaction is in
        // scope, so rollback is explicit.
        let result = catch_unwind(AssertUnwindSafe(|| -> Result<Value> {
            if serde_json::to_vec(&document)?.len() > MAX_DOCUMENT_BYTES {
                return Err(Error::Schema("stored document exceeds 1 MiB".into()));
            }
            for migrate in &T::MIGRATIONS[(from - 1) as usize..] {
                migrate(&mut document)?;
            }
            if !document.is_object() {
                return Err(Error::Schema("migration must produce a JSON object".into()));
            }
            if serde_json::to_vec(&document)?.len() > MAX_DOCUMENT_BYTES {
                return Err(Error::Schema("migrated document exceeds 1 MiB".into()));
            }
            let value: T = serde_json::from_value(document.clone())?;
            if value.get_primary_key() != id {
                return Err(Error::Schema("migration cannot change the primary key".into()));
            }
            value.validate().map_err(Error::Validation)?;
            // Persist the transformed JSON, not a T roundtrip: serde may ignore
            // fields still needed by a future migration.
            Ok(document)
        }));
        match result {
            Ok(Ok(document)) => Ok(document),
            Ok(Err(error)) => Err(Error::Schema(format!(
                "{}/{} record {id}: {error}",
                self.namespace,
                T::COLLECTION,
            ))),
            Err(_) => Err(Error::Schema(format!(
                "{}/{} record {id}: migration or validation panicked",
                self.namespace,
                T::COLLECTION,
            ))),
        }
    }
}
