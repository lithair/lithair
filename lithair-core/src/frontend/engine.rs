//! Frontend SCC2 Engine - Lock-free Asset Management
//!
//! This module provides ultra-performance lock-free frontend asset serving
//! using SCC2 HashMap with event sourcing persistence.
//!
//! # Performance
//! - 40M+ ops/sec concurrent asset reads (vs RwLock bottleneck)
//! - Zero contention with SCC2 lock-free HashMap
//! - Event sourcing with .raftlog persistence
//! - Memory-first with zero disk I/O after load
//!
use super::assets::StaticAsset;
use crate::engine::{EngineError, EventStore, Scc2Engine, Scc2EngineConfig};
use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Arc;
use std::sync::RwLock;

// Keep the original bare StaticAsset representation readable (and writable).
// Tombstones carry a distinct tag so they cannot be mistaken for an upsert.
#[derive(Serialize, Deserialize)]
enum AssetMutation {
    AssetDeleted { path: String },
}

#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum StoredAssetEvent {
    Mutation(AssetMutation),
    Asset(Box<StaticAsset>),
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FrontendSnapshot {
    version: u32,
    host_id: String,
    assets: Vec<StaticAsset>,
}

/// Summary of a single hot reload of a [`FrontendEngine`].
///
/// Returned by [`FrontendEngine::reload`] so the admin API can report what
/// changed without re-reading the whole asset set.
#[derive(Debug, Clone)]
pub struct ReloadOutcome {
    /// Number of assets live after the reload.
    pub asset_count: usize,
    /// Total bytes of asset content live after the reload.
    pub total_bytes: u64,
    /// Content fingerprint after the reload (see [`FrontendEngine::version`]).
    pub version: String,
    /// `true` when the post-reload fingerprint differs from the pre-reload one.
    pub changed: bool,
}

/// Frontend Engine - Lock-free asset management with SCC2
pub struct FrontendEngine {
    /// SCC2 engine for ultra-fast lock-free access
    /// Keys: "{host_id}:{path}" (e.g., "rbac_demo:/index.html")
    pub engine: Arc<Scc2Engine<StaticAsset>>,

    /// Virtual host ID for this engine instance
    host_id: String,

    /// Filesystem directory this engine was last loaded from. Interior
    /// mutability (the engine is shared behind an `Arc` on the request path)
    /// so a reload can re-read the same source without `&mut self`.
    source_dir: RwLock<String>,

    /// Timestamp of the most recent successful load/reload, `None` until the
    /// first `load_directory`/`reload`. Used by the frontend admin API.
    last_reload_at: RwLock<Option<DateTime<Utc>>>,

    /// Cached SHA-256 fingerprint of the loaded asset set, computed once at
    /// mutation/replay. `version()` returns this instead of re-hashing every
    /// asset's content on each `GET /_admin/frontend` (PR #138 review:
    /// the previous per-request compute cloned + hashed all asset bytes).
    version: RwLock<String>,

    /// Cached sum of asset content sizes, updated at mutation/replay, so
    /// `total_bytes()` is an O(1) atomic load instead of iterating + cloning
    /// every asset on each call.
    total_bytes: std::sync::atomic::AtomicU64,

    /// Orders source scans, deduplication, publication and checkpoint capture.
    /// Asset reads continue to use SCC without acquiring this lock.
    mutations: tokio::sync::Mutex<()>,
}

