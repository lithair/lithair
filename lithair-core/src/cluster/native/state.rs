//! One durable checkpoint contains data, application revision and request results.
//! No model code, wall clock or random generator runs during committed apply.
use super::model::Model;
use super::Reply;
use crate::cluster::durable_log::DurableLog;
use openraft::{
    Entry, EntryPayload, ErrorSubject, ErrorVerb, LogId, LogState, RaftLogReader,
    RaftSnapshotBuilder, RaftStorage, Snapshot, SnapshotMeta, StorageError, StoredMembership, Vote,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, VecDeque},
    io::Cursor,
    ops::RangeBounds,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
};
use tokio::sync::{Mutex, RwLock};

pub(super) const RESULTS: usize = 256;
pub(super) const MAX_CHECKPOINT: usize = 48 * 1024 * 1024;
openraft::declare_raft_types!(pub Config: D=Command, R=Reply, Node=(), SnapshotData=Cursor<Vec<u8>>);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Command {
    pub version: u32,
    pub contract: [u8; 32],
    pub model: String,
    pub request_id: String,
    pub fingerprint: [u8; 32],
    pub revision: u64,
    pub change: Change,
    pub reply: Reply,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum Change {
    None,
    Put { key: String, value: Value },
    Delete { key: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    model: String,
    request_id: String,
    fingerprint: [u8; 32],
    reply: Reply,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct State {
    version: u32,
    pub contract: [u8; 32],
    pub applied: Option<LogId<u64>>,
    membership: StoredMembership<u64, ()>,
    pub revision: u64,
    pub models: BTreeMap<String, BTreeMap<String, Value>>,
    results: VecDeque<Receipt>,
}
impl State {
    pub fn new(contract: [u8; 32], models: &BTreeMap<String, Model>) -> Self {
        Self {
            version: 1,
            contract,
            applied: None,
            membership: Default::default(),
            revision: 0,
            models: models.keys().map(|id| (id.clone(), BTreeMap::new())).collect(),
            results: VecDeque::new(),
        }
    }
    pub fn result(&self, model: &str, id: &str, fingerprint: &[u8; 32]) -> Option<Reply> {
        self.results.iter().find(|r| r.model == model && r.request_id == id).map(|r| {
            if &r.fingerprint == fingerprint {
                r.reply.clone()
            } else {
                Reply::error(409, "Idempotency-Key was already used for another request")
            }
        })
    }
    pub fn execute(&mut self, command: &Command) -> std::io::Result<Reply> {
        if command.version != 1
            || command.contract != self.contract
            || !self.models.contains_key(&command.model)
        {
            return Err(std::io::Error::other("incompatible committed application command"));
        }
        if let Some(reply) = self.result(&command.model, &command.request_id, &command.fingerprint)
        {
            return Ok(reply);
        }
        let reply = if command.revision != self.revision {
            Reply::error(409, "Ordered state changed before commit; use a new request key")
        } else {
            let records = self
                .models
                .get_mut(&command.model)
                .ok_or_else(|| std::io::Error::other("unknown model"))?;
            match &command.change {
                Change::None => {}
                Change::Put { key, value } => {
                    records.insert(key.clone(), value.clone());
                }
                Change::Delete { key } => {
                    records.remove(key);
                }
            }
            command.reply.clone()
        };
        self.revision = self
            .revision
            .checked_add(1)
            .ok_or_else(|| std::io::Error::other("revision exhausted"))?;
        self.results.push_back(Receipt {
            model: command.model.clone(),
            request_id: command.request_id.clone(),
            fingerprint: command.fingerprint,
            reply: reply.clone(),
        });
        while self.results.len() > RESULTS {
            self.results.pop_front();
        }
        Ok(reply)
    }
    fn meta(&self) -> SnapshotMeta<u64, ()> {
        SnapshotMeta {
            last_log_id: self.applied,
            last_membership: self.membership.clone(),
            snapshot_id: format!(
                "native-v1-{}-{}",
                self.applied.map_or(0, |i| i.index),
                self.revision
            ),
        }
    }
    fn validate(&self, meta: &SnapshotMeta<u64, ()>, expected: &State) -> std::io::Result<()> {
        if self.version != 1
            || self.contract != expected.contract
            || self.models.keys().ne(expected.models.keys())
            || self.applied != meta.last_log_id
            || self.membership != meta.last_membership
            || self.results.len() > RESULTS
        {
            return Err(std::io::Error::other(
                "native checkpoint contract, membership or applied position mismatch",
            ));
        }
        let mut seen = std::collections::BTreeSet::new();
        for r in &self.results {
            if !self.models.contains_key(&r.model) || !seen.insert((&r.model, &r.request_id)) {
                return Err(std::io::Error::other("invalid retained request results"));
            }
        }
        Ok(())
    }
}

pub(super) fn invalid(message: &str) -> StorageError<u64> {
    StorageError::from_io_error(
        ErrorSubject::StateMachine,
        ErrorVerb::Write,
        std::io::Error::other(message),
    )
}

#[derive(Clone)]
pub(super) struct Machine {
    pub durable: DurableLog<Config>,
    pub state: Arc<RwLock<State>>,
    gate: Arc<Mutex<()>>,
    pub failed: Arc<AtomicBool>,
    pub installs: Arc<AtomicU64>,
}
impl Machine {
    pub async fn restore(
        durable: DurableLog<Config>,
        empty: State,
    ) -> Result<Self, StorageError<u64>> {
        let state = if let Some(saved) = durable.snapshot().await? {
            let state: State =
                serde_json::from_slice(&saved.data).map_err(|e| invalid(&e.to_string()))?;
            state.validate(&saved.meta, &empty).map_err(|e| invalid(&e.to_string()))?;
            state
        } else {
            // The durable manifest already binds the application. A crash
            // before first apply replays the committed journal from index zero.
            empty
        };
        Ok(Self {
            durable,
            state: Arc::new(RwLock::new(state)),
            gate: Arc::new(Mutex::new(())),
            failed: Arc::new(AtomicBool::new(false)),
            installs: Arc::new(AtomicU64::new(0)),
        })
    }
    async fn publish(
        &self,
        state: State,
        meta: SnapshotMeta<u64, ()>,
    ) -> Result<(), StorageError<u64>> {
        // A failure remains sticky, even if cancellation/panic interrupts apply.
        // The state-machine worker owns the operation after HTTP cancellation.
        self.failed.store(true, Ordering::Release);
        let data = serde_json::to_vec(&state).map_err(|e| invalid(&e.to_string()))?;
        if data.len() > MAX_CHECKPOINT {
            return Err(invalid("native checkpoint exceeds supported capacity"));
        }
        self.durable.save_snapshot(meta, data).await?;
        *self.state.write().await = state;
        self.failed.store(false, Ordering::Release);
        Ok(())
    }
}
impl RaftSnapshotBuilder<Config> for Machine {
    async fn build_snapshot(&mut self) -> Result<Snapshot<Config>, StorageError<u64>> {
        let gate = self.gate.clone();
        let _gate = gate.lock().await;
        self.get_current_snapshot()
            .await?
            .ok_or_else(|| invalid("missing native checkpoint"))
    }
}
impl RaftLogReader<Config> for Machine {
    async fn try_get_log_entries<R: RangeBounds<u64> + Clone + std::fmt::Debug + Send>(
        &mut self,
        range: R,
    ) -> Result<Vec<Entry<Config>>, StorageError<u64>> {
        self.durable.try_get_log_entries(range).await
    }
}
impl RaftStorage<Config> for Machine {
    type LogReader = DurableLog<Config>;
    type SnapshotBuilder = Self;
    async fn get_log_reader(&mut self) -> Self::LogReader {
        self.durable.clone()
    }
    async fn get_log_state(&mut self) -> Result<LogState<Config>, StorageError<u64>> {
        self.durable.get_log_state().await
    }
    async fn save_vote(&mut self, vote: &Vote<u64>) -> Result<(), StorageError<u64>> {
        self.durable.save_vote(vote).await
    }
    async fn read_vote(&mut self) -> Result<Option<Vote<u64>>, StorageError<u64>> {
        self.durable.read_vote().await
    }
    async fn save_committed(&mut self, id: Option<LogId<u64>>) -> Result<(), StorageError<u64>> {
        self.durable.save_committed(id).await
    }
    async fn read_committed(&mut self) -> Result<Option<LogId<u64>>, StorageError<u64>> {
        self.durable.read_committed().await
    }
    async fn append_to_log<I: IntoIterator<Item = Entry<Config>> + Send>(
        &mut self,
        entries: I,
    ) -> Result<(), StorageError<u64>> {
        self.durable.append_to_log(entries).await
    }
    async fn delete_conflict_logs_since(
        &mut self,
        id: LogId<u64>,
    ) -> Result<(), StorageError<u64>> {
        self.durable.delete_conflict_logs_since(id).await
    }
    async fn purge_logs_upto(&mut self, id: LogId<u64>) -> Result<(), StorageError<u64>> {
        self.durable.wait_for_snapshot(id).await?;
        self.durable.purge_logs_upto(id).await?;
        self.durable.compact().await
    }
    async fn last_applied_state(
        &mut self,
    ) -> Result<(Option<LogId<u64>>, StoredMembership<u64, ()>), StorageError<u64>> {
        let state = self.state.read().await;
        Ok((state.applied, state.membership.clone()))
    }
    async fn apply_to_state_machine(
        &mut self,
        entries: &[Entry<Config>],
    ) -> Result<Vec<Reply>, StorageError<u64>> {
        let _gate = self.gate.lock().await;
        if self.failed.load(Ordering::Acquire) {
            return Err(invalid("native apply failed; reopen required"));
        }
        let mut next = self.state.read().await.clone();
        let mut results = Vec::with_capacity(entries.len());
        for entry in entries {
            if next.applied.is_some_and(|id| entry.log_id.index != id.index + 1) {
                return Err(invalid("nonconsecutive application log"));
            }
            let reply = match &entry.payload {
                EntryPayload::Blank => Reply::empty(),
                EntryPayload::Membership(membership) => {
                    next.membership = StoredMembership::new(Some(entry.log_id), membership.clone());
                    Reply::empty()
                }
                EntryPayload::Normal(command) => {
                    next.execute(command).map_err(|e| invalid(&e.to_string()))?
                }
            };
            next.applied = Some(entry.log_id);
            results.push(reply);
        }
        if !entries.is_empty() {
            let meta = next.meta();
            self.publish(next, meta).await?;
        }
        Ok(results)
    }
    async fn get_snapshot_builder(&mut self) -> Self {
        self.clone()
    }
    async fn begin_receiving_snapshot(
        &mut self,
    ) -> Result<Box<Cursor<Vec<u8>>>, StorageError<u64>> {
        Ok(Box::new(Cursor::new(Vec::new())))
    }
    async fn install_snapshot(
        &mut self,
        meta: &SnapshotMeta<u64, ()>,
        snapshot: Box<Cursor<Vec<u8>>>,
    ) -> Result<(), StorageError<u64>> {
        let _gate = self.gate.lock().await;
        let next: State =
            serde_json::from_slice(snapshot.get_ref()).map_err(|e| invalid(&e.to_string()))?;
        next.validate(meta, &*self.state.read().await)
            .map_err(|e| invalid(&e.to_string()))?;
        self.publish(next, meta.clone()).await?;
        self.installs.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
    async fn get_current_snapshot(
        &mut self,
    ) -> Result<Option<Snapshot<Config>>, StorageError<u64>> {
        Ok(self
            .durable
            .snapshot()
            .await?
            .map(|s| Snapshot { meta: s.meta, snapshot: Box::new(Cursor::new(s.data.to_vec())) }))
    }
}
