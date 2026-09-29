//! Trusted application commands: read, decide and atomically write several
//! declared collections of one namespace in one SQL transaction.
//!
//! No HTTP route, session or permission list is involved: authenticate before
//! calling, then check current policy inside the decision **before** looking up
//! a receipt. Decisions must be deterministic and free of external effects;
//! rollback cannot undo network, file or process changes. See the README.
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
use turso::{params, Connection};

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

/// Committed state inside the command transaction. Nothing else can write the
/// database until the decision returns: reads and the commit are serializable.
pub struct View<'a> {
    connection: &'a Connection,
    commands: &'a Commands,
}
impl View<'_> {
    pub async fn get(&self, collection: &str, key: &str) -> Result<Option<Json>> {
        self.commands.declared(collection)?;
        let mut rows = self
            .connection
            .query(
                "SELECT body FROM lithair_documents_v1 WHERE namespace = ?1 AND model = ?2 AND id = ?3",
                params![self.commands.namespace.as_str(), collection, key],
            )
            .await?;
        match rows.next().await? {
            Some(row) => Ok(Some(serde_json::from_str(&row.get::<String>(0)?)?)),
            None => Ok(None),
        }
    }
    /// Read a typed record; refused unless the partition has this model's schema.
    pub async fn model<T: SqlModel>(&self, id: &str) -> Result<Option<T>> {
        schema_matches(
            self.connection,
            &self.commands.namespace,
            T::COLLECTION,
            T::VERSION,
            T::SCHEMA,
        )
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
        let mut rows = self
            .connection
            .query(
                "SELECT id, body FROM lithair_documents_v1 WHERE namespace = ?1 AND model = ?2 AND (?3 IS NULL OR id > ?3) ORDER BY id LIMIT ?4",
                params![self.commands.namespace.as_str(), collection, after, i64::from(limit)],
            )
            .await?;
        let mut page = Vec::new();
        while let Some(row) = rows.next().await? {
            page.push((row.get::<String>(0)?, serde_json::from_str(&row.get::<String>(1)?)?));
        }
        Ok(page)
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
    /// Run `decide` and commit its writes in one `BEGIN IMMEDIATE` transaction.
    /// Returns only after commit. An error or panic in the decision, or any
    /// invalid write, rolls back everything.
    ///
    /// The decision holds the shared connection: keep it short, never block or
    /// perform I/O outside `View`. Once admitted, the command completes even if
    /// the caller drops this future, so a timeout or lost response is an
    /// unknown outcome: retry with the same business key, which the decision
    /// resolves through its own durable receipt.
    pub async fn execute<F>(&self, decide: F) -> Result<Json>
    where
        F: for<'a> FnOnce(&'a View<'a>) -> Decided<'a> + Send + 'static,
    {
        let commands = self.clone();
        tokio::spawn(async move {
            let mut connection = commands.database.connection.lock().await;
            let transaction = connection
                .transaction_with_behavior(turso::transaction::TransactionBehavior::Immediate)
                .await?;
            match commands.run(&transaction, decide).await {
                Ok(reply) => {
                    transaction.commit().await?;
                    Ok(reply)
                }
                Err(error) => {
                    // A rollback error must remain visible to the caller.
                    transaction.rollback().await?;
                    Err(error)
                }
            }
        })
        .await?
    }

    async fn run<F>(&self, connection: &Connection, decide: F) -> Result<Json>
    where
        F: for<'a> FnOnce(&'a View<'a>) -> Decided<'a>,
    {
        let view = View { connection, commands: self };
        // Catch panics while the transaction is still in scope, so rollback is
        // explicit rather than deferred to the connection's next use.
        let panicked = || Error::Command(anyhow::anyhow!("command decision panicked"));
        let mut decided =
            catch_unwind(AssertUnwindSafe(|| decide(&view))).map_err(|_| panicked())?;
        let decision = std::future::poll_fn(|cx| {
            match catch_unwind(AssertUnwindSafe(|| decided.as_mut().poll(cx))) {
                Ok(Poll::Pending) => Poll::Pending,
                Ok(Poll::Ready(result)) => Poll::Ready(result.map_err(Error::Command)),
                Err(_) => Poll::Ready(Err(panicked())),
            }
        })
        .await?;
        drop(decided);
        let (writes, reply) = match decision {
            Decision::Read(reply) => return Ok(reply),
            Decision::Commit { writes, reply } => (writes, reply),
        };
        let mut encoded = Vec::with_capacity(writes.len());
        self.check(&writes, &mut encoded)?;
        for (write, body) in writes.iter().zip(encoded) {
            let (collection, key) = write.target();
            match write {
                Write::Put { schema, .. } => {
                    let tracked = stored_schema(connection, &self.namespace, collection).await?;
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
                    connection
                        .execute(
                            "INSERT INTO lithair_documents_v1 (namespace, model, id, body) VALUES (?1, ?2, ?3, ?4) ON CONFLICT (namespace, model, id) DO UPDATE SET body = excluded.body",
                            params![self.namespace.as_str(), collection, key, body],
                        )
                        .await?;
                }
                Write::Delete { .. } => {
                    connection
                        .execute(
                            "DELETE FROM lithair_documents_v1 WHERE namespace = ?1 AND model = ?2 AND id = ?3",
                            params![self.namespace.as_str(), collection, key],
                        )
                        .await?;
                }
            }
        }
        Ok(reply)
    }

    /// Validate the whole batch before the first statement runs.
    fn check(&self, writes: &[Write], encoded: &mut Vec<String>) -> Result<()> {
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
            encoded.push(match write {
                Write::Put { value, .. } => {
                    if !value.is_object() {
                        return Err(Error::InvalidInput("records must be JSON objects".into()));
                    }
                    let body = serde_json::to_string(value)?;
                    if body.len() > MAX_DOCUMENT_BYTES {
                        return Err(Error::InvalidInput("document exceeds 1 MiB".into()));
                    }
                    body
                }
                Write::Delete { .. } => String::new(),
            });
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

async fn stored_schema(
    connection: &Connection,
    namespace: &str,
    collection: &str,
) -> Result<Option<(u32, String)>> {
    let mut rows = connection
        .query(
            "SELECT version, schema FROM lithair_schemas_v1 WHERE namespace = ?1 AND model = ?2",
            params![namespace, collection],
        )
        .await?;
    match rows.next().await? {
        Some(row) => Ok(Some((u32::try_from(row.get::<i64>(0)?).unwrap_or(0), row.get(1)?))),
        None => Ok(None),
    }
}

async fn schema_matches(
    connection: &Connection,
    namespace: &str,
    collection: &str,
    version: u32,
    schema: &str,
) -> Result<()> {
    match stored_schema(connection, namespace, collection).await? {
        Some((v, s)) if v == version && s == schema => Ok(()),
        _ => Err(Error::Schema(format!(
            "{namespace}/{collection} is not prepared with this model's schema"
        ))),
    }
}