impl FrontendEngine {
    /// Create a new frontend engine with event sourcing
    ///
    /// # Arguments
    /// * `host_id` - Virtual host identifier (e.g., "rbac_demo", "blog")
    /// * `data_dir` - Directory for .raftlog persistence
    ///
    /// # Returns
    /// Lock-free frontend engine with event sourcing
    ///
    /// Restores the checkpoint, streams persisted assets/deletion tombstones,
    /// then compacts history before returning. Memory scales with live assets
    /// and the largest event, rather than the complete historical journal.
    /// Invalid events fail startup rather than silently restoring partial state.
    pub async fn new(host_id: impl Into<String>, data_dir: impl AsRef<Path>) -> Result<Self> {
        let host_id = host_id.into();
        let data_path = data_dir.as_ref().join(format!("frontend_{}", host_id));

        // Create event store for persistence
        let event_store = EventStore::new_streaming_json(data_path.to_string_lossy().as_ref())?;
        let event_store_arc = Arc::new(RwLock::new(event_store));

        // Configure SCC2 engine for frontend assets
        let config = Scc2EngineConfig {
            verbose_logging: false,
            enable_snapshots: false, // Frontend owns its typed checkpoint below.
            snapshot_interval: 1000,
            enable_deduplication: true,
            auto_persist_writes: true,
            force_immediate_persistence: true, // Immediate persistence for assets
        };

        // Create SCC2 engine
        let engine = Scc2Engine::new(event_store_arc, config)?;

        let frontend = Self {
            engine: Arc::new(engine),
            host_id,
            source_dir: RwLock::new(String::new()),
            last_reload_at: RwLock::new(None),
            version: RwLock::new(String::new()),
            total_bytes: std::sync::atomic::AtomicU64::new(0),
            mutations: tokio::sync::Mutex::new(()),
        };
        {
            let store = frontend.engine.event_store();
            let mut store =
                store.write().map_err(|_| anyhow::anyhow!("frontend store poisoned"))?;
            let snapshot = store.load_snapshot()?;
            let had_snapshot = snapshot.is_some();
            if let Some(json) = snapshot {
                let snapshot: FrontendSnapshot = serde_json::from_str(&json)?;
                anyhow::ensure!(
                    snapshot.version == 1 && snapshot.host_id == frontend.host_id,
                    "incompatible frontend checkpoint"
                );
                for asset in snapshot.assets {
                    frontend.apply_stored_event(StoredAssetEvent::Asset(Box::new(asset)));
                }
            }
            store.replay_json_suffix(|json| {
                let event: StoredAssetEvent = serde_json::from_str(json).map_err(|e| {
                    EngineError::PersistenceError(format!(
                        "invalid frontend event for {}: {e}",
                        frontend.host_id
                    ))
                })?;
                frontend.apply_stored_event(event);
                Ok(())
            })?;
            if had_snapshot || store.event_count() != 0 {
                frontend.refresh_version_cache();
                // Reclaim legacy history immediately, even without a source
                // directory. This also cleans obsolete generations after an
                // interrupted earlier compaction.
                frontend.checkpoint(&mut store)?;
            }
        }
        Ok(frontend)
    }

    /// Apply an already durable event. Blocking SCC entry operations ensure a
    /// concurrent reader cannot cause a successful mutation to be skipped.
    fn apply_stored_event(&self, event: StoredAssetEvent) {
        match event {
            StoredAssetEvent::Asset(asset) => {
                let key = format!("{}:{}", self.host_id, asset.path);
                let entry = crate::engine::VersionedEntry {
                    version: 1,
                    last_updated: Utc::now().timestamp().max(0) as u64,
                    data: *asset,
                };
                match self.engine.internal_map().entry_sync(key) {
                    scc::hash_map::Entry::Occupied(mut occupied) => {
                        let version = occupied.get().version + 1;
                        *occupied.get_mut() = crate::engine::VersionedEntry { version, ..entry };
                    }
                    scc::hash_map::Entry::Vacant(vacant) => {
                        vacant.insert_entry(entry);
                    }
                }
            }
            StoredAssetEvent::Mutation(AssetMutation::AssetDeleted { path }) => {
                self.engine.internal_map().remove_sync(&format!("{}:{}", self.host_id, path));
            }
        }
    }

