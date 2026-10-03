//! L1 copies across nodes (RFC 304). Writes publish `pg_notify` inside their
//! transaction, so a notification exists only for committed changes and is
//! delivered after commit. Every node listens on a dedicated connection and
//! evicts the named copies. While that connection is down, notifications are
//! lost, so every cache is flushed and disabled until listening resumes.
use crate::{Error, Result};
use lithair_core::app::{CachePolicy, DocumentCache};
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, Weak,
    },
    time::Duration,
};
use tokio::sync::OnceCell;
use tokio_postgres::{AsyncMessage, Transaction};

/// The notification channel shared by every Lithair node on a database.
pub(crate) const CHANNEL: &str = "lithair_cache";
/// `application_name` of the listening connection.
pub const LISTENER_NAME: &str = "lithair-cache-listener";

/// How copies stay coherent with writes from other processes.
pub(crate) enum Coherence {
    /// Notifications (default): copies are evicted milliseconds after commit.
    Notify {
        config: Box<tokio_postgres::Config>,
        tls: Option<tokio_postgres_rustls::MakeRustlsConnect>,
    },
    /// Explicit opt-in: no listener; copies expire only with their TTL.
    TtlOnly,
}

pub(crate) struct Caches {
    caches: Mutex<HashMap<(String, String), Arc<DocumentCache>>>,
    coherence: Coherence,
    listening: AtomicBool,
    started: OnceCell<std::result::Result<(), String>>,
    connect_timeout: Duration,
}

