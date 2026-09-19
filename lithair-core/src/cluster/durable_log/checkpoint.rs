//! Atomic generation selection for snapshots and physical journal compaction.
use super::*;
use bytes::Bytes;
use sha2::{Digest, Sha256};

pub(super) const JOURNAL_MAGIC: &[u8; 8] = b"LTRLOG02";
const SNAPSHOT_MAGIC: &[u8; 8] = b"LTRSNP01";
pub(crate) const MAX_SNAPSHOT: usize = 64 * 1024 * 1024;
const MAX_SNAPSHOT_FILE: u64 = (MAX_SNAPSHOT + MAX_FRAME + 56) as u64;

#[derive(Clone)]
pub(crate) struct SavedSnapshot<C: RaftTypeConfig> {
    pub meta: SnapshotMeta<C::NodeId, C::Node>,
    pub data: Bytes,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SnapshotPointer {
    generation: [u8; 16],
    bytes: u64,
    digest: [u8; 32],
}
fn name(prefix: &str, generation: [u8; 16]) -> String {
    format!("{prefix}-{}", uuid::Uuid::from_bytes(generation).simple())
}
impl DurableEnd {
    pub(super) fn journal_name(&self) -> String {
        self.journal.map(|id| name("journal", id)).unwrap_or_else(|| "journal".into())
    }
}

pub(super) fn load_snapshot<C: RaftTypeConfig>(
    directory: &Path,
    end: &DurableEnd,
) -> io::Result<Option<SavedSnapshot<C>>> {
    let Some(pointer) = &end.snapshot else {
        return Ok(None);
    };
    if pointer.bytes > MAX_SNAPSHOT_FILE || pointer.bytes < 56 {
        return Err(invalid("invalid snapshot file length"));
    }
    let mut file = File::open(directory.join(name("snapshot", pointer.generation)))?;
    if file.metadata()?.len() != pointer.bytes {
        return Err(invalid("snapshot length mismatch"));
    }
    let mut bytes = vec![0; pointer.bytes as usize];
    file.read_exact(&mut bytes)?;
    if <[u8; 32]>::from(Sha256::digest(&bytes)) != pointer.digest
        || &bytes[..8] != SNAPSHOT_MAGIC
        || bytes[8..24] != end.identity
        || bytes[24..40] != pointer.generation
    {
        return Err(invalid("snapshot checksum, version or identity mismatch"));
    }
    let mut cursor = io::Cursor::new(&bytes[40..]);
    let (meta, meta_len): (SnapshotMeta<C::NodeId, C::Node>, _) =
        decode(&mut cursor, pointer.bytes - 40)?;
    let mut len = [0; 8];
    cursor.read_exact(&mut len)?;
    let len = u64::from_le_bytes(len);
    if len > MAX_SNAPSHOT as u64 || 48 + meta_len + len != pointer.bytes {
        return Err(invalid("invalid snapshot payload length"));
    }
    validate_meta::<C>(&meta)?;
    Ok(Some(SavedSnapshot {
        meta,
        data: Bytes::from(bytes).slice((48 + meta_len) as usize..),
    }))
}

fn validate_meta<C: RaftTypeConfig>(meta: &SnapshotMeta<C::NodeId, C::Node>) -> io::Result<()> {
    if let Some(membership) = meta.last_membership.log_id() {
        if meta
            .last_log_id
            .as_ref()
            .is_none_or(|last| membership > last || membership.index > last.index)
        {
            return Err(invalid("snapshot membership exceeds applied index"));
        }
    }
    Ok(())
}

impl<C: RaftTypeConfig> Inner<C> {
    pub(super) fn activate(&mut self, end: &DurableEnd) -> io::Result<()> {
        let path = self.directory.join(name("manifest", *uuid::Uuid::new_v4().as_bytes()));
        let mut metadata = OpenOptions::new().write(true).create_new(true).open(&path)?;
        self.checkpoint(Stage::MetadataWrite)?;
        metadata.write_all(&encode(end)?)?;
        self.checkpoint(Stage::MetadataSync)?;
        metadata.sync_all()?;
        self.checkpoint(Stage::MetadataRename)?;
        std::fs::rename(&path, self.directory.join("durable.meta"))?;
        self.checkpoint(Stage::DirectorySync)?;
        self.dir.sync_all()?;
        Ok(())
    }

