//! Frontend In-Memory Asset Serving for Lithair
//!
//! This module provides memory-first static asset serving capabilities
//! for serving frontend files with zero disk I/O after initial load.
//!
//! ## Current Features (v1)
//! - Load static files from filesystem into memory at startup
//! - Zero disk I/O after initial load - all assets served from memory
//! - Virtual host support for multi-site serving from single instance
//! - Automatic MIME type detection and optimal cache headers
//! - Thread-safe with Arc<RwLock> for concurrent access
//! - SCC2 lock-free performance optimization
//!
//! ## Planned Features (v2)
//! - ⏳ Event sourcing with .raftlog files for asset versioning
//! - ⏳ Hot deployment without server restart
//! - ⏳ Version history and time travel debugging
//! - ⏳ Declarative asset management with DeclarativeModel macro
//! - ⏳ Admin interface with automatic CRUD APIs
//!
//! ## Usage Example
//!
//! ```rust,no_run
//! use lithair_core::frontend::{FrontendState, load_static_directory_to_memory, AssetServer};
//! use std::sync::Arc;
//! use tokio::sync::RwLock;
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     // Initialize frontend state
//!     let frontend_state = Arc::new(RwLock::new(FrontendState::default()));
//!
//!     // Load static files from directory into memory
//!     let count = load_static_directory_to_memory(
//!         frontend_state.clone(),
//!         "main_site",        // Virtual host ID
//!         "/",                // Base path for HTTP routing
//!         "./public"          // Directory with static files
//!     ).await?;
//!
//!     println!("Loaded {} assets into memory", count);
//!
//!     // Create asset server for serving
//!     let asset_server = Arc::new(AssetServer::new(frontend_state));
//!
//!     // Use asset_server.serve_asset() in your HTTP handlers
//!     Ok(())
//! }
//! ```

pub mod admin;
pub mod assets;
pub mod config;
pub mod engine;
pub mod server;

pub use admin::AssetAdminHandler;
pub use assets::StaticAsset;
pub use config::FrontendConfig;
pub use engine::FrontendEngine;
pub use server::{AssetServer, FrontendServer};

// Utility functions are defined below

use crate::engine::Event;
use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use uuid::Uuid;

/// Frontend events for asset management with event sourcing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FrontendEvent {
    /// Asset was uploaded/created
    AssetCreated {
        id: Uuid,
        path: String,
        size_bytes: u64,
        mime_type: String,
        version: String,
        created_at: DateTime<Utc>,
    },
    /// Asset content was updated
    AssetUpdated {
        id: Uuid,
        old_version: String,
        new_version: String,
        size_bytes: u64,
        updated_at: DateTime<Utc>,
    },
    /// Asset was deleted
    AssetDeleted { id: Uuid, path: String, deleted_at: DateTime<Utc> },
    /// Asset was deployed to a specific version
    AssetDeployed {
        id: Uuid,
        version: String,
        deployment_source: String,
        deployed_at: DateTime<Utc>,
    },
}

/// Virtual host location configuration for multi-site support
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VirtualHostLocation {
    /// Virtual host identifier (e.g., "lithair", "blog", "docs")
    pub host_id: String,
    /// Base path for routing (e.g., "/", "/blog", "/docs")
    pub base_path: String,
    /// Assets specific to this virtual host
    pub assets: HashMap<Uuid, StaticAsset>,
    /// Path to asset ID mapping for this virtual host
    pub path_index: HashMap<String, Uuid>,
    /// Static root directory for this virtual host
    pub static_root: String,
    /// Active status
    pub active: bool,
}

/// Frontend state for managing multi-virtual-host assets in memory
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FrontendState {
    /// Virtual host locations mapped by host_id
    pub virtual_hosts: HashMap<String, VirtualHostLocation>,
    /// Global version history for each asset (across all virtual hosts)
    pub version_history: HashMap<Uuid, Vec<String>>,
    /// Global deployment metadata
    pub deployments: HashMap<String, DateTime<Utc>>,
    /// Global configuration
    pub config: FrontendConfig,
}

impl Event for FrontendEvent {
    type State = FrontendState;

    fn apply(&self, state: &mut Self::State) {
        match self {
            FrontendEvent::AssetCreated {
                id,
                path: _,
                size_bytes: _,
                mime_type: _,
                version,
                created_at: _,
            } => {
                // Note: With multi-virtual-host architecture, event sourcing needs rework
                // For now, just track version history globally
                state.version_history.entry(*id).or_default().push(version.clone());
            }
            FrontendEvent::AssetUpdated {
                id,
                old_version: _,
                new_version,
                size_bytes: _,
                updated_at: _,
            } => {
                state.version_history.entry(*id).or_default().push(new_version.clone());
            }
            FrontendEvent::AssetDeleted { id, path: _, deleted_at: _ } => {
                // Note: With multi-virtual-host architecture, we would need to know which vhost
                // For now, remove from version history
                state.version_history.remove(id);
            }
            FrontendEvent::AssetDeployed { id: _, version: _, deployment_source, deployed_at } => {
                state.deployments.insert(deployment_source.clone(), *deployed_at);
            }
        }
    }
}

