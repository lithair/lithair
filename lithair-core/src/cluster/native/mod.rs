//! Explicit native model consensus; independent of optional admin HTTP/UI.
//! See `docs/features/state-engine/native-cluster.md` for the supported contract.
mod model;
mod state;
#[cfg(test)]
mod tests;
use super::{durable_log::DurableLog, peer_transport::PeerTransport};
use crate::app::{response, RouteRequest, RouteResponse};
use http::{Method, StatusCode};
use http_body_util::{BodyExt, Limited};
pub use model::Model;
use model::Mutation;
use openraft::{storage::Adaptor, Raft};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use state::{Change, Command, Config, Machine, State};
use std::{
    collections::BTreeMap,
    path::Path,
    sync::{atomic::Ordering, Arc},
    time::Duration,
};
use tokio::{
    net::TcpListener,
    sync::{oneshot, Mutex, Semaphore},
    task::JoinHandle,
};

const REQUEST_LIMIT: usize = 64 * 1024;
const DEADLINE: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reply {
    pub(super) status: u16,
    pub(super) body: Option<Value>,
}
impl Reply {
    fn error(status: u16, error: &str) -> Self {
        Self { status, body: Some(json!({"error":error})) }
    }
    fn empty() -> Self {
        Self { status: 204, body: None }
    }
    fn response(&self) -> RouteResponse {
        let status = StatusCode::from_u16(self.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        match &self.body {
            Some(body) => response::json_value(status, body),
            None => response::json(status, ""),
        }
    }
}

struct Inner {
    raft: Raft<Config>,
    machine: Machine,
    transport: PeerTransport<Config>,
    models: BTreeMap<String, Model>,
    admission: Mutex<()>,
    slots: Arc<Semaphore>,
    stop: std::sync::Mutex<Option<oneshot::Sender<()>>>,
    peer_task: std::sync::Mutex<Option<JoinHandle<std::io::Result<()>>>>,
}
impl Drop for Inner {
    fn drop(&mut self) {
        if let Ok(stop) = self.stop.get_mut() {
            if let Some(stop) = stop.take() {
                let _ = stop.send(());
            }
        }
        if let Ok(task) = self.peer_task.get_mut() {
            if let Some(task) = task.take() {
                task.abort();
            }
        }
        let raft = self.raft.clone();
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
                let _ = raft.shutdown().await;
            });
        }
    }
}

/// A running three-voter native group. Opening never provisions or bootstraps.
/// Clones share one runtime. Drop closes the peer listener; use `shutdown()` to
/// await shutdown before reopening a store in the same process.
#[derive(Clone)]
pub struct NativeCluster {
    inner: Arc<Inner>,
}
impl NativeCluster {
    /// Open a previously provisioned operator configuration and bind a fixed
    /// application/schema contract. All nodes must use the same version label
    /// and model declarations. Schema transformations are deliberately refused.
    pub async fn open(
        config: impl AsRef<Path>,
        application: &str,
        models: Vec<Model>,
    ) -> anyhow::Result<Self> {
        let path = config.as_ref().to_owned();
        let prepared =
            tokio::task::spawn_blocking(move || super::operator::prepare::<Config>(&path))
                .await??;
        let listener = TcpListener::bind(prepared.transport.local_address()).await?;
        Self::start(prepared, listener, application, models).await
    }

    /// Like `open`, using a socket kept bound by the caller (useful for isolated
    /// tests and hosts that bind separately from their configured dial address).
    pub async fn open_with_listener(
        config: impl AsRef<Path>,
        application: &str,
        models: Vec<Model>,
        listener: TcpListener,
    ) -> anyhow::Result<Self> {
        let path = config.as_ref().to_owned();
        let prepared =
            tokio::task::spawn_blocking(move || super::operator::prepare::<Config>(&path))
                .await??;
        anyhow::ensure!(
            listener.local_addr()?.port() == prepared.transport.local_address().port(),
            "peer listener port differs from configured dial port"
        );
        Self::start(prepared, listener, application, models).await
    }