    /// Caller holds `mutations`, so comparison and log publication see the same
    /// ordered state, including concurrent reloads and programmatic updates.
    fn persist_asset_event(&self, event: StoredAssetEvent) -> Result<()> {
        let unchanged = match &event {
            StoredAssetEvent::Asset(asset) => self
                .engine
                .read(&format!("{}:{}", self.host_id, asset.path), |old| {
                    old.content == asset.content
                        && old.mime_type == asset.mime_type
                        && old.version == asset.version
                        && old.size_bytes == asset.size_bytes
                        && old.compression_enabled == asset.compression_enabled
                        && old.cache_ttl_seconds == asset.cache_ttl_seconds
                        && old.deployment_source == asset.deployment_source
                        && old.metadata == asset.metadata
                })
                .unwrap_or(false),
            StoredAssetEvent::Mutation(AssetMutation::AssetDeleted { path }) => {
                self.engine.read(&format!("{}:{path}", self.host_id), |_| ()).is_none()
            }
        };
        // New scan IDs/timestamps are not content changes. Keep the existing
        // metadata and avoid serialization, fsync and memory replacement.
        if unchanged {
            return Ok(());
        }
        let json = serde_json::to_string(&event)?;
        let store = self.engine.event_store();
        let mut store =
            store.write().map_err(|_| anyhow::anyhow!("frontend event store poisoned"))?;
        store.append_raw_line(&json)?;
        store.force_flush()?;
        self.apply_stored_event(event);
        Ok(())
    }

    fn checkpoint(&self, store: &mut EventStore) -> Result<()> {
        let snapshot = FrontendSnapshot {
            version: 1,
            host_id: self.host_id.clone(),
            assets: self.list_assets(),
        };
        store.force_flush()?;
        store.save_snapshot(&serde_json::to_string(&snapshot)?)?;
        store.truncate_events()?;
        Ok(())
    }

    fn compact_if_needed(&self) -> Result<()> {
        let store = self.engine.event_store();
        let mut store =
            store.write().map_err(|_| anyhow::anyhow!("frontend event store poisoned"))?;
        // Bound both tiny-file churn and repeated updates of one large bundle.
        // JSON byte arrays need up to four bytes per content byte. A single
        // mutation may cross the budget; checkpoint before acknowledging it.
        if store.event_count() > self.asset_count().saturating_mul(2).max(1)
            || store.journal_size()? > self.total_bytes().saturating_mul(8).max(1024 * 1024)
        {
            self.checkpoint(&mut store)?;
        }
        Ok(())
    }

    /// Synchronize a directory with the live asset set. Only new/changed files
    /// and removal tombstones are appended. Unchanged files retain their IDs
    /// and timestamps. Missing files are removed, including across restarts.
    pub async fn load_directory(&self, directory: impl AsRef<Path>) -> Result<usize> {
        let _mutation = self.mutations.lock().await;
        Ok(self.synchronize_directory(directory.as_ref()).await?.asset_count)
    }

    async fn synchronize_directory(&self, directory: &Path) -> Result<ReloadOutcome> {
        let version_before = self.version();
        let scan_path = directory.to_owned();
        let fresh = tokio::task::spawn_blocking(move || Self::scan_directory(&scan_path))
            .await
            .map_err(|e| anyhow::anyhow!("frontend scan task failed: {e}"))??;
        let new_keys: std::collections::HashSet<_> = fresh.iter().map(|(p, _)| p.clone()).collect();
        let existing = self.list_assets();
        let result = (|| -> Result<()> {
            for (path, content) in fresh {
                self.persist_asset_event(StoredAssetEvent::Asset(Box::new(StaticAsset::new(
                    path, content,
                ))))?;
            }
            for old in existing {
                if !new_keys.contains(&old.path) {
                    self.persist_asset_event(StoredAssetEvent::Mutation(
                        AssetMutation::AssetDeleted { path: old.path },
                    ))?;
                }
            }
            Ok(())
        })();
        // An I/O failure can leave a successfully persisted prefix. Cached
        // statistics must still describe the assets actually published.
        self.refresh_version_cache();
        result?;
        self.compact_if_needed()?;
        if let Ok(mut dir) = self.source_dir.write() {
            *dir = directory.to_string_lossy().into_owned();
        }
        if let Ok(mut ts) = self.last_reload_at.write() {
            *ts = Some(Utc::now());
        }
        let version = self.version();
        Ok(ReloadOutcome {
            asset_count: self.asset_count(),
            total_bytes: self.total_bytes(),
            changed: version != version_before,
            version,
        })
    }

