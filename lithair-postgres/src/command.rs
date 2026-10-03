//! Trusted application commands: read, decide and atomically write several
//! declared collections of one namespace in one `SERIALIZABLE` transaction.
//!
//! Same contract as `lithair-turso` commands, with one difference: the
//! decision is an `Fn`, because a serialization conflict with another writer
//! (possibly another process) rolls the transaction back and **runs the
//! decision again** on the new committed state. Decisions must therefore be
//! deterministic and free of external effects. Authenticate before calling;
//! check current policy inside the decision before looking up a receipt.
use crate::{Database, Error, Result, SqlModel, MAX_BATCH_SIZE, MAX_DOCUMENT_BYTES};
use serde_json::Value as Json;
use std::{
    collections::BTreeSet,
    future::Future,
    panic::{catch_unwind, AssertUnwindSafe},
    pin::Pin,
    sync::Arc,
    task::Poll,
};
use tokio_postgres::Transaction;

/// The future a decision returns; it borrows the transaction view.
pub type Decided<'a> = Pin<Box<dyn Future<Output = anyhow::Result<Decision>> + Send + 'a>>;

/// A record change. The whole batch commits or none of it does.
pub enum Write {
    Put { collection: String, key: String, value: Json, schema: Option<(u32, &'static str)> },
    Delete { collection: String, key: String },
}
impl Write {
    /// Put a JSON object into an application-owned (untracked) collection.
    pub fn put(collection: impl Into<String>, key: impl Into<String>, value: Json) -> Self {
        Self::Put { collection: collection.into(), key: key.into(), value, schema: None }
    }
    pub fn delete(collection: impl Into<String>, key: impl Into<String>) -> Self {
        Self::Delete { collection: collection.into(), key: key.into() }
    }
    /// Put a typed model into its prepared partition. Runs `validate`, not the
    /// permission hooks: the decision is trusted and enforces its own policy.
    /// The commit is refused unless the partition has exactly this model's schema.
    pub fn model<T: SqlModel>(value: &T) -> Result<Self> {
        value.validate().map_err(Error::Validation)?;
        Ok(Self::Put {
            collection: T::COLLECTION.into(),
            key: value.get_primary_key(),
            value: serde_json::to_value(value)?,
            schema: Some((T::VERSION, T::SCHEMA)),
        })
    }
    fn target(&self) -> (&str, &str) {
        match self {
            Self::Put { collection, key, .. } | Self::Delete { collection, key } => {
                (collection, key)
            }
        }
    }
}

/// `Read` returns without writing (for example an authorized receipt replay).
/// `Commit` writes every record atomically, then returns the reply.
pub enum Decision {
    Read(Json),
    Commit { writes: Vec<Write>, reply: Json },
}

/// Committed state inside the command transaction. `SERIALIZABLE` isolation
/// makes the reads and the commit behave as if no other transaction ran
/// concurrently; a conflicting one makes Postgres replay the whole command.
pub struct View<'a> {
    tx: &'a Transaction<'a>,
    commands: &'a Commands,
}
impl View<'_> {
    pub async fn get(&self, collection: &str, key: &str) -> Result<Option<Json>> {
        self.commands.declared(collection)?;
        let row = self
            .tx
            .query_opt(
                "SELECT body FROM lithair.documents
                 WHERE namespace = $1 AND collection = $2 AND id = $3",
                &[&self.commands.namespace, &collection, &key],
            )
            .await?;
        Ok(row.map(|row| row.get(0)))
    }
    /// Read a typed record; refused unless the partition has this model's schema.
    pub async fn model<T: SqlModel>(&self, id: &str) -> Result<Option<T>> {
        schema_matches(self.tx, &self.commands.namespace, T::COLLECTION, T::VERSION, T::SCHEMA)
            .await?;
        match self.get(T::COLLECTION, id).await? {
            Some(value) => Ok(Some(serde_json::from_value(value)?)),
            None => Ok(None),
        }
    }
    /// Up to `limit` (1..=MAX_PAGE_SIZE) records after `after`, in key order.
    pub async fn list(
        &self,
        collection: &str,
        after: Option<&str>,
        limit: u32,
    ) -> Result<Vec<(String, Json)>> {
        self.commands.declared(collection)?;
        if limit == 0 || limit > crate::MAX_PAGE_SIZE {
            return Err(Error::InvalidInput(format!(
                "page limit must be 1..{}",
                crate::MAX_PAGE_SIZE
            )));
        }
        let rows = self
            .tx
            .query(
                "SELECT id, body FROM lithair.documents
                 WHERE namespace = $1 AND collection = $2 AND ($3::text IS NULL OR id > $3)
                 ORDER BY id LIMIT $4",
                &[&self.commands.namespace, &collection, &after, &i64::from(limit)],
            )
            .await?;
        Ok(rows.into_iter().map(|row| (row.get(0), row.get(1))).collect())
    }
}