    async fn start(
        prepared: super::operator::Prepared<Config>,
        listener: TcpListener,
        application: &str,
        models: Vec<Model>,
    ) -> anyhow::Result<Self> {
        anyhow::ensure!(
            !application.is_empty() && application.len() <= 128,
            "an explicit application contract version is required"
        );
        anyhow::ensure!(!models.is_empty(), "native consensus requires at least one model");
        let mut registered: BTreeMap<String, Model> = BTreeMap::new();
        for model in models {
            anyhow::ensure!(
                registered.values().all(|m| m.path != model.path
                    && !m.path.starts_with(&format!("{}/", model.path))
                    && !model.path.starts_with(&format!("{}/", m.path))),
                "overlapping native model routes"
            );
            anyhow::ensure!(
                registered.insert(model.id.clone(), model).is_none(),
                "duplicate native model identity"
            );
        }
        let specs: BTreeMap<_, _> = registered.iter().map(|(id, m)| (id, &m.contract)).collect();
        let contract: [u8; 32] = Sha256::digest(serde_json::to_vec(
            &json!({"wire":1,"application":application,"models":specs}),
        )?)
        .into();
        let transport = prepared.transport.with_application(contract)?;
        let identity = transport.node_identity()?;
        let durable =
            DurableLog::<Config>::open_for_node(&prepared.directory, identity.clone()).await?;
        durable.bind_application(contract).await?;
        let machine = Machine::restore(durable, State::new(contract, &registered)).await?;
        // Separate adaptor locks: purge can wait for a concurrent snapshot install.
        let (log, _) = Adaptor::new(machine.clone());
        let (_, state) = Adaptor::new(machine.clone());
        let config = openraft::Config {
            heartbeat_interval: 100,
            election_timeout_min: 400,
            election_timeout_max: 800,
            snapshot_policy: openraft::SnapshotPolicy::LogsSinceLast(16),
            replication_lag_threshold: 16,
            max_in_snapshot_log_to_keep: 2,
            purge_batch_size: 1,
            max_payload_entries: 1,
            snapshot_max_chunk_size: 64 * 1024,
            ..Default::default()
        }
        .validate()?;
        let raft =
            Raft::new(identity.node_id, Arc::new(config), transport.clone(), log, state).await?;
        let (stop, stopped) = oneshot::channel();
        let network = transport.clone();
        let peer_raft = raft.clone();
        let peer_task = tokio::spawn(async move {
            network
                .serve(listener, peer_raft, async {
                    let _ = stopped.await;
                })
                .await
        });
        Ok(Self {
            inner: Arc::new(Inner {
                raft,
                machine,
                transport,
                models: registered,
                admission: Mutex::new(()),
                slots: Arc::new(Semaphore::new(128)),
                stop: std::sync::Mutex::new(Some(stop)),
                peer_task: std::sync::Mutex::new(Some(peer_task)),
            }),
        })
    }