    /// Recompute and store the cached `version` and `total_bytes` from the
    /// currently-loaded asset set. Called after mutations/replay — never
    /// on a read path. Hashing/summing happens here once, not per request.
    fn refresh_version_cache(&self) {
        let assets = self.list_assets();
        let total: u64 = assets.iter().map(|a| a.size_bytes).sum();
        let version = Self::compute_version(&assets);
        self.total_bytes.store(total, std::sync::atomic::Ordering::SeqCst);
        if let Ok(mut v) = self.version.write() {
            *v = version;
        }
    }

    /// Recursively read a directory into `(web_path, content)` pairs.
    ///
    /// Symlinks are skipped to avoid traversal cycles, mirroring
    /// [`crate::frontend::load_static_directory_to_memory`].
    fn scan_directory(dir_path: &Path) -> Result<Vec<(String, Vec<u8>)>> {
        fn walk_dir(
            dir: &Path,
            base_path_disk: &Path,
            assets: &mut Vec<(String, Vec<u8>)>,
        ) -> Result<()> {
            for entry in std::fs::read_dir(dir)? {
                let entry = entry?;
                let path = entry.path();

                // Skip symlinks to prevent infinite recursion from cycles.
                if path.is_symlink() {
                    continue;
                }

                if path.is_dir() {
                    walk_dir(&path, base_path_disk, assets)?;
                } else if path.is_file() {
                    let relative_path = path.strip_prefix(base_path_disk)?;
                    let web_path =
                        format!("/{}", relative_path.to_string_lossy().replace('\\', "/"));
                    let content = std::fs::read(&path)?;
                    assets.push((web_path, content));
                }
            }
            Ok(())
        }

        let mut assets_vec = Vec::new();
        walk_dir(dir_path, dir_path, &mut assets_vec)?;
        Ok(assets_vec)
    }

    /// Re-read the source directory off the request path and persist only its
    /// changes. Scanning finishes before any mutation. Each existing path is
    /// replaced in place, so readers never see a temporary 404 for a retained
    /// file. This is per-asset publication, not a transaction over the full set.
    /// Concurrent frontend writers/reloads are serialized; SCC reads remain free.
    pub async fn reload(&self) -> Result<ReloadOutcome> {
        let _mutation = self.mutations.lock().await;
        let dir = self.source_dir();
        anyhow::ensure!(
            !dir.is_empty(),
            "frontend '{}' has no recorded source directory to reload",
            self.host_id
        );
        self.synchronize_directory(Path::new(&dir)).await
    }

    /// Snapshot the assets currently held by this engine.
    ///
    /// Returns one entry per asset with the SCC2-key prefix (`{host_id}:`)
    /// stripped, so paths read as web paths (`/index.html`). The result is a
    /// point-in-time copy; concurrent reloads do not block it.
    pub fn list_assets(&self) -> Vec<StaticAsset> {
        let prefix = format!("{}:", self.host_id);
        self.engine
            .iter_all_sync()
            .into_iter()
            .map(|(_key, asset)| asset)
            .map(|mut asset| {
                // `path` on the asset already stores the web path; the SCC2
                // key carries the host prefix, not the asset's `path` field.
                // Defensive strip in case a key ever leaks into `path`.
                if let Some(stripped) = asset.path.strip_prefix(&prefix) {
                    asset.path = stripped.to_string();
                }
                asset
            })
            .collect()
    }

    /// Number of assets currently held in memory.
    pub fn asset_count(&self) -> usize {
        self.engine.total_count()
    }