    // Only remove this store's reserved generation names, after the selected
    // manifest and all referenced files have been validated and synced.
    pub(super) fn reclaim_generations(&mut self) -> io::Result<()> {
        let current_journal = self.end.journal_name();
        let current_snapshot = self.end.snapshot.as_ref().map(|s| name("snapshot", s.generation));
        let mut removed = false;
        for entry in std::fs::read_dir(&self.directory)? {
            let entry = entry?;
            let filename = entry.file_name();
            let Some(filename) = filename.to_str() else {
                continue;
            };
            let generation = filename
                .strip_prefix("journal-")
                .or_else(|| filename.strip_prefix("snapshot-"))
                .or_else(|| filename.strip_prefix("manifest-"));
            let owned = generation
                .is_some_and(|s| s.len() == 32 && uuid::Uuid::parse_str(s).is_ok())
                || (filename == "journal" && self.end.journal.is_some());
            if owned && filename != current_journal && Some(filename) != current_snapshot.as_deref()
            {
                std::fs::remove_file(entry.path())?;
                removed = true;
            }
        }
        if removed {
            self.dir.sync_all()?;
        }
        Ok(())
    }

    fn save_snapshot(
        &mut self,
        meta: SnapshotMeta<C::NodeId, C::Node>,
        data: Vec<u8>,
    ) -> io::Result<()> {
        validate_meta::<C>(&meta)?;
        if data.len() > MAX_SNAPSHOT {
            return Err(invalid("snapshot exceeds 64 MiB"));
        }
        let floor = self.snapshot.as_ref().and_then(|s| s.meta.last_log_id.as_ref());
        for floor in [floor, self.state.purged.as_ref()].into_iter().flatten() {
            if meta
                .last_log_id
                .as_ref()
                .is_none_or(|new| new < floor || new.index < floor.index)
            {
                return Err(invalid("snapshot must not move backwards"));
            }
        }
        let encoded_meta = encode(&meta)?;
        let generation = *uuid::Uuid::new_v4().as_bytes();
        let mut prefix = Vec::new();
        prefix.extend_from_slice(SNAPSHOT_MAGIC);
        prefix.extend_from_slice(&self.end.identity);
        prefix.extend_from_slice(&generation);
        prefix.extend_from_slice(&encoded_meta);
        prefix.extend_from_slice(&(data.len() as u64).to_le_bytes());
        let mut hash = Sha256::new();
        hash.update(&prefix);
        hash.update(&data);
        let pointer = SnapshotPointer {
            generation,
            bytes: (prefix.len() + data.len()) as u64,
            digest: hash.finalize().into(),
        };
        let end = DurableEnd { version: 2, snapshot: Some(pointer), ..self.end.clone() };
        self.failed = true;
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(self.directory.join(name("snapshot", generation)))?;
        file.write_all(&prefix)?;
        self.checkpoint(Stage::GenerationWrite)?;
        file.write_all(&data)?;
        self.checkpoint(Stage::GenerationSync)?;
        file.sync_all()?;
        self.checkpoint(Stage::GenerationDirectorySync)?;
        self.dir.sync_all()?;
        self.activate(&end)?;
        self.end = end;
        self.snapshot = Some(SavedSnapshot { meta, data: Bytes::from(data) });
        self.checkpoint(Stage::Cleanup)?;
        self.reclaim_generations()?;
        self.checkpoint(Stage::Complete)?;
        self.failed = false;
        self.snapshot_changed.notify_waiters();
        Ok(())
    }