/// Application commands over declared collections of one trusted namespace.
#[derive(Clone)]
pub struct Commands {
    database: Database,
    namespace: String,
    collections: Arc<BTreeSet<String>>,
}

impl Database {
    /// Namespace and collections are chosen by trusted code, never by a client.
    pub fn commands(&self, namespace: &str, collections: &[&str]) -> Result<Commands> {
        let declared: BTreeSet<String> = collections.iter().map(|c| (*c).to_owned()).collect();
        if namespace.is_empty()
            || namespace.len() > 128
            || declared.is_empty()
            || declared.len() != collections.len()
            || declared.iter().any(|c| c.is_empty() || c.len() > 128)
        {
            return Err(Error::InvalidInput(
                "declare 1..128-byte namespace and distinct 1..128-byte collections".into(),
            ));
        }
        Ok(Commands {
            database: self.clone(),
            namespace: namespace.into(),
            collections: Arc::new(declared),
        })
    }
}

impl Commands {
    /// Run `decide` and commit its writes in one `SERIALIZABLE` transaction,
    /// replayed from the start (decision included) on a serialization
    /// conflict. Returns only after commit. An error or panic in the
    /// decision, or any invalid write, rolls back everything.
    ///
    /// Once admitted, the command completes even if the caller drops this
    /// future. A timeout or a connection lost during `COMMIT` is an unknown
    /// outcome: retry with the same business key, which the decision resolves
    /// through its own durable receipt.
    pub async fn execute<F>(&self, decide: F) -> Result<Json>
    where
        F: for<'a> Fn(&'a View<'a>) -> Decided<'a> + Send + Sync + 'static,
    {
        let commands = self.clone();
        tokio::spawn(async move {
            // Every attempt's writes, for local invalidation (RFC 304).
            let touched = std::sync::Mutex::new(Vec::new());
            let (commands, decide, touched_ref) = (&commands, &decide, &touched);
            let result = commands
                .database
                .transaction(|tx, _| Box::pin(commands.run(tx, decide, touched_ref)))
                .await;
            let touched = touched.into_inner().unwrap_or_else(|e| e.into_inner());
            for (collection, key) in &touched {
                commands.database.caches.invalidate(&commands.namespace, collection, Some(key));
            }
            result
        })
        .await?
    }