    /// Sum of all asset content sizes currently held in memory.
    ///
    /// O(1) read of the cache populated at mutation/replay — does not iterate or
    /// clone assets (PR #138 review).
    pub fn total_bytes(&self) -> u64 {
        self.total_bytes.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Filesystem directory this engine was last loaded from.
    ///
    /// Empty until the first successful `load_directory`.
    pub fn source_dir(&self) -> String {
        self.source_dir.read().map(|g| g.clone()).unwrap_or_default()
    }

    /// Timestamp (RFC3339) of the most recent load/reload, if any.
    pub fn last_reload_at(&self) -> Option<DateTime<Utc>> {
        self.last_reload_at.read().ok().and_then(|g| *g)
    }

    /// Stable, comparable content fingerprint of the loaded asset set.
    ///
    /// Computed as the SHA-256 of the sorted `(web_path, size_bytes,
    /// sha256(content))` tuples of every asset. The sort makes it independent
    /// of filesystem iteration order; the per-asset content hash makes it
    /// sensitive to any byte change even when a file's size is unchanged.
    ///
    /// The same source directory loaded twice yields the same version; any
    /// added, removed, or modified file changes it. The string is prefixed
    /// `sha256:` and rendered as lowercase hex.
    ///
    /// O(1) read of the cache populated at mutation/replay — does not re-hash
    /// asset content per call (PR #138 review). Empty string before the
    /// first mutation or replay.
    pub fn version(&self) -> String {
        self.version.read().map(|g| g.clone()).unwrap_or_default()
    }

    /// Public wrapper over the private `compute_version` for
    /// callers (e.g. the admin API) that already hold an asset snapshot and
    /// want to avoid a second `list_assets` scan.
    pub fn compute_version_pub(assets: &[StaticAsset]) -> String {
        Self::compute_version(assets)
    }

    /// Compute the fingerprint for a given asset snapshot. Shared by
    /// [`version`](Self::version) and [`reload`](Self::reload) so both agree.
    fn compute_version(assets: &[StaticAsset]) -> String {
        use sha2::{Digest, Sha256};

        // (web_path, size, content-hash) per asset, sorted by path for a
        // deterministic, order-independent fingerprint.
        let mut entries: Vec<(String, u64, [u8; 32])> = assets
            .iter()
            .map(|a| {
                let mut h = Sha256::new();
                h.update(&a.content);
                let digest: [u8; 32] = h.finalize().into();
                (a.path.clone(), a.size_bytes, digest)
            })
            .collect();
        entries.sort_by(|a, b| a.0.cmp(&b.0));

        let mut hasher = Sha256::new();
        for (path, size, content_hash) in &entries {
            hasher.update(path.as_bytes());
            hasher.update(b"\0");
            hasher.update(size.to_le_bytes());
            hasher.update(b"\0");
            hasher.update(content_hash);
            hasher.update(b"\n");
        }
        format!("sha256:{:x}", hasher.finalize())
    }

    /// Get asset by path (lock-free read)
    ///
    /// # Arguments
    /// * `path` - Web path (e.g., "/index.html", "/css/styles.css")
    ///
    /// # Returns
    /// Asset if found, None otherwise
    pub async fn get_asset(&self, path: &str) -> Option<StaticAsset> {
        let key = format!("{}:{}", self.host_id, path);
        self.engine.read(&key, |asset| asset.clone())
    }

    /// Update asset content (emits AssetUpdated event)
    ///
    /// # Arguments
    /// * `path` - Web path
    /// * `content` - New content
    ///
    /// # Returns
    /// Result
    pub async fn update_asset(&self, path: &str, content: Vec<u8>) -> Result<()> {
        self.write_asset(path, content, None).await
    }

    /// Update asset content with an explicit MIME type (emits AssetUpdated event)
    ///
    /// `update_asset` derives the MIME type from the path extension, which
    /// yields `application/octet-stream` for extensionless clean URLs like
    /// `/posts/hello` — browsers download instead of rendering (issue #193).
    /// A caller pushing rendered content knows its real type; this variant
    /// records it on the asset so `FrontendServer` serves the right
    /// Content-Type.
    ///
    /// # Arguments
    /// * `path` - Web path
    /// * `content` - New content
    /// * `mime_type` - MIME type to serve the asset with (e.g., "text/html")
    ///
    /// # Returns
    /// Result
    pub async fn update_asset_with_mime(
        &self,
        path: &str,
        content: Vec<u8>,
        mime_type: &str,
    ) -> Result<()> {
        self.write_asset(path, content, Some(mime_type)).await
    }

    async fn write_asset(
        &self,
        path: &str,
        content: Vec<u8>,
        mime_type: Option<&str>,
    ) -> Result<()> {
        let _mutation = self.mutations.lock().await;
        let key = format!("{}:{}", self.host_id, path);

        // Get existing asset or create new one
        let mut asset = self
            .engine
            .read(&key, |a| a.clone())
            .unwrap_or_else(|| StaticAsset::new(path.to_string(), content.clone()));

        // Update content
        asset.content = content;
        asset.size_bytes = asset.content.len() as u64;
        asset.updated_at = Some(chrono::Utc::now());
        if let Some(mime) = mime_type {
            asset.set_mime_type(mime);
        }

        // Write back (emits event)
        self.persist_asset_event(StoredAssetEvent::Asset(Box::new(asset)))?;
        self.refresh_version_cache();
        self.compact_if_needed()
    }

    /// Durably delete an asset. Deleting a missing path is idempotent.
    ///
    /// A tombstone is flushed before removing the in-memory asset, so replay
    /// cannot resurrect it. A later update can recreate the same path.
    ///
    /// # Arguments
    /// * `path` - Web path
    pub async fn delete_asset(&self, path: &str) -> Result<()> {
        let _mutation = self.mutations.lock().await;
        self.persist_asset_event(StoredAssetEvent::Mutation(AssetMutation::AssetDeleted {
            path: path.to_string(),
        }))?;
        self.refresh_version_cache();
        self.compact_if_needed()
    }

    /// Get engine reference for advanced operations
    pub fn engine(&self) -> Arc<Scc2Engine<StaticAsset>> {
        self.engine.clone()
    }

    /// Get host ID
    pub fn host_id(&self) -> &str {
        &self.host_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset(path: &str, content: &[u8]) -> StaticAsset {
        StaticAsset::new(path.to_string(), content.to_vec())
    }

    #[test]
    fn version_is_stable_for_same_assets() {
        let set_a = vec![asset("/index.html", b"<h1>hi</h1>"), asset("/css/app.css", b"body{}")];
        // Same content, different filesystem iteration order.
        let set_b = vec![asset("/css/app.css", b"body{}"), asset("/index.html", b"<h1>hi</h1>")];

        let v_a = FrontendEngine::compute_version(&set_a);
        let v_b = FrontendEngine::compute_version(&set_b);
        assert_eq!(v_a, v_b, "version must be independent of asset ordering");
        assert!(v_a.starts_with("sha256:"));
    }

    #[test]
    fn version_changes_when_content_changes() {
        let before = vec![asset("/index.html", b"<h1>v1</h1>")];
        let after = vec![asset("/index.html", b"<h1>v2</h1>")];
        assert_ne!(
            FrontendEngine::compute_version(&before),
            FrontendEngine::compute_version(&after),
            "a content change must change the fingerprint"
        );
    }

    #[test]
    fn version_changes_when_asset_added_or_removed() {
        let one = vec![asset("/index.html", b"x")];
        let two = vec![asset("/index.html", b"x"), asset("/extra.txt", b"y")];
        assert_ne!(
            FrontendEngine::compute_version(&one),
            FrontendEngine::compute_version(&two),
            "adding an asset must change the fingerprint"
        );
    }

    #[test]
    fn version_distinguishes_same_bytes_different_paths() {
        // Identical content under different paths must not collide.
        let a = vec![asset("/a.txt", b"same")];
        let b = vec![asset("/b.txt", b"same")];
        assert_ne!(FrontendEngine::compute_version(&a), FrontendEngine::compute_version(&b));
    }
}
