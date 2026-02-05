//! Admin Interface for Lithair Frontend Assets

use super::{assets::StaticAsset, FrontendState};
use crate::http::HttpResponse;
use anyhow::Result;
use serde_json::json;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

/// Admin handler for asset management
pub struct AssetAdminHandler {
    state: Arc<RwLock<FrontendState>>,
}

impl AssetAdminHandler {
    pub fn new(state: Arc<RwLock<FrontendState>>) -> Self {
        Self { state }
    }

    /// List all assets across all virtual hosts
    pub async fn list_assets(&self) -> Result<HttpResponse> {
        let state = self.state.read().await;

        // Aggregate assets from all virtual hosts
        let mut all_assets = Vec::new();
        for (host_id, vhost) in &state.virtual_hosts {
            for asset in vhost.assets.values() {
                all_assets.push(json!({
                    "asset": asset,
                    "virtual_host": host_id,
                    "base_path": &vhost.base_path
                }));
            }
        }

        let total_size: u64 = state
            .virtual_hosts
            .values()
            .flat_map(|vh| vh.assets.values())
            .map(|a| a.size_bytes)
            .sum();

        let json_value = json!({
            "assets": all_assets,
            "total_count": all_assets.len(),
            "total_size": total_size,
            "virtual_hosts": state.virtual_hosts.keys().collect::<Vec<_>>()
        });

        Ok(HttpResponse::ok().json(&json_value.to_string()))
    }

    /// Get specific asset across all virtual hosts
    pub async fn get_asset(&self, asset_id: Uuid) -> Result<HttpResponse> {
        let state = self.state.read().await;

        // Search in all virtual hosts
        for (host_id, vhost) in &state.virtual_hosts {
            if let Some(asset) = vhost.assets.get(&asset_id) {
                let json_value = json!({
                    "asset": asset,
                    "virtual_host": host_id,
                    "base_path": &vhost.base_path
                });
                return Ok(HttpResponse::ok().json(&json_value.to_string()));
            }
        }

        Ok(HttpResponse::not_found())
    }

    /// Upload new asset to a specific virtual host (defaults to "default")
    pub async fn upload_asset(
        &self,
        path: String,
        content: Vec<u8>,
        version: Option<String>,
    ) -> Result<HttpResponse> {
        self.upload_asset_to_host("default", path, content, version).await
    }

    /// Upload asset to a specific virtual host
    pub async fn upload_asset_to_host(
        &self,
        host_id: &str,
        path: String,
        content: Vec<u8>,
        version: Option<String>,
    ) -> Result<HttpResponse> {
        let asset = if let Some(v) = version {
            StaticAsset::new(path.clone(), content).with_version(v)
        } else {
            StaticAsset::new(path.clone(), content)
        };

        let mut state = self.state.write().await;

        // Get or create virtual host
        let vhost = state.virtual_hosts.entry(host_id.to_string()).or_insert_with(|| {
            super::VirtualHostLocation {
                host_id: host_id.to_string(),
                base_path: "/".to_string(),
                assets: std::collections::HashMap::new(),
                path_index: std::collections::HashMap::new(),
                static_root: String::new(),
                active: true,
            }
        });

        vhost.assets.insert(asset.id, asset.clone());
        vhost.path_index.insert(asset.path.clone(), asset.id);

        let json_value = json!({
            "success": true,
            "asset": asset,
            "virtual_host": host_id,
            "message": "Asset uploaded successfully"
        });

        Ok(HttpResponse::ok().json(&json_value.to_string()))
    }

    /// Delete asset from any virtual host
    pub async fn delete_asset(&self, asset_id: Uuid) -> Result<HttpResponse> {
        let mut state = self.state.write().await;

        // Search and delete in all virtual hosts
        for vhost in state.virtual_hosts.values_mut() {
            if let Some(asset) = vhost.assets.remove(&asset_id) {
                vhost.path_index.remove(&asset.path);

                let json_value = json!({
                    "success": true,
                    "message": "Asset deleted successfully"
                });

                return Ok(HttpResponse::ok().json(&json_value.to_string()));
            }
        }

        Ok(HttpResponse::not_found())
    }

    /// Get asset statistics across all virtual hosts
    pub async fn get_stats(&self) -> Result<HttpResponse> {
        let state = self.state.read().await;

        let total_assets: usize = state.virtual_hosts.values().map(|vh| vh.assets.len()).sum();

        let total_size: u64 = state
            .virtual_hosts
            .values()
            .flat_map(|vh| vh.assets.values())
            .map(|a| a.size_bytes)
            .sum();

        let mime_types: std::collections::HashMap<String, usize> = state
            .virtual_hosts
            .values()
            .flat_map(|vh| vh.assets.values())
            .fold(std::collections::HashMap::new(), |mut acc, asset| {
                *acc.entry(asset.mime_type.clone()).or_default() += 1;
                acc
            });

        let vhost_stats: Vec<_> = state
            .virtual_hosts
            .iter()
            .map(|(id, vh)| {
                json!({
                    "host_id": id,
                    "base_path": &vh.base_path,
                    "asset_count": vh.assets.len(),
                    "active": vh.active
                })
            })
            .collect();

        let json_value = json!({
            "total_assets": total_assets,
            "total_size_bytes": total_size,
            "mime_types": mime_types,
            "virtual_hosts": vhost_stats,
            "deployments": state.deployments.len(),
            "config": state.config
        });

        Ok(HttpResponse::ok().json(&json_value.to_string()))
    }
}

impl StaticAsset {
    pub fn with_version(mut self, version: String) -> Self {
        self.version = version;
        self
    }
}