    async fn run<F>(
        &self,
        tx: &Transaction<'_>,
        decide: &F,
        touched: &std::sync::Mutex<Vec<(String, String)>>,
    ) -> Result<Json>
    where
        F: for<'a> Fn(&'a View<'a>) -> Decided<'a>,
    {
        let view = View { tx, commands: self };
        let panicked = || Error::Command(anyhow::anyhow!("command decision panicked"));
        let mut decided =
            catch_unwind(AssertUnwindSafe(|| decide(&view))).map_err(|_| panicked())?;
        let decision = std::future::poll_fn(|cx| {
            match catch_unwind(AssertUnwindSafe(|| decided.as_mut().poll(cx))) {
                Ok(Poll::Pending) => Poll::Pending,
                Ok(Poll::Ready(result)) => Poll::Ready(result.map_err(decision_error)),
                Err(_) => Poll::Ready(Err(panicked())),
            }
        })
        .await?;
        drop(decided);
        let (writes, reply) = match decision {
            Decision::Read(reply) => return Ok(reply),
            Decision::Commit { writes, reply } => (writes, reply),
        };
        self.check(&writes)?;
        touched.lock().unwrap_or_else(|e| e.into_inner()).extend(writes.iter().map(|w| {
            let (collection, key) = w.target();
            (collection.to_owned(), key.to_owned())
        }));
        for write in &writes {
            let (collection, key) = write.target();
            self.database.caches.notify(tx, &self.namespace, collection, Some(key)).await?;
        }
        for write in &writes {
            let (collection, key) = write.target();
            match write {
                Write::Put { schema, value, .. } => {
                    let tracked = stored_schema(tx, &self.namespace, collection).await?;
                    match (schema, tracked) {
                        (Some((version, schema)), Some((v, s)))
                            if v == *version && s == *schema => {}
                        (Some(_), _) => {
                            return Err(Error::Schema(format!(
                                "{}/{collection} is not prepared with this model's schema",
                                self.namespace
                            )))
                        }
                        (None, Some(_)) => {
                            return Err(Error::InvalidInput(format!(
                                "{collection} is a typed model partition; use Write::model"
                            )))
                        }
                        (None, None) => {}
                    }
                    tx.execute(
                        "INSERT INTO lithair.documents (namespace, collection, id, body)
                         VALUES ($1, $2, $3, $4)
                         ON CONFLICT (namespace, collection, id) DO UPDATE SET body = excluded.body",
                        &[&self.namespace, &collection, &key, value],
                    )
                    .await?;
                }
                Write::Delete { .. } => {
                    tx.execute(
                        "DELETE FROM lithair.documents
                         WHERE namespace = $1 AND collection = $2 AND id = $3",
                        &[&self.namespace, &collection, &key],
                    )
                    .await?;
                }
            }
        }
        Ok(reply)
    }

    /// Validate the whole batch before the first statement runs.
    fn check(&self, writes: &[Write]) -> Result<()> {
        if writes.is_empty() || writes.len() > MAX_BATCH_SIZE {
            return Err(Error::InvalidInput(format!("batch size must be 1..{MAX_BATCH_SIZE}")));
        }
        let mut seen = BTreeSet::new();
        for write in writes {
            let (collection, key) = write.target();
            self.declared(collection)?;
            if key.is_empty() || key.len() > 512 || !seen.insert((collection, key)) {
                return Err(Error::InvalidInput(
                    "keys must contain 1..512 bytes and appear once per collection".into(),
                ));
            }
            if let Write::Put { value, .. } = write {
                if !value.is_object() {
                    return Err(Error::InvalidInput("records must be JSON objects".into()));
                }
                if serde_json::to_vec(value)?.len() > MAX_DOCUMENT_BYTES {
                    return Err(Error::InvalidInput("document exceeds 1 MiB".into()));
                }
            }
        }
        Ok(())
    }

    fn declared(&self, collection: &str) -> Result<()> {
        if self.collections.contains(collection) {
            Ok(())
        } else {
            Err(Error::InvalidInput(format!("undeclared collection: {collection}")))
        }
    }
}

/// A storage error raised inside the decision (for example a read that hit a
/// serialization conflict) keeps its type, so the transaction is replayed;
/// anything else is the application's rejection.
fn decision_error(error: anyhow::Error) -> Error {
    match error.downcast::<Error>() {
        Ok(storage) => storage,
        Err(other) => Error::Command(other),
    }
}

async fn stored_schema(
    tx: &Transaction<'_>,
    namespace: &str,
    collection: &str,
) -> Result<Option<(u32, String)>> {
    let row = tx
        .query_opt(
            "SELECT version, schema FROM lithair.schemas WHERE namespace = $1 AND collection = $2",
            &[&namespace, &collection],
        )
        .await?;
    Ok(row.map(|row| (u32::try_from(row.get::<_, i32>(0)).unwrap_or(0), row.get(1))))
}

async fn schema_matches(
    tx: &Transaction<'_>,
    namespace: &str,
    collection: &str,
    version: u32,
    schema: &str,
) -> Result<()> {
    match stored_schema(tx, namespace, collection).await? {
        Some((v, s)) if v == version && s == schema => Ok(()),
        _ => Err(Error::Schema(format!(
            "{namespace}/{collection} is not prepared with this model's schema"
        ))),
    }
}
