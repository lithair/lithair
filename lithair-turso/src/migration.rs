//! Startup document evolution, isolated by namespace/collection and transactional.
use crate::{Error, Result, SqlModel, Store, MAX_DOCUMENT_BYTES, MAX_PAGE_SIZE};
use serde_json::Value;
use std::panic::{catch_unwind, AssertUnwindSafe};
use turso::{params, Connection};

/// A trusted synchronous document transformation. Entry zero in `MIGRATIONS`
/// upgrades version 1 to 2, entry one upgrades 2 to 3, and so on. Keep the full
/// history in order. Callbacks must be deterministic and have no external side
/// effects: SQL rollback cannot undo file, network or process state changes.
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
    /// before serving. Repository operations also prepare lazily. All pending
    /// steps, document validation and metadata commit together, with bounded
    /// keyset pages. An admitted migration finishes even if its caller is dropped.
    /// Await it before stopping the runtime. No user permission hooks run here:
    /// migrations are trusted application code, not requests from a session.
    pub async fn prepare(&self) -> Result<()> {
        self.ensure_prepared().await?;
        let connection = self.database.connection.lock().await;
        self.check_schema(&connection).await
    }

    pub(crate) async fn ensure_prepared(&self) -> Result<()> {
        self.prepared
            .get_or_try_init(|| async {
                let store = self.clone();
                tokio::spawn(async move {
                    let mut connection = store.database.connection.lock().await;
                    let transaction = connection
                        .transaction_with_behavior(
                            turso::transaction::TransactionBehavior::Immediate,
                        )
                        .await?;
                    if let Err(error) = store.prepare_transaction(&transaction).await {
                        transaction.rollback().await?;
                        return Err(error);
                    }
                    transaction.commit().await?;
                    Ok(())
                })
                .await?
            })
            .await?;
        // A different typed store may have advanced the partition since this
        // handle prepared. Every operation checks under the same connection lock.
        Ok(())
    }

    async fn stored_schema(&self, connection: &Connection) -> Result<Option<(u32, String)>> {
        let mut rows = connection.query(
            "SELECT version, schema FROM lithair_schemas_v1 WHERE namespace = ?1 AND model = ?2",
            params![self.namespace.as_str(), T::COLLECTION],
        ).await?;
        match rows.next().await? {
            Some(row) => {
                let version = u32::try_from(row.get::<i64>(0)?)
                    .ok()
                    .filter(|v| *v > 0)
                    .ok_or_else(|| Error::Schema("invalid stored schema version".into()))?;
                Ok(Some((version, row.get(1)?)))
            }
            None => Ok(None),
        }
    }

    pub(crate) async fn check_schema(&self, connection: &Connection) -> Result<()> {
        match self.stored_schema(connection).await? {
            Some((version, schema)) if version == T::VERSION && schema == T::SCHEMA => Ok(()),
            _ => Err(Error::Schema(format!(
                "{}/{} is incompatible with this model handle (expected version {}); reopen with the current model",
                self.namespace, T::COLLECTION, T::VERSION,
            ))),
        }
    }

    async fn prepare_transaction(&self, connection: &Connection) -> Result<()> {
        let stored = self.stored_schema(connection).await?;
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
                    return Ok(());
                }
                // An unversioned legacy partition can adopt a declaration after
                // validating all records. A tracked schema cannot be replaced.
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
        // A newly tracked unversioned store keeps the 0.1 behavior. Explicit
        // declarations validate legacy data even when adopting version 1.
        if !T::SCHEMA.is_empty() {
            self.migrate_documents(connection, from).await?;
        }
        connection.execute(
            "INSERT INTO lithair_schemas_v1 (namespace, model, version, schema) VALUES (?1, ?2, ?3, ?4) ON CONFLICT (namespace, model) DO UPDATE SET version = excluded.version, schema = excluded.schema",
            params![self.namespace.as_str(), T::COLLECTION, i64::from(T::VERSION), T::SCHEMA],
        ).await?;
        Ok(())
    }

    async fn migrate_documents(&self, connection: &Connection, from: u32) -> Result<()> {
        let mut after: Option<String> = None;
        loop {
            // Drop the cursor before updating. The primary key never changes,
            // so advancing by ID visits each original record once, without OFFSET.
            let page = {
                let mut rows = connection.query(
                    "SELECT id, body FROM lithair_documents_v1 WHERE namespace = ?1 AND model = ?2 AND (?3 IS NULL OR id > ?3) ORDER BY id LIMIT ?4",
                    params![self.namespace.as_str(), T::COLLECTION, after.as_deref(), i64::from(MAX_PAGE_SIZE)],
                ).await?;
                let mut page = Vec::new();
                while let Some(row) = rows.next().await? {
                    page.push((row.get::<String>(0)?, row.get::<String>(1)?));
                }
                page
            };
            if page.is_empty() {
                return Ok(());
            }
            for (id, body) in page {
                let migrated = self.migrate_document(&id, &body, from)?;
                connection.execute(
                    "UPDATE lithair_documents_v1 SET body = ?4 WHERE namespace = ?1 AND model = ?2 AND id = ?3",
                    params![self.namespace.as_str(), T::COLLECTION, id.as_str(), migrated],
                ).await?;
                after = Some(id);
            }
        }
    }

    fn migrate_document(&self, id: &str, body: &str, from: u32) -> Result<String> {
        // Catch user callbacks/validation panics before leaving the transaction
        // scope, so rollback is explicit rather than an async destructor promise.
        let result = catch_unwind(AssertUnwindSafe(|| -> Result<String> {
            if body.len() > MAX_DOCUMENT_BYTES {
                return Err(Error::Schema("stored document exceeds 1 MiB".into()));
            }
            let mut document: Value = serde_json::from_str(body)?;
            for migrate in &T::MIGRATIONS[(from - 1) as usize..] {
                migrate(&mut document)?;
            }
            if !document.is_object() {
                return Err(Error::Schema("migration must produce a JSON object".into()));
            }
            let body = serde_json::to_string(&document)?;
            if body.len() > MAX_DOCUMENT_BYTES {
                return Err(Error::Schema("migrated document exceeds 1 MiB".into()));
            }
            let value: T = serde_json::from_value(document)?;
            if value.get_primary_key() != id {
                return Err(Error::Schema("migration cannot change the primary key".into()));
            }
            value.validate().map_err(Error::Validation)?;
            // Persist the transformed JSON, not a T roundtrip: serde may ignore
            // fields still needed by a future migration.
            Ok(body)
        }));
        match result {
            Ok(Ok(body)) => Ok(body),
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

#[cfg(test)]
mod tests {
    use super::{Migration, SqlModel};
    use crate::Database;
    use lithair_macros::DeclarativeModel;
    use serde::{Deserialize, Serialize};
    use std::{future::Future, task::Poll, time::Duration};

    #[derive(Clone, Serialize, Deserialize, DeclarativeModel)]
    struct Document {
        id: String,
        title: String,
    }
    impl SqlModel for Document {
        const COLLECTION: &'static str = "cancel";
    }

    #[derive(Clone, Serialize, Deserialize, DeclarativeModel)]
    struct Upgraded {
        id: String,
        title: String,
    }
    impl SqlModel for Upgraded {
        const COLLECTION: &'static str = "cancel";
        const VERSION: u32 = 2;
        const SCHEMA: &'static str = "id:string,title:string:v2";
        const MIGRATIONS: &'static [Migration] = &[|document| {
            document["title"] =
                serde_json::json!(format!("{}!", document["title"].as_str().unwrap()));
            Ok(())
        }];
    }

    #[tokio::test]
    async fn cancelled_and_concurrent_preparations_commit_once_and_release_the_connection() {
        let tmp = tempfile::tempdir().unwrap();
        let db = Database::open(tmp.path().join("model.db").to_str().unwrap()).await.unwrap();
        db.store::<Document>("default")
            .unwrap()
            .create(Document { id: "one".into(), title: "kept".into() }, &[])
            .await
            .unwrap();
        let store = db.store::<Upgraded>("default").unwrap();
        let lock = db.connection.lock().await;
        let mut operation = Box::pin(store.prepare());
        std::future::poll_fn(|cx| {
            assert!(operation.as_mut().poll(cx).is_pending());
            Poll::Ready(())
        })
        .await;
        drop(operation);
        drop(lock);
        // A fresh legacy handle only becomes unusable after the detached task
        // commits. Do not accidentally drive the upgrade through another read.
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if db.store::<Document>("default").unwrap().prepare().await.is_err() {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let mut tasks = tokio::task::JoinSet::new();
        for _ in 0..8 {
            let db = db.clone();
            tasks.spawn(async move { db.store::<Upgraded>("default").unwrap().prepare().await });
        }
        while let Some(result) = tasks.join_next().await {
            result.unwrap().unwrap();
        }
        assert_eq!(store.get("one", &[]).await.unwrap().unwrap().title, "kept!");
    }
}