/// Load static files from a directory into Lithair memory
///
/// This loads all files from the specified directory into memory as a virtual host,
/// enabling zero-disk-I/O serving after initial load. Files are served via AssetServer.
///
/// # Arguments
/// * `state` - Shared frontend state (thread-safe with Arc<RwLock>)
/// * `host_id` - Virtual host identifier (e.g., "main_site", "blog")
/// * `base_path` - HTTP base path for routing (e.g., "/", "/blog")
/// * `directory` - Filesystem directory containing static files
///
/// # Returns
/// Number of files loaded into memory
pub async fn load_static_directory_to_memory<P: AsRef<Path>>(
    state: std::sync::Arc<tokio::sync::RwLock<FrontendState>>,
    host_id: &str,
    base_path: &str,
    directory: P,
) -> Result<usize> {
    // Convert to owned PathBuf before moving into spawn_blocking
    let dir = directory.as_ref().to_path_buf();
    let dir_display = dir.to_string_lossy().to_string();

    // Perform blocking I/O outside the lock via spawn_blocking
    let assets_vec = tokio::task::spawn_blocking(move || -> Result<Vec<(String, Vec<u8>)>> {
        let dir_path = dir.as_path();
        if !dir_path.exists() {
            return Err(anyhow::anyhow!("Directory does not exist: {}", dir_path.display()));
        }
        fn walk_dir(
            dir: &Path,
            base_path_disk: &Path,
            assets: &mut Vec<(String, Vec<u8>)>,
        ) -> Result<()> {
            for entry in std::fs::read_dir(dir)? {
                let entry = entry?;
                let path = entry.path();
                // Skip symlinks to prevent infinite recursion from symlink cycles
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
        let mut assets = Vec::new();
        walk_dir(dir_path, dir_path, &mut assets)?;
        Ok(assets)
    })
    .await
    .map_err(|e| anyhow::anyhow!("spawn_blocking failed: {}", e))??;

    // Now acquire the write lock only for in-memory mutation
    let mut state_guard = state.write().await;
    let host_id_str = host_id.to_string();
    let base_path_str = base_path.to_string();

    // Clear stale assets if reloading an existing host
    if let Some(existing) = state_guard.virtual_hosts.get_mut(&host_id_str) {
        existing.assets.clear();
        existing.path_index.clear();
        existing.base_path = base_path_str.clone();
        existing.static_root = dir_display.clone();
    }

    let location = state_guard.virtual_hosts.entry(host_id_str.clone()).or_insert_with(|| {
        VirtualHostLocation {
            host_id: host_id_str,
            base_path: base_path_str,
            assets: HashMap::new(),
            path_index: HashMap::new(),
            static_root: dir_display,
            active: true,
        }
    });

    let mut loaded_count = 0;
    for (web_path, content) in assets_vec {
        let asset = StaticAsset::new(web_path.clone(), content);
        log::info!("[{}] {} ({} bytes, {})", host_id, web_path, asset.size_bytes, asset.mime_type);
        location.assets.insert(asset.id, asset.clone());
        location.path_index.insert(web_path, asset.id);
        loaded_count += 1;
    }

    Ok(loaded_count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_frontend_event_asset_created() {
        let mut state = FrontendState::default();
        let asset_id = Uuid::new_v4();

        let event = FrontendEvent::AssetCreated {
            id: asset_id,
            path: "/index.html".to_string(),
            size_bytes: 1024,
            mime_type: "text/html".to_string(),
            version: "v1.0.0".to_string(),
            created_at: Utc::now(),
        };

        event.apply(&mut state);

        // Note: Tests need to be updated for multi-virtual-host architecture
        assert_eq!(state.version_history.get(&asset_id).unwrap().len(), 1);
    }

    #[test]
    fn test_frontend_event_asset_updated() {
        let mut state = FrontendState::default();
        let asset_id = Uuid::new_v4();

        // Create asset first
        let create_event = FrontendEvent::AssetCreated {
            id: asset_id,
            path: "/style.css".to_string(),
            size_bytes: 512,
            mime_type: "text/css".to_string(),
            version: "v1.0.0".to_string(),
            created_at: Utc::now(),
        };
        create_event.apply(&mut state);

        // Update asset
        let update_event = FrontendEvent::AssetUpdated {
            id: asset_id,
            old_version: "v1.0.0".to_string(),
            new_version: "v1.1.0".to_string(),
            size_bytes: 768,
            updated_at: Utc::now(),
        };
        update_event.apply(&mut state);

        assert_eq!(state.version_history.get(&asset_id).unwrap().len(), 2);
        assert_eq!(state.version_history.get(&asset_id).unwrap()[1], "v1.1.0");
    }

    #[test]
    fn test_frontend_event_asset_deleted() {
        let mut state = FrontendState::default();
        let asset_id = Uuid::new_v4();

        // Create asset first
        let create_event = FrontendEvent::AssetCreated {
            id: asset_id,
            path: "/app.js".to_string(),
            size_bytes: 2048,
            mime_type: "application/javascript".to_string(),
            version: "v1.0.0".to_string(),
            created_at: Utc::now(),
        };
        create_event.apply(&mut state);

        // Delete asset
        let delete_event = FrontendEvent::AssetDeleted {
            id: asset_id,
            path: "/app.js".to_string(),
            deleted_at: Utc::now(),
        };
        delete_event.apply(&mut state);

        // Note: Tests need to be updated for multi-virtual-host architecture
        assert!(!state.version_history.contains_key(&asset_id));
    }
}