    /// Explicit operator action, only once on the designated pristine node.
    /// The public app and peer RPC listeners never expose this operation.
    pub async fn bootstrap(&self) -> anyhow::Result<()> {
        self.inner
            .transport
            .bootstrap(&self.inner.machine.durable, &self.inner.raft)
            .await
    }
    pub async fn shutdown(&self) -> anyhow::Result<()> {
        if let Some(stop) = self
            .inner
            .stop
            .lock()
            .map_err(|_| anyhow::anyhow!("shutdown lock failed"))?
            .take()
        {
            let _ = stop.send(());
        }
        self.inner.raft.shutdown().await?;
        let task = self
            .inner
            .peer_task
            .lock()
            .map_err(|_| anyhow::anyhow!("peer task lock failed"))?
            .take();
        if let Some(task) = task {
            task.await??;
        }
        Ok(())
    }
    /// A fresh consensus barrier, including local apply. Followers and isolated
    /// leaders return false. Suitable for leader-only load-balancer readiness.
    pub async fn ready(&self) -> bool {
        tokio::time::timeout(DEADLINE, self.barrier()).await.is_ok_and(|r| r.is_ok())
    }
    async fn barrier(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            !self.inner.machine.failed.load(Ordering::Acquire),
            "native apply unavailable"
        );
        anyhow::ensure!(
            !self
                .inner
                .peer_task
                .lock()
                .map_err(|_| anyhow::anyhow!("peer task failed"))?
                .as_ref()
                .is_none_or(|t| t.is_finished()),
            "peer listener unavailable"
        );
        self.inner.raft.ensure_linearizable().await?;
        anyhow::ensure!(
            !self.inner.machine.failed.load(Ordering::Acquire),
            "native apply unavailable"
        );
        Ok(())
    }
    /// Local diagnostics, not a consistent application read and not proof of
    /// readiness. Applications choose whether/how to expose operator reports.
    pub async fn inspect(&self) -> Value {
        let metrics = self.inner.raft.metrics().borrow().clone();
        let state = self.inner.machine.state.read().await;
        json!({"node_id":metrics.id,"leader":metrics.current_leader,"term":metrics.current_term,
            "data_sha256":hex::encode(Sha256::digest(serde_json::to_vec(&state.models).unwrap_or_default())),
            "applied":state.applied.map(|id| id.index),"revision":state.revision,
            "purged":metrics.purged.map(|id|id.index),"snapshot_installs":self.inner.machine.installs.load(Ordering::Relaxed),
            "failed":self.inner.machine.failed.load(Ordering::Acquire) || metrics.running_state.is_err(),"initialized":self.inner.raft.is_initialized().await.unwrap_or(false)})
    }
    pub(crate) fn paths(&self) -> Vec<String> {
        self.inner.models.values().map(|m| m.path.clone()).collect()
    }
    pub(crate) fn matches(&self, path: &str) -> bool {
        self.inner
            .models
            .values()
            .any(|m| path == m.path || path.starts_with(&format!("{}/", m.path)))
    }
    pub(crate) fn unavailable() -> RouteResponse {
        let mut response = Reply::error(503, "Native consensus unavailable; retry through the ready leader with the same Idempotency-Key").response();
        response
            .headers_mut()
            .insert("retry-after", http::HeaderValue::from_static("1"));
        response
    }
    pub(crate) async fn handle(&self, request: RouteRequest) -> RouteResponse {
        let path = request.uri().path().trim_end_matches('/');
        let Some(model) = self
            .inner
            .models
            .values()
            .find(|m| path == m.path || path.starts_with(&format!("{}/", m.path)))
        else {
            return Reply::error(404, "Unknown model").response();
        };
        if request.method() == Method::OPTIONS {
            let mut response = Reply::empty().response();
            response.headers_mut().insert(
                "allow",
                http::HeaderValue::from_static("GET, HEAD, POST, PUT, PATCH, DELETE, OPTIONS"),
            );
            return response;
        }
        let key = path.strip_prefix(&format!("{}/", model.path)).map(str::to_owned);
        if key.as_ref().is_some_and(|id| id.contains('/') || model::reserved_key(id)) {
            return Reply::error(404, "Unsupported native consensus endpoint").response();
        }
        let method = request.method().clone();
        if method == Method::GET || method == Method::HEAD {
            if !self.ready().await {
                return Self::unavailable();
            }
            let state = self.inner.machine.state.read().await;
            let records = &state.models[&model.id];
            let reply = match &key {
                Some(key) => match records.get(key) {
                    None => Reply::error(404, "Entity not found"),
                    Some(value) if !(model.readable)(value) => {
                        Reply::error(403, "Insufficient permissions")
                    }
                    Some(value) => Reply { status: 200, body: Some(value.clone()) },
                },
                None => Reply {
                    status: 200,
                    body: Some(crate::http::query::list_response(
                        request.uri().query().unwrap_or(""),
                        records.values().filter(|v| (model.readable)(v)).cloned().collect(),
                    )),
                },
            };
            let mut response = reply.response();
            if method == Method::HEAD {
                *response.body_mut() = http_body_util::Full::new(bytes::Bytes::new()).boxed();
            }
            return response;
        }
        if !(method == Method::POST && key.is_none()
            || matches!(method, Method::PUT | Method::PATCH | Method::DELETE) && key.is_some())
        {
            return Reply::error(405, "Unsupported method").response();
        }
        if request.uri().query().is_some() {
            return Reply::error(400, "Mutation query parameters are unsupported").response();
        }
        let request_id =
            match request.headers().get("idempotency-key").and_then(|s| s.to_str().ok()) {
                Some(id)
                    if !id.is_empty()
                        && id.len() <= 128
                        && id.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b)) =>
                {
                    id.to_owned()
                }
                _ => {
                    return Reply::error(400, "A 1-128 character ASCII Idempotency-Key is required")
                        .response()
                }
            };
        if method != Method::DELETE
            && !request.headers().get("content-type").and_then(|v| v.to_str().ok()).is_some_and(
                |v| {
                    v.split(';')
                        .next()
                        .is_some_and(|s| s.trim().eq_ignore_ascii_case("application/json"))
                },
            )
        {
            return Reply::error(415, "Expected application/json").response();
        }
        let model_id = model.id.clone();
        let bytes = match Limited::new(request.into_body(), REQUEST_LIMIT).collect().await {
            Ok(body) => body.to_bytes(),
            Err(_) => return Reply::error(413, "Native command body exceeds 64 KiB").response(),
        };
        let data = if method == Method::DELETE {
            Value::Null
        } else {
            match serde_json::from_slice(&bytes) {
                Ok(value) => value,
                Err(_) => return Reply::error(400, "Invalid JSON").response(),
            }
        };
        let mutation = Mutation { method: method.to_string(), key, data };
        let Ok(permit) = self.inner.slots.clone().try_acquire_owned() else {
            return Self::unavailable();
        };
        let this = self.clone();
        // Own the admission permit and proposal until consensus resolves it.
        // Timing out or dropping HTTP never rolls back an admitted command.
        let task = tokio::spawn(async move {
            let _permit = permit;
            tokio::time::timeout(DEADLINE * 2, this.write(model_id, request_id, mutation)).await?
        });
        match tokio::time::timeout(DEADLINE, task).await {
            Ok(Ok(Ok(reply))) => reply.response(),
            _ => Self::unavailable(),
        }
    }
    async fn write(
        &self,
        model_id: String,
        request_id: String,
        request: Mutation,
    ) -> anyhow::Result<Reply> {
        let _admission = self.inner.admission.lock().await;
        self.barrier().await?;
        let fingerprint: [u8; 32] = Sha256::digest(serde_json::to_vec(&json!([
            model_id,
            request.method,
            request.key,
            request.data
        ]))?)
        .into();
        let state = self.inner.machine.state.read().await;
        if let Some(result) = state.result(&model_id, &request_id, &fingerprint) {
            return Ok(result);
        }
        let model = &self.inner.models[&model_id];
        let (change, reply) = (model.prepare)(&state.models[&model_id], &request)
            .unwrap_or_else(|reply| (Change::None, reply));
        let mut command = Command {
            version: 1,
            contract: state.contract,
            model: model_id,
            request_id,
            fingerprint,
            revision: state.revision,
            change,
            reply,
        };
        if serde_json::to_vec(&command)?.len() > 256 * 1024 {
            command.change = Change::None;
            command.reply = Reply::error(413, "Canonical native command exceeds 256 KiB");
        }
        let mut candidate = state.clone();
        candidate.execute(&command)?;
        if serde_json::to_vec(&candidate)?.len() > state::MAX_CHECKPOINT - REQUEST_LIMIT {
            command.change = Change::None;
            command.reply = Reply::error(507, "Native consensus checkpoint capacity reached");
        }
        drop(state);
        Ok(self.inner.raft.client_write(command).await?.data)
    }
}
