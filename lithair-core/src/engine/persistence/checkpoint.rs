//! Single-file native checkpoints. The atomically replaced snapshot is also the
//! manifest selecting the journal generation and its already-applied prefix.
use super::{EngineError, EngineResult, FileStorage};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::Ordering;

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Checkpoint {
    version: u32,
    journal: String,
    offset: u64,
    pub covered_events: usize,
    prefix_sha256: String,
    pub last_event_hash: Option<String>,
    state: String,
}
#[derive(Serialize, Deserialize)]
struct Manifest {
    __lithair_native_checkpoint: Checkpoint,
    sha256: String,
}
fn error(e: impl std::fmt::Display) -> EngineError {
    EngineError::PersistenceError(format!("native checkpoint: {e}"))
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn journal_name(name: &str) -> bool {
    name == "events.raftlog"
        || name
            .strip_prefix("native-journal-")
            .and_then(|s| s.strip_suffix(".raftlog"))
            .is_some_and(|s| uuid::Uuid::parse_str(s).is_ok())
}
fn prefix_digest(path: &Path, len: u64) -> EngineResult<String> {
    let file = File::open(path).map_err(error)?;
    if file.metadata().map_err(error)?.len() < len {
        return Err(error("journal shorter than checkpoint boundary"));
    }
    let mut reader = file.take(len);
    let mut hash = Sha256::new();
    std::io::copy(&mut reader, &mut hash).map_err(error)?;
    Ok(format!("{:x}", hash.finalize()))
}
impl FileStorage {
    pub(super) fn check_checkpoint_writable(&self) -> EngineResult<()> {
        if self.checkpoint_failed.load(Ordering::Acquire) {
            return Err(error("publication failed; reopen before writing"));
        }
        Ok(())
    }
    pub(super) fn checkpoint(&self) -> EngineResult<Option<Checkpoint>> {
        let content = match fs::read_to_string(&self.snapshot_file) {
            Ok(v) => v,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(error(e)),
        };
        let value: serde_json::Value = serde_json::from_str(&content).map_err(error)?;
        if value.get("__lithair_native_checkpoint").is_none() {
            return Ok(None);
        }
        let manifest: Manifest = serde_json::from_value(value).map_err(error)?;
        let cp = manifest.__lithair_native_checkpoint;
        if cp.version != 1
            || !journal_name(&cp.journal)
            || digest(&serde_json::to_vec(&cp).map_err(error)?) != manifest.sha256
        {
            return Err(error("invalid version, journal name or manifest checksum"));
        }
        let path = Path::new(&self.base_path).join(&cp.journal);
        if prefix_digest(&path, cp.offset)? != cp.prefix_sha256 {
            return Err(error("checkpoint journal prefix checksum mismatch"));
        }
        serde_json::from_str::<serde_json::Value>(&cp.state).map_err(error)?;
        Ok(Some(cp))
    }
    pub(super) fn restore_checkpoint(&mut self) -> EngineResult<()> {
        if let Some(cp) = self.checkpoint()? {
            self.checkpoint_active.store(true, Ordering::Release);
            self.events_file =
                Path::new(&self.base_path).join(cp.journal).to_string_lossy().into_owned();
            self.index_file = Path::new(&self.events_file)
                .with_extension("raftidx")
                .to_string_lossy()
                .into_owned();
        }
        if !self.checkpoint_active.load(Ordering::Acquire) {
            for entry in fs::read_dir(&self.base_path).map_err(error)? {
                let entry = entry.map_err(error)?;
                if entry
                    .file_name()
                    .to_str()
                    .is_some_and(|n| n != "events.raftlog" && journal_name(n))
                {
                    return Err(error("checkpoint missing but journal generations exist"));
                }
            }
        }
        Ok(())
    }
    pub(crate) fn checkpoint_boundary(&self) -> EngineResult<(usize, Option<String>)> {
        Ok(self
            .checkpoint()?
            .map(|c| (c.covered_events, c.last_event_hash))
            .unwrap_or_default())
    }
    fn publish_checkpoint(&self, cp: Checkpoint) -> EngineResult<()> {
        self.check_checkpoint_writable()?;
        let sha256 = digest(&serde_json::to_vec(&cp).map_err(error)?);
        let bytes = serde_json::to_vec(&Manifest { __lithair_native_checkpoint: cp, sha256 })
            .map_err(error)?;
        let temporary = Path::new(&self.base_path)
            .join(format!("native-snapshot-{}.tmp", uuid::Uuid::new_v4()));
        let result = (|| {
            let mut file = OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&temporary)
                .map_err(error)?;
            file.write_all(&bytes).map_err(error)?;
            stage("snapshot_written");
            file.sync_all().map_err(error)?;
            stage("snapshot_synced");
            fs::rename(&temporary, &self.snapshot_file).map_err(error)?;
            stage("snapshot_renamed");
            File::open(&self.base_path).and_then(|f| f.sync_all()).map_err(error)?;
            stage("directory_synced");
            self.checkpoint_active.store(true, Ordering::Release);
            Ok(())
        })();
        if result.is_err() {
            self.checkpoint_failed.store(true, Ordering::Release);
        }
        result
    }
    pub(crate) fn save_checkpoint(
        &self,
        state: &str,
        covered_events: usize,
        last_event_hash: Option<String>,
    ) -> EngineResult<()> {
        self.check_checkpoint_writable()?;
        // This API borrows immutably for backwards compatibility. Callers must
        // explicitly drain their queues/buffers before capturing state.
        if self.async_writer.is_some()
            || !self.event_batch.is_empty()
            || self.writer.as_ref().is_some_and(|w| !w.buffer().is_empty())
            || self.binary_writer.as_ref().is_some_and(|w| !w.buffer().is_empty())
        {
            return Err(error(
                "flush pending events before snapshot; LT_OPT_PERSIST is unsupported",
            ));
        }
        serde_json::from_str::<serde_json::Value>(state).map_err(error)?;
        // Persist the covered prefix even if ordinary appends have fsync off.
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.events_file)
            .map_err(error)?;
        file.sync_all().map_err(error)?;
        let offset = file.metadata().map_err(error)?.len();
        File::open(&self.base_path).and_then(|f| f.sync_all()).map_err(error)?;
        stage("journal_synced");
        let cp = Checkpoint {
            version: 1,
            journal: Path::new(&self.events_file)
                .file_name()
                .and_then(|n| n.to_str())
                .ok_or_else(|| error("invalid journal path"))?
                .into(),
            offset,
            covered_events,
            prefix_sha256: prefix_digest(Path::new(&self.events_file), offset)?,
            last_event_hash,
            state: state.into(),
        };
        self.publish_checkpoint(cp)
    }
    pub(super) fn compact_checkpoint(&mut self) -> EngineResult<()> {
        self.check_checkpoint_writable()?;
        if self.async_writer.is_some() {
            return Err(error("LT_OPT_PERSIST cannot compact safely"));
        }
        self.flush_events()?;
        let mut cp = self
            .checkpoint()?
            .ok_or_else(|| error("truncation requires a current checkpoint"))?;
        if fs::metadata(&self.events_file).map_err(error)?.len() != cp.offset {
            return Err(error("journal advanced since snapshot; capture a new checkpoint"));
        }
        let next = format!("native-journal-{}.raftlog", uuid::Uuid::new_v4());
        let next_path = Path::new(&self.base_path).join(&next);
        OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&next_path)
            .and_then(|f| f.sync_all())
            .map_err(error)?;
        File::open(&self.base_path).and_then(|f| f.sync_all()).map_err(error)?;
        stage("new_journal_synced");
        cp.journal = next;
        cp.offset = 0;
        cp.covered_events = 0;
        cp.prefix_sha256 = digest(&[]);
        self.publish_checkpoint(cp)?;
        self.writer = None;
        self.binary_writer = None;
        self.index_writer = None;
        let old_index = self.index_file.clone();
        self.events_file = next_path.to_string_lossy().into_owned();
        self.index_file = next_path.with_extension("raftidx").to_string_lossy().into_owned();
        // Generation-specific offsets cannot survive into a different journal.
        let _ = fs::remove_file(old_index);
        if let Err(error) = self.reclaim_checkpoint_files() {
            // Selection already changed; callers must reopen to reset their
            // journal counters before admitting another append.
            self.checkpoint_failed.store(true, Ordering::Release);
            return Err(error);
        }
        Ok(())
    }
    fn reclaim_checkpoint_files(&self) -> EngineResult<()> {
        for entry in fs::read_dir(&self.base_path).map_err(error)? {
            let entry = entry.map_err(error)?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            let temporary = name
                .strip_prefix("native-snapshot-")
                .and_then(|s| s.strip_suffix(".tmp"))
                .is_some_and(|s| uuid::Uuid::parse_str(s).is_ok());
            let obsolete_index = name
                .strip_suffix(".raftidx")
                .and_then(|s| s.strip_prefix("native-journal-"))
                .is_some_and(|s| uuid::Uuid::parse_str(s).is_ok())
                && entry.path() != Path::new(&self.index_file);
            if (journal_name(name) || temporary || obsolete_index)
                && entry.path() != Path::new(&self.events_file)
            {
                // Publication is already durable; failure only leaves garbage.
                if let Err(e) = fs::remove_file(entry.path()) {
                    log::warn!("checkpoint cleanup: {e}");
                }
            }
        }
        File::open(&self.base_path).and_then(|f| f.sync_all()).map_err(error)?;
        stage("reclaimed");
        Ok(())
    }
    pub(super) fn load_checkpoint_state(&self) -> EngineResult<Option<String>> {
        if let Some(cp) = self.checkpoint()? {
            return Ok(Some(cp.state));
        }
        match fs::read_to_string(&self.snapshot_file) {
            Ok(value) => Ok(Some(value)), // Legacy raw JSON snapshot.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(error(e)),
        }
    }
}
#[cfg(not(test))]
fn stage(_: &str) {}
#[cfg(test)]
fn stage(name: &str) {
    tests::stage(name);
}
#[cfg(test)]
mod tests;
