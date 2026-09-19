//! Private OpenRaft log/vote foundation. No application state machine or legacy
//! WAL migration. See docs/internal/specs/OPENRAFT_STORAGE.md for the disk contract.

use openraft::storage::Adaptor;
use openraft::{
    Entry, ErrorSubject, ErrorVerb, LogId, LogState, RaftLogReader, RaftSnapshotBuilder,
    RaftStorage, RaftTypeConfig, Snapshot, SnapshotMeta, StorageError, StoredMembership, Vote,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt::Debug;
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::ops::{Bound, RangeBounds};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;

const MAGIC: &[u8; 8] = b"LTRLOG01";
const HEADER_LEN: u64 = 24;
const MAX_FRAME: usize = 8 * 1024 * 1024;
const MAX_ENTRIES: usize = 4096;

#[derive(Serialize, Deserialize)]
#[serde(bound = "", deny_unknown_fields)]
enum Record<C: RaftTypeConfig> {
    Vote(Vote<C::NodeId>),
    Append(Vec<Entry<C>>),
    Truncate(LogId<C::NodeId>),
    Purge(LogId<C::NodeId>),
    Committed(Option<LogId<C::NodeId>>),
}

#[derive(Default)]
struct State<C: RaftTypeConfig> {
    vote: Option<Vote<C::NodeId>>,
    logs: BTreeMap<u64, Entry<C>>,
    purged: Option<LogId<C::NodeId>>,
    committed: Option<LogId<C::NodeId>>,
}

impl<C: RaftTypeConfig> State<C> {
    fn validate(&self, record: &Record<C>) -> io::Result<()> {
        match record {
            Record::Append(entries) => {
                if entries.len() > MAX_ENTRIES {
                    return Err(invalid("too many entries in append"));
                }
                if entries.windows(2).any(|pair| {
                    pair[0].log_id.index.checked_add(1) != Some(pair[1].log_id.index)
                        || pair[0].log_id >= pair[1].log_id
                }) {
                    return Err(invalid("nonconsecutive or decreasing append batch"));
                }
                // OpenRaft's recovery suite can supply entries already covered
                // by a snapshot. Ignore that prefix; never resurrect purged data.
                let entries = &entries[entries.partition_point(|e| {
                    self.purged.as_ref().is_some_and(|p| e.log_id.index <= p.index)
                })..];
                let Some(first) = entries.first() else {
                    return Ok(());
                };
                let last = &entries[entries.len() - 1];
                if self.committed.as_ref().is_some_and(|id| first.log_id.index <= id.index) {
                    return Err(invalid("append overlaps committed entries"));
                }
                let previous = self
                    .logs
                    .range(..first.log_id.index)
                    .next_back()
                    .map(|(_, e)| &e.log_id)
                    .or(self.purged.as_ref());
                let mut previous = previous;
                for entry in entries {
                    if let Some(prev) = previous {
                        if prev.index.checked_add(1) != Some(entry.log_id.index)
                            || prev >= &entry.log_id
                        {
                            return Err(invalid("nonconsecutive or decreasing log IDs"));
                        }
                    }
                    previous = Some(&entry.log_id);
                }
                if let Some((_, next)) =
                    self.logs.range((Bound::Excluded(last.log_id.index), Bound::Unbounded)).next()
                {
                    if last.log_id.index.checked_add(1) != Some(next.log_id.index)
                        || last.log_id >= next.log_id
                    {
                        return Err(invalid(
                            "append leaves an inconsistent suffix; truncate first",
                        ));
                    }
                }
            }
            Record::Vote(vote) => {
                if self.vote.as_ref().is_some_and(|old| {
                    !matches!(
                        vote.partial_cmp(old),
                        Some(std::cmp::Ordering::Equal | std::cmp::Ordering::Greater)
                    )
                }) {
                    return Err(invalid("vote must not decrease or become incomparable"));
                }
            }
            Record::Truncate(id) => {
                if self.purged.as_ref().is_some_and(|p| id.index <= p.index)
                    || self.committed.as_ref().is_some_and(|c| id.index <= c.index)
                {
                    return Err(invalid("cannot truncate purged or committed entries"));
                }
            }
            Record::Purge(id) => {
                if self.purged.as_ref().is_some_and(|p| id < p || id.index < p.index) {
                    return Err(invalid("purge watermark must not decrease"));
                }
                if self.logs.get(&id.index).is_some_and(|entry| &entry.log_id != id) {
                    return Err(invalid("purge log ID does not match the retained entry"));
                }
            }
            Record::Committed(id) => {
                if id < &self.committed
                    || self
                        .committed
                        .as_ref()
                        .is_some_and(|old| id.as_ref().is_none_or(|new| new.index < old.index))
                {
                    return Err(invalid("committed watermark must not decrease"));
                }
            }
        }
        Ok(())
    }

    fn apply(&mut self, record: Record<C>) {
        match record {
            Record::Vote(vote) => self.vote = Some(vote),
            Record::Append(entries) => self.logs.extend(
                entries
                    .into_iter()
                    .filter(|e| self.purged.as_ref().is_none_or(|p| e.log_id.index > p.index))
                    .map(|e| (e.log_id.index, e)),
            ),
            Record::Truncate(id) => {
                self.logs.split_off(&id.index);
            }
            Record::Purge(id) => {
                self.logs = match id.index.checked_add(1) {
                    Some(next) => self.logs.split_off(&next),
                    None => BTreeMap::new(),
                };
                self.purged = Some(id);
            }
            Record::Committed(id) => self.committed = id,
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DurableEnd {
    version: u32,
    identity: [u8; 16],
    offset: u64,
}

struct Inner<C: RaftTypeConfig> {
    directory: PathBuf,
    dir: File,
    _lock: DirectoryLock,
    journal: File,
    end: DurableEnd,
    state: State<C>,
    failed: bool,
    #[cfg(test)]
    fault: Option<(Stage, bool)>,
    #[cfg(test)]
    pause: Option<TestPause>,
}

struct DirectoryLock(File);
impl Drop for DirectoryLock {
    fn drop(&mut self) {
        // Explicit unlock also releases flock if another thread briefly forked
        // a child which inherited this descriptor before exec closes it.
        let _ = self.0.unlock();
    }
}

#[cfg(test)]
struct TestPause {
    stage: Stage,
    reached: tokio::sync::oneshot::Sender<()>,
    release: std::sync::mpsc::Receiver<()>,
}

/// All handles share one writer and lock, including log readers. Blocking disk
/// operations run in an owned task so dropping a future cannot cancel half a write.
#[derive(Clone)]
pub(crate) struct DurableLog<C: RaftTypeConfig>(Arc<Mutex<Inner<C>>>);

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
fn storage_error<C: RaftTypeConfig>(verb: ErrorVerb, error: io::Error) -> StorageError<C::NodeId> {
    StorageError::from_io_error(ErrorSubject::Store, verb, error)
}

/// A capped serializer avoids allocating an unbounded JSON frame before checking
/// its size. Frame checksums cover both the length and payload.
struct Buffer(Vec<u8>);
impl Write for Buffer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > MAX_FRAME.saturating_sub(self.0.len()) {
            return Err(invalid("frame exceeds 8 MiB"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
fn encode(value: &impl Serialize) -> io::Result<Vec<u8>> {
    let mut body = Buffer(Vec::new());
    serde_json::to_writer(&mut body, value).map_err(io::Error::other)?;
    let mut frame = Vec::with_capacity(body.0.len() + 8);
    frame.extend_from_slice(&(body.0.len() as u32).to_le_bytes());
    frame.extend_from_slice(&body.0);
    frame.extend_from_slice(&crc32fast::hash(&frame).to_le_bytes());
    Ok(frame)
}
fn decode<T: serde::de::DeserializeOwned>(file: &mut File, available: u64) -> io::Result<(T, u64)> {
    if available < 8 {
        return Err(invalid("truncated durable frame"));
    }
    let mut length = [0; 4];
    file.read_exact(&mut length)?;
    let size = u32::from_le_bytes(length) as usize;
    if size > MAX_FRAME || size as u64 + 8 > available {
        return Err(invalid("invalid durable frame length"));
    }
    let mut frame = vec![0; size + 4];
    frame[..4].copy_from_slice(&length);
    file.read_exact(&mut frame[4..])?;
    let mut checksum = [0; 4];
    file.read_exact(&mut checksum)?;
    if crc32fast::hash(&frame) != u32::from_le_bytes(checksum) {
        return Err(invalid("durable frame checksum mismatch"));
    }
    let value = serde_json::from_slice(&frame[4..]).map_err(io::Error::other)?;
    Ok((value, size as u64 + 8))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Stage {
    PartialWrite,
    JournalSync,
    MetadataWrite,
    MetadataSync,
    MetadataRename,
    DirectorySync,
    Complete,
}

impl<C: RaftTypeConfig> Inner<C> {
    fn open(directory: PathBuf, create: bool) -> io::Result<Self> {
        // The caller provisions a durable, dedicated directory. Never interpret
        // an existing legacy WAL or a partially initialized store as empty.
        let directory = directory.canonicalize()?;
        let dir = File::open(&directory)?;
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(create)
            .truncate(false)
            .open(directory.join("LOCK"))?;
        lock.try_lock().map_err(io::Error::other)?;
        let lock = DirectoryLock(lock);
        let journal_path = directory.join("journal");
        let metadata_path = directory.join("durable.meta");
        let (mut journal, end) = if create {
            for entry in std::fs::read_dir(&directory)? {
                if entry?.file_name() != "LOCK" {
                    return Err(invalid(
                        "new OpenRaft store requires an empty dedicated directory",
                    ));
                }
            }
            let identity = *uuid::Uuid::new_v4().as_bytes();
            let mut new = tempfile::NamedTempFile::new_in(&directory)?;
            new.write_all(MAGIC)?;
            new.write_all(&identity)?;
            new.as_file().sync_all()?;
            let journal = new.persist(&journal_path).map_err(|e| e.error)?;
            dir.sync_all()?;
            let end = DurableEnd { version: 1, identity, offset: HEADER_LEN };
            let mut metadata = tempfile::NamedTempFile::new_in(&directory)?;
            metadata.write_all(&encode(&end)?)?;
            metadata.as_file().sync_all()?;
            metadata.persist(&metadata_path).map_err(|e| e.error)?;
            dir.sync_all()?;
            (journal, end)
        } else {
            let journal = OpenOptions::new().read(true).write(true).open(&journal_path)?;
            let mut metadata = File::open(&metadata_path)?;
            let size = metadata.metadata()?.len();
            let (end, read): (DurableEnd, _) = decode(&mut metadata, size)?;
            if read != size || end.version != 1 {
                return Err(invalid("unsupported or invalid durable metadata format"));
            }
            (journal, end)
        };
        journal.seek(SeekFrom::Start(0))?;
        let mut header = [0; HEADER_LEN as usize];
        journal.read_exact(&mut header)?;
        if &header[..8] != MAGIC || header[8..] != end.identity {
            return Err(invalid(
                "unsupported journal format or journal/metadata identity mismatch",
            ));
        }
        if end.offset < HEADER_LEN || end.offset > journal.metadata()?.len() {
            return Err(invalid("durable journal prefix is missing"));
        }
        let mut state = State::<C>::default();
        let mut offset = HEADER_LEN;
        while offset < end.offset {
            let (record, size) = decode(&mut journal, end.offset - offset)?;
            state.validate(&record)?;
            state.apply(record);
            offset += size;
        }
        // Only bytes outside the durable boundary may be discarded. They were
        // never acknowledged. Damage inside that boundary was rejected above.
        journal.set_len(end.offset)?;
        journal.sync_all()?;
        journal.seek(SeekFrom::Start(end.offset))?;
        // A preceding process may have died after rename but before directory
        // sync. Stabilize the selected metadata before allowing any new response.
        dir.sync_all()?;
        Ok(Self {
            directory,
            dir,
            _lock: lock,
            journal,
            end,
            state,
            failed: false,
            #[cfg(test)]
            fault: None,
            #[cfg(test)]
            pause: None,
        })
    }

    fn checkpoint(&mut self, stage: Stage) -> io::Result<()> {
        #[cfg(test)]
        if self.pause.as_ref().is_some_and(|pause| pause.stage == stage) {
            if let Some(pause) = self.pause.take() {
                let _ = pause.reached.send(());
                pause
                    .release
                    .recv_timeout(std::time::Duration::from_secs(10))
                    .map_err(io::Error::other)?;
            }
        }
        #[cfg(test)]
        if self.fault.is_some_and(|(at, _)| at == stage) {
            if self.fault.is_some_and(|(_, exit)| exit) {
                std::process::exit(86);
            }
            return Err(io::Error::other(format!("injected failure at {stage:?}")));
        }
        let _ = stage;
        Ok(())
    }

    fn commit(&mut self, record: Record<C>) -> io::Result<()> {
        self.state.validate(&record)?;
        let frame = encode(&record)?;
        let offset = self
            .end
            .offset
            .checked_add(frame.len() as u64)
            .ok_or_else(|| invalid("journal offset overflow"))?;
        let end = DurableEnd { version: 1, identity: self.end.identity, offset };
        // After any I/O error the result may be indeterminate. Refuse all further
        // operations through every clone until close/reopen resolves the boundary.
        self.failed = true;
        self.journal.write_all(&frame[..frame.len() / 2])?;
        self.checkpoint(Stage::PartialWrite)?;
        self.journal.write_all(&frame[frame.len() / 2..])?;
        self.checkpoint(Stage::JournalSync)?;
        self.journal.sync_all()?;
        let mut metadata = tempfile::NamedTempFile::new_in(&self.directory)?;
        self.checkpoint(Stage::MetadataWrite)?;
        metadata.write_all(&encode(&end)?)?;
        self.checkpoint(Stage::MetadataSync)?;
        metadata.as_file().sync_all()?;
        self.checkpoint(Stage::MetadataRename)?;
        metadata.persist(self.directory.join("durable.meta")).map_err(|e| e.error)?;
        self.checkpoint(Stage::DirectorySync)?;
        self.dir.sync_all()?;
        self.checkpoint(Stage::Complete)?;
        self.end = end;
        self.state.apply(record);
        self.failed = false;
        Ok(())
    }
}

impl<C: RaftTypeConfig> DurableLog<C> {
    /// Explicit bootstrap only; never used as a fallback when recovery fails.
    pub(crate) async fn create(
        directory: impl AsRef<Path>,
    ) -> Result<Self, StorageError<C::NodeId>> {
        Self::load(directory, true).await
    }

    /// Reopen an initialized store. Missing files are always an error.
    pub(crate) async fn open(directory: impl AsRef<Path>) -> Result<Self, StorageError<C::NodeId>> {
        Self::load(directory, false).await
    }

    async fn load(
        directory: impl AsRef<Path>,
        create: bool,
    ) -> Result<Self, StorageError<C::NodeId>> {
        let directory = directory.as_ref().to_owned();
        tokio::task::spawn_blocking(move || {
            Inner::open(directory.clone(), create)
                .map(|inner| Self(Arc::new(Mutex::new(inner))))
                .map_err(|e| {
                    io::Error::new(e.kind(), format!("OpenRaft store {}: {e}", directory.display()))
                })
        })
        .await
        .map_err(|e| storage_error::<C>(ErrorVerb::Read, io::Error::other(e)))?
        .map_err(|e| storage_error::<C>(ErrorVerb::Read, e))
    }

    async fn access<T: Send + 'static>(
        &self,
        verb: ErrorVerb,
        f: impl FnOnce(&mut Inner<C>) -> io::Result<T> + Send + 'static,
    ) -> Result<T, StorageError<C::NodeId>> {
        // Acquire in async FIFO order before dispatching disk work. An abandoned
        // future cannot let a later vote overtake an already admitted append.
        let mut guard = self.0.clone().lock_owned().await;
        if guard.failed {
            return Err(storage_error::<C>(
                verb,
                io::Error::other("OpenRaft store failed; close all handles and reopen"),
            ));
        }
        tokio::task::spawn_blocking(move || {
            // Tokio's mutex does not poison itself on unwind. Isolate panicking
            // payload serializers/cloners and explicitly invalidate every handle.
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(&mut guard))) {
                Ok(result) => result,
                Err(_) => {
                    guard.failed = true;
                    Err(io::Error::other(
                        "OpenRaft storage operation panicked; close all handles and reopen",
                    ))
                }
            }
        })
        .await
        .map_err(|e| storage_error::<C>(verb, io::Error::other(e)))?
        .map_err(|e| storage_error::<C>(verb, e))
    }

    #[cfg(test)]
    pub(crate) async fn inject_failure(&self, stage: Stage, exit: bool) {
        self.access(ErrorVerb::Write, move |inner| {
            inner.fault = Some((stage, exit));
            Ok(())
        })
        .await
        .expect("inject fault");
    }

    #[cfg(test)]
    pub(crate) async fn pause_at(
        &self,
        stage: Stage,
        reached: tokio::sync::oneshot::Sender<()>,
        release: std::sync::mpsc::Receiver<()>,
    ) {
        self.access(ErrorVerb::Write, move |inner| {
            inner.pause = Some(TestPause { stage, reached, release });
            Ok(())
        })
        .await
        .expect("set pause");
    }
}

impl<C: RaftTypeConfig<Entry = Entry<C>>> RaftLogReader<C> for DurableLog<C>
where
    C::D: Clone,
{
    async fn try_get_log_entries<R: RangeBounds<u64> + Clone + Debug + Send>(
        &mut self,
        range: R,
    ) -> Result<Vec<C::Entry>, StorageError<C::NodeId>> {
        let bounds = (range.start_bound().cloned(), range.end_bound().cloned());
        // Avoid BTreeMap's panic for an invalid caller range.
        let start = match bounds.0 {
            Bound::Included(n) => Some(n),
            Bound::Excluded(n) => n.checked_add(1),
            Bound::Unbounded => Some(0),
        };
        let end = match bounds.1 {
            Bound::Included(n) => Some(n),
            Bound::Excluded(n) => n.checked_sub(1),
            Bound::Unbounded => Some(u64::MAX),
        };
        self.access(ErrorVerb::Read, move |inner| match (start, end) {
            (Some(start), Some(end)) if start <= end => {
                Ok(inner.state.logs.range(start..=end).map(|(_, e)| e.clone()).collect())
            }
            _ => Ok(Vec::new()),
        })
        .await
    }
}

impl<C: RaftTypeConfig<Entry = Entry<C>>> RaftStorage<C> for DurableLog<C>
where
    C::D: Clone,
{
    type LogReader = Self;
    type SnapshotBuilder = Self;
    async fn get_log_reader(&mut self) -> Self {
        self.clone()
    }
    async fn get_log_state(&mut self) -> Result<LogState<C>, StorageError<C::NodeId>> {
        self.access(ErrorVerb::Read, |i| {
            Ok(LogState {
                last_purged_log_id: i.state.purged.clone(),
                last_log_id: i
                    .state
                    .logs
                    .last_key_value()
                    .map(|(_, e)| e.log_id.clone())
                    .or_else(|| i.state.purged.clone()),
            })
        })
        .await
    }
    async fn save_vote(&mut self, vote: &Vote<C::NodeId>) -> Result<(), StorageError<C::NodeId>> {
        let vote = vote.clone();
        self.access(ErrorVerb::Write, move |i| i.commit(Record::Vote(vote))).await
    }
    async fn read_vote(&mut self) -> Result<Option<Vote<C::NodeId>>, StorageError<C::NodeId>> {
        self.access(ErrorVerb::Read, |i| Ok(i.state.vote.clone())).await
    }
    async fn save_committed(
        &mut self,
        id: Option<LogId<C::NodeId>>,
    ) -> Result<(), StorageError<C::NodeId>> {
        self.access(ErrorVerb::Write, move |i| i.commit(Record::Committed(id))).await
    }
    async fn read_committed(
        &mut self,
    ) -> Result<Option<LogId<C::NodeId>>, StorageError<C::NodeId>> {
        self.access(ErrorVerb::Read, |i| Ok(i.state.committed.clone())).await
    }
    async fn append_to_log<I>(&mut self, entries: I) -> Result<(), StorageError<C::NodeId>>
    where
        I: IntoIterator<Item = C::Entry> + Send,
    {
        let entries = entries.into_iter().take(MAX_ENTRIES + 1).collect();
        self.access(ErrorVerb::Write, move |i| i.commit(Record::Append(entries))).await
    }
    async fn delete_conflict_logs_since(
        &mut self,
        id: LogId<C::NodeId>,
    ) -> Result<(), StorageError<C::NodeId>> {
        self.access(ErrorVerb::Write, move |i| i.commit(Record::Truncate(id))).await
    }
    async fn purge_logs_upto(
        &mut self,
        id: LogId<C::NodeId>,
    ) -> Result<(), StorageError<C::NodeId>> {
        self.access(ErrorVerb::Write, move |i| i.commit(Record::Purge(id))).await
    }
    // Only the adapter's log half may be used. No fake application state machine
    // is provided: consensus integration must supply a separate durable one.
    async fn last_applied_state(
        &mut self,
    ) -> Result<
        (Option<LogId<C::NodeId>>, StoredMembership<C::NodeId, C::Node>),
        StorageError<C::NodeId>,
    > {
        Err(no_state_machine::<C>())
    }
    async fn apply_to_state_machine(
        &mut self,
        _: &[C::Entry],
    ) -> Result<Vec<C::R>, StorageError<C::NodeId>> {
        Err(no_state_machine::<C>())
    }
    async fn get_snapshot_builder(&mut self) -> Self {
        self.clone()
    }
    async fn begin_receiving_snapshot(
        &mut self,
    ) -> Result<Box<C::SnapshotData>, StorageError<C::NodeId>> {
        Err(no_state_machine::<C>())
    }
    async fn install_snapshot(
        &mut self,
        _: &SnapshotMeta<C::NodeId, C::Node>,
        _: Box<C::SnapshotData>,
    ) -> Result<(), StorageError<C::NodeId>> {
        Err(no_state_machine::<C>())
    }
    async fn get_current_snapshot(
        &mut self,
    ) -> Result<Option<Snapshot<C>>, StorageError<C::NodeId>> {
        Err(no_state_machine::<C>())
    }
}

fn no_state_machine<C: RaftTypeConfig>() -> StorageError<C::NodeId> {
    storage_error::<C>(
        ErrorVerb::Read,
        io::Error::new(
            io::ErrorKind::Unsupported,
            "log-only storage: supply a separate durable state machine",
        ),
    )
}
impl<C: RaftTypeConfig> RaftSnapshotBuilder<C> for DurableLog<C> {
    async fn build_snapshot(&mut self) -> Result<Snapshot<C>, StorageError<C::NodeId>> {
        Err(no_state_machine::<C>())
    }
}
impl<C: RaftTypeConfig<Entry = Entry<C>>> DurableLog<C>
where
    C::D: Clone,
{
    pub(crate) fn into_log_store(self) -> Adaptor<C, Self> {
        Adaptor::new(self).0
    }
}