    fn compact(&mut self) -> io::Result<()>
    where
        C::D: Clone,
    {
        let generation = *uuid::Uuid::new_v4().as_bytes();
        self.failed = true;
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(self.directory.join(name("journal", generation)))?;
        file.write_all(JOURNAL_MAGIC)?;
        file.write_all(&self.end.identity)?;
        file.write_all(&generation)?;
        self.checkpoint(Stage::GenerationWrite)?;
        let mut writer = io::BufWriter::new(file);
        // Reconstitute the same logical state, including uncommitted suffixes.
        // Apply the committed watermark last to avoid rejecting retained entries.
        if let Some(vote) = &self.state.vote {
            writer.write_all(&encode(&Record::<C>::Vote(vote.clone()))?)?;
        }
        if let Some(id) = &self.state.purged {
            writer.write_all(&encode(&Record::<C>::Purge(id.clone()))?)?;
        }
        for entry in self.state.logs.values() {
            writer.write_all(&encode(&Record::<C>::Append(vec![entry.clone()]))?)?;
        }
        writer.write_all(&encode(&Record::<C>::Committed(self.state.committed.clone()))?)?;
        writer.flush()?;
        let mut file = writer.into_inner().map_err(|e| e.into_error())?;
        let offset = file.stream_position()?;
        self.checkpoint(Stage::GenerationSync)?;
        file.sync_all()?;
        self.checkpoint(Stage::GenerationDirectorySync)?;
        self.dir.sync_all()?;
        let end = DurableEnd { version: 2, journal: Some(generation), offset, ..self.end.clone() };
        self.activate(&end)?;
        self.end = end;
        self.journal = file;
        self.checkpoint(Stage::Cleanup)?;
        self.reclaim_generations()?;
        self.checkpoint(Stage::Complete)?;
        self.failed = false;
        self.snapshot_changed.notify_waiters();
        Ok(())
    }
}

impl<C: RaftTypeConfig> DurableLog<C> {
    /// Caller supplies a complete, validated state-machine cut. Publication does
    /// not apply it to application stores; the caller must do that before serving.
    pub(crate) async fn save_snapshot(
        &self,
        meta: SnapshotMeta<C::NodeId, C::Node>,
        data: Vec<u8>,
    ) -> Result<(), StorageError<C::NodeId>> {
        self.access(ErrorVerb::Write, move |inner| inner.save_snapshot(meta, data))
            .await
    }
    pub(crate) async fn snapshot(
        &self,
    ) -> Result<Option<SavedSnapshot<C>>, StorageError<C::NodeId>> {
        self.access(ErrorVerb::Read, |inner| Ok(inner.snapshot.clone())).await
    }
    /// OpenRaft may schedule purge while its state-machine worker is still
    /// installing the snapshot. Wait outside the storage lock; callers must use
    /// independently locked log and state-machine adapters. A bounded wait fails
    /// closed if the application snapshot worker never publishes its result.
    pub(crate) async fn wait_for_snapshot(
        &self,
        id: LogId<C::NodeId>,
    ) -> Result<(), StorageError<C::NodeId>> {
        let changed =
            self.access(ErrorVerb::Read, |inner| Ok(inner.snapshot_changed.clone())).await?;
        tokio::time::timeout(std::time::Duration::from_secs(30), async {
            loop {
                let notified = changed.notified();
                tokio::pin!(notified);
                notified.as_mut().enable();
                let snapshot = self.snapshot().await?;
                if snapshot
                    .as_ref()
                    .and_then(|s| s.meta.last_log_id.as_ref())
                    .is_some_and(|last| last >= &id && last.index >= id.index)
                {
                    return Ok(());
                }
                notified.await;
            }
        })
        .await
        .map_err(|e| {
            storage_error::<C>(ErrorVerb::Read, io::Error::new(io::ErrorKind::TimedOut, e))
        })?
    }

    /// Rewrites retained state only. Purging requires a state-machine checkpoint;
    /// compaction itself never advances the purge or commit watermark.
    pub(crate) async fn compact(&self) -> Result<(), StorageError<C::NodeId>>
    where
        C::D: Clone,
    {
        self.access(ErrorVerb::Write, |inner| inner.compact()).await
    }
}