impl Caches {
    pub fn new(coherence: Coherence, connect_timeout: Duration) -> Arc<Self> {
        Arc::new(Self {
            caches: Mutex::default(),
            coherence,
            listening: AtomicBool::new(false),
            started: OnceCell::new(),
            connect_timeout,
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<(String, String), Arc<DocumentCache>>> {
        self.caches.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The shared cache of one partition. With notifications, it serves
    /// nothing until the listener is confirmed.
    pub fn cache(
        &self,
        namespace: &str,
        collection: &str,
        policy: CachePolicy,
    ) -> Result<Arc<DocumentCache>> {
        if matches!(self.coherence, Coherence::TtlOnly) && policy.ttl.is_none() {
            return Err(Error::Config(format!(
                "{namespace}/{collection}: a TTL-only cache requires #[retention(ttl = \"...\")]"
            )));
        }
        let mut caches = self.lock();
        let cache = caches
            .entry((namespace.to_owned(), collection.to_owned()))
            .or_insert_with(|| {
                let cache = DocumentCache::new(policy);
                if matches!(self.coherence, Coherence::Notify { .. })
                    && !self.listening.load(Ordering::Acquire)
                {
                    cache.set_enabled(false);
                }
                Arc::new(cache)
            })
            .clone();
        Ok(cache)
    }

    pub fn invalidate(&self, namespace: &str, collection: &str, id: Option<&str>) {
        if let Some(cache) = self.lock().get(&(namespace.to_owned(), collection.to_owned())) {
            match id {
                Some(id) => cache.invalidate(id),
                None => cache.clear(),
            }
        }
    }

    fn flush_all(&self) {
        for cache in self.lock().values() {
            cache.clear();
        }
    }

    fn set_listening(&self, listening: bool) {
        // Flip under the registry lock, so a cache created concurrently sees a
        // consistent state; set_enabled also bumps generations.
        let caches = self.lock();
        self.listening.store(listening, Ordering::Release);
        for cache in caches.values() {
            cache.set_enabled(listening);
        }
    }

    /// Start the listener once, and wait for its first `LISTEN`. Fails closed
    /// when it cannot listen (choose `with_ttl_only_cache` to run without).
    pub async fn ensure_listening(self: &Arc<Self>) -> Result<()> {
        let Coherence::Notify { .. } = &self.coherence else { return Ok(()) };
        let result = self
            .started
            .get_or_init(|| async {
                let (ready, confirmed) = tokio::sync::oneshot::channel();
                tokio::spawn(listen(Arc::downgrade(self), ready));
                match tokio::time::timeout(self.connect_timeout * 2, confirmed).await {
                    Ok(Ok(Ok(()))) => Ok(()),
                    Ok(Ok(Err(e))) => Err(e),
                    _ => Err("timed out".to_string()),
                }
            })
            .await;
        result.clone().map_err(|e| {
            Error::Config(format!(
                "cache invalidation listener unavailable ({e}); use \
                 PostgresConfig::with_ttl_only_cache to cache without it"
            ))
        })
    }
}

/// Publish the invalidation of one record (`Some(id)`) or of a partition
/// (`None`) as part of `tx`: delivered only if it commits.
pub(crate) async fn notify(
    tx: &Transaction<'_>,
    namespace: &str,
    collection: &str,
    id: Option<&str>,
) -> Result<()> {
    let payload = serde_json::to_string(&(namespace, collection, id))?;
    tx.execute("SELECT pg_notify($1, $2)", &[&CHANNEL, &payload]).await?;
    Ok(())
}

async fn listen(
    caches: Weak<Caches>,
    ready: tokio::sync::oneshot::Sender<std::result::Result<(), String>>,
) {
    let mut ready = Some(ready);
    let mut backoff = Duration::from_millis(100);
    loop {
        let Some(shared) = caches.upgrade() else { return };
        let Coherence::Notify { config, tls } = &shared.coherence else { return };
        let mut config = (**config).clone();
        config.application_name(LISTENER_NAME);
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let connected = match tls {
            Some(tls) => config.connect(tls.clone()).await.map(|(c, conn)| (c, drive(conn, tx))),
            None => config
                .connect(tokio_postgres::NoTls)
                .await
                .map(|(c, conn)| (c, drive(conn, tx))),
        };
        let outcome: std::result::Result<(), String> = match connected {
            Ok((client, driver)) => {
                match client.batch_execute(&format!("LISTEN {CHANNEL}")).await {
                    Ok(()) => {
                        shared.set_listening(true);
                        if let Some(ready) = ready.take() {
                            let _ = ready.send(Ok(()));
                        }
                        backoff = Duration::from_millis(100);
                        drop(shared);
                        while let Some(payload) = rx.recv().await {
                            let Some(shared) = caches.upgrade() else { return };
                            match serde_json::from_str::<(String, String, Option<String>)>(&payload)
                            {
                                Ok((namespace, collection, id)) => {
                                    shared.invalidate(&namespace, &collection, id.as_deref())
                                }
                                // Unknown payload: assume the worst for this node.
                                Err(_) => shared.flush_all(),
                            }
                        }
                        driver.abort();
                        drop(client);
                        Err("listener connection lost".to_string())
                    }
                    Err(e) => Err(e.to_string()),
                }
            }
            Err(e) => Err(e.to_string()),
        };
        let Some(shared) = caches.upgrade() else { return };
        // Notifications may have been missed: nothing cached may be trusted.
        shared.set_listening(false);
        if let (Some(ready), Err(error)) = (ready.take(), &outcome) {
            let _ = ready.send(Err(error.clone()));
            return;
        }
        if let Err(error) = outcome {
            log::warn!("PostgreSQL cache listener: {error}; caches disabled until it reconnects");
        }
        drop(shared);
        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(Duration::from_secs(5));
    }
}

/// Drive the listening connection, forwarding notification payloads. The
/// channel closes when the connection ends.
fn drive<S, T>(
    mut connection: tokio_postgres::Connection<S, T>,
    tx: tokio::sync::mpsc::UnboundedSender<String>,
) -> tokio::task::JoinHandle<()>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
    T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        while let Some(message) = std::future::poll_fn(|cx| connection.poll_message(cx)).await {
            match message {
                Ok(AsyncMessage::Notification(n)) => {
                    if tx.send(n.payload().to_owned()).is_err() {
                        return;
                    }
                }
                Ok(_) => {}
                Err(_) => return,
            }
        }
    })
}
