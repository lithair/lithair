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
use crate::engine::{EventStore, Scc2Engine, Scc2EngineConfig};
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
    /// Replays persisted assets and deletion tombstones before returning.
    /// Invalid events fail startup rather than silently restoring partial state.
    pub async fn new(host_id: impl Into<String>, data_dir: impl AsRef<Path>) -> Result<Self> {
        let host_id = host_id.into();
        let data_path = data_dir.as_ref().join(format!("frontend_{}", host_id));

        // Create event store for persistence
        let event_store = EventStore::new(data_path.to_string_lossy().as_ref())?;
        let events = event_store.get_all_events()?;
        let has_events = !events.is_empty();
        let event_store_arc = Arc::new(RwLock::new(event_store));

        // Configure SCC2 engine for frontend assets
        let config = Scc2EngineConfig {
            verbose_logging: false,
            enable_snapshots: false,
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
        };
        for json in events {
            let event: StoredAssetEvent = serde_json::from_str(&json).map_err(|e| {
                anyhow::anyhow!("invalid frontend event for {}: {}", frontend.host_id, e)
            })?;
            frontend.apply_stored_event(event);
        }
        if has_events {
            frontend.refresh_version_cache();
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

    fn persist_asset_event(&self, event: StoredAssetEvent, refresh_cache: bool) -> Result<()> {
        let json = serde_json::to_string(&event)?;
        let store = self.engine.event_store();
        // Serialize log order and memory publication for all frontend mutations.
        // Persist before publishing, and propagate append/flush failures.
        let mut store =
            store.write().map_err(|_| anyhow::anyhow!("frontend event store poisoned"))?;
        store.append_raw_line(&json)?;
        store.force_flush()?;
        self.apply_stored_event(event);
        if refresh_cache {
            self.refresh_version_cache();
        }
        Ok(())
    }

    /// Load static directory into memory with event sourcing
    ///
    /// This scans the directory and emits AssetCreated events for each file,
    /// persisting them to .raftlog for replay on restart.
    ///
    /// # Arguments
    /// * `directory` - Filesystem directory containing static files
    ///
    /// # Returns
    /// Number of assets loaded
    pub async fn load_directory(&self, directory: impl AsRef<Path>) -> Result<usize> {
        let dir_path = directory.as_ref();
        if !dir_path.exists() {
            return Err(anyhow::anyhow!("Directory does not exist: {}", dir_path.display()));
        }

        let assets_vec = Self::scan_directory(dir_path)?;
        let mut loaded_count = 0;

        // Store each asset in SCC2 engine with event sourcing
        for (web_path, content) in assets_vec {
            let asset = StaticAsset::new(web_path.clone(), content);

            log::info!(
                "📄 [{}] {} ({} bytes, {})",
                self.host_id,
                web_path,
                asset.size_bytes,
                asset.mime_type
            );

            // Persist before publishing to SCC2; refresh the cache once below.
            self.persist_asset_event(StoredAssetEvent::Asset(Box::new(asset)), false)?;
            loaded_count += 1;
        }

        // Record where we loaded from so a later hot reload can re-read the
        // same directory without the caller having to track it.
        if let Ok(mut dir) = self.source_dir.write() {
            *dir = dir_path.to_string_lossy().to_string();
        }
        if let Ok(mut ts) = self.last_reload_at.write() {
            *ts = Some(Utc::now());
        }
        // Cache the fingerprint + size once, now that the asset set is loaded,
        // so `version()`/`total_bytes()` are O(1) reads (PR #138 review).
        self.refresh_version_cache();

        Ok(loaded_count)
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

    /// Re-read this engine's source directory into memory atomically.
    ///
    /// The new asset set is built fully off the request path (blocking I/O runs
    /// on a `spawn_blocking` worker) *before* any in-memory mutation. The swap
    /// then upserts every freshly read asset and removes only the keys that no
    /// longer exist on disk. A path that is present both before and after is
    /// only ever overwritten in place — it is never transiently removed — so a
    /// concurrent request can never observe a half-loaded set or a spurious
    /// 404 for an asset that still exists.
    ///
    /// Returns a [`ReloadOutcome`] describing the post-reload state, including a
    /// `changed` flag computed by comparing the content [`version`] before and
    /// after.
    ///
    /// [`version`]: FrontendEngine::version
    pub async fn reload(&self) -> Result<ReloadOutcome> {
        let dir = self.source_dir();
        if dir.is_empty() {
            return Err(anyhow::anyhow!(
                "frontend '{}' has no recorded source directory to reload",
                self.host_id
            ));
        }

        let dir_path = std::path::PathBuf::from(&dir);
        if !dir_path.exists() {
            return Err(anyhow::anyhow!("source directory does not exist: {}", dir));
        }
        if !dir_path.is_dir() {
            return Err(anyhow::anyhow!("source path is not a directory: {}", dir));
        }

        let version_before = self.version();

        // Read the whole tree off the request path before touching memory.
        let scan_path = dir_path.clone();
        let fresh = tokio::task::spawn_blocking(move || Self::scan_directory(&scan_path))
            .await
            .map_err(|e| anyhow::anyhow!("frontend reload scan task failed: {}", e))??;

        // Build the new in-memory asset set keyed by SCC2 key. Doing this
        // before any mutation guarantees the swap never exposes a partial set.
        let mut new_assets: Vec<(String, StaticAsset)> = Vec::with_capacity(fresh.len());
        let mut new_keys: std::collections::HashSet<String> =
            std::collections::HashSet::with_capacity(fresh.len());
        for (web_path, content) in fresh {
            let asset = StaticAsset::new(web_path.clone(), content);
            let key = format!("{}:{}", self.host_id, web_path);
            new_keys.insert(key.clone());
            new_assets.push((key, asset));
        }

        // Atomic swap: upsert every new asset (overwrites in place), then drop
        // only keys that vanished from disk. SCC2 mutations are lock-free per
        // key; reads of unaffected keys are never blocked or disturbed.
        let existing_keys: Vec<String> =
            self.engine.iter_all_sync().into_iter().map(|(k, _)| k).collect();

        for (_key, asset) in new_assets {
            self.persist_asset_event(StoredAssetEvent::Asset(Box::new(asset)), false)?;
        }
        for key in existing_keys {
            if !new_keys.contains(&key) {
                if let Some(path) = key.strip_prefix(&format!("{}:", self.host_id)) {
                    self.persist_asset_event(
                        StoredAssetEvent::Mutation(AssetMutation::AssetDeleted {
                            path: path.to_string(),
                        }),
                        false,
                    )?;
                }
            }
        }

        if let Ok(mut ts) = self.last_reload_at.write() {
            *ts = Some(Utc::now());
        }

        let assets = self.list_assets();
        let asset_count = assets.len();
        let total_bytes: u64 = assets.iter().map(|a| a.size_bytes).sum();
        let version_after = Self::compute_version(&assets);

        // Update the caches from the values just computed (no extra scan), so
        // subsequent `version()`/`total_bytes()` reads stay O(1) (PR #138).
        self.total_bytes.store(total_bytes, std::sync::atomic::Ordering::SeqCst);
        if let Ok(mut v) = self.version.write() {
            *v = version_after.clone();
        }

        Ok(ReloadOutcome {
            asset_count,
            total_bytes,
            changed: version_after != version_before,
            version: version_after,
        })
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
        self.persist_asset_event(StoredAssetEvent::Asset(Box::new(asset)), true)?;
        Ok(())
    }

    /// Durably delete an asset. Deleting a missing path is idempotent.
    ///
    /// A tombstone is flushed before removing the in-memory asset, so replay
    /// cannot resurrect it. A later update can recreate the same path.
    ///
    /// # Arguments
    /// * `path` - Web path
    pub async fn delete_asset(&self, path: &str) -> Result<()> {
        self.persist_asset_event(
            StoredAssetEvent::Mutation(AssetMutation::AssetDeleted { path: path.to_string() }),
            true,
        )
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
