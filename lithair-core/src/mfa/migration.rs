//! Migration utility: JSON storage → Event-sourced storage
//!
//! This module helps migrate from the old JSON-based MFA storage
//! to the new event-sourced system.

use super::event_log::MfaEventLog;
use super::events::MfaEvent;
use super::storage::UserMfaData;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Legacy JSON storage format
#[derive(Debug, Serialize, Deserialize)]
struct LegacyMfaStorage {
    users: HashMap<String, UserMfaData>,
}

/// Migration statistics
#[derive(Debug, Default)]
pub struct MigrationStats {
    pub users_migrated: usize,
    pub events_generated: usize,
    pub errors: Vec<String>,
}

impl MigrationStats {
    pub fn success(&self) -> bool {
        self.errors.is_empty()
    }

    pub fn summary(&self) -> String {
        format!(
            "Migration complete: {} users → {} events (errors: {})",
            self.users_migrated,
            self.events_generated,
            self.errors.len()
        )
    }
}

/// Migrate from JSON files to event log (async version)
///
/// # Arguments
/// * `json_dir` - Directory containing old `*.json` MFA data files
/// * `event_log_path` - Path to the new event log file
/// * `dry_run` - If true, validate migration without writing events
///
/// # Returns
/// Migration statistics with details about the process
///
/// # Example
/// ```ignore
/// use lithair_core::mfa::migration::migrate_json_to_events;
///
/// let stats = migrate_json_to_events(
///     "./old_mfa_secrets",
///     "./mfa_events.log",
///     false  // Actually perform migration
/// ).await.unwrap();
///
/// println!("{}", stats.summary());
/// ```
pub async fn migrate_json_to_events(
    json_dir: impl AsRef<Path>,
    event_log_path: impl Into<PathBuf>,
    dry_run: bool,
) -> Result<MigrationStats> {
    let json_dir = json_dir.as_ref();
    let event_log_path = event_log_path.into();

    let mut stats = MigrationStats::default();

    // Find all JSON files in directory
    let json_files = find_json_files(json_dir)?;

    if json_files.is_empty() {
        log::warn!("No JSON files found in {:?}", json_dir);
        return Ok(stats);
    }

    log::info!("Found {} JSON files to migrate", json_files.len());

    // Create event log (or open existing)
    let event_log = if !dry_run {
        Some(MfaEventLog::new(&event_log_path)?)
    } else {
        log::info!("DRY RUN MODE - No events will be written");
        None
    };

    // Process each JSON file
    for json_file in json_files {
        match migrate_file(&json_file, event_log.as_ref(), &mut stats).await {
            Ok(_) => log::info!("Migrated: {:?}", json_file),
            Err(e) => {
                let error_msg = format!("Failed to migrate {:?}: {}", json_file, e);
                log::error!("{}", error_msg);
                stats.errors.push(error_msg);
            }
        }
    }

    log::info!("{}", stats.summary());

    Ok(stats)
}

/// Find all JSON files in directory
fn find_json_files(dir: &Path) -> Result<Vec<PathBuf>> {
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut files = Vec::new();

    for entry in
        fs::read_dir(dir).with_context(|| format!("Failed to read directory: {:?}", dir))?
    {
        let entry = entry?;
        let path = entry.path();

        if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("json") {
            files.push(path);
        }
    }

    Ok(files)
}

/// Migrate a single JSON file
async fn migrate_file(
    json_path: &Path,
    event_log: Option<&MfaEventLog>,
    stats: &mut MigrationStats,
) -> Result<()> {
    // Read JSON file
    let json_content = fs::read_to_string(json_path)
        .with_context(|| format!("Failed to read JSON file: {:?}", json_path))?;

    // Try parsing as multi-user format first {"users": {...}}
    if let Ok(legacy_storage) = serde_json::from_str::<LegacyMfaStorage>(&json_content) {
        // Multi-user format
        for (username, user_data) in legacy_storage.users {
            migrate_user(&username, &user_data, event_log, stats).await?;
        }
    } else {
        // Try parsing as single-user format (e.g., admin.json)
        match serde_json::from_str::<UserMfaData>(&json_content) {
            Ok(user_data) => {
                // Extract username from filename (e.g., "admin.json" → "admin")
                let username = json_path.file_stem().and_then(|s| s.to_str()).unwrap_or("unknown");

                migrate_user(username, &user_data, event_log, stats).await?;
            }
            Err(e) => {
                return Err(anyhow::anyhow!(
                    "Failed to parse JSON as multi-user or single-user format: {}",
                    e
                ));
            }
        }
    }

    Ok(())
}

/// Migrate a single user's data to events
async fn migrate_user(
    username: &str,
    user_data: &UserMfaData,
    event_log: Option<&MfaEventLog>,
    stats: &mut MigrationStats,
) -> Result<()> {
    let now = chrono::Utc::now();

    // Event 1: MfaSetupInitiated (secret was generated)
    let setup_event = MfaEvent::MfaSetupInitiated {
        username: username.to_string(),
        secret: user_data.secret.clone(),
        timestamp: user_data.status.enabled_at.unwrap_or(now),
    };

    if let Some(log) = event_log {
        log.append(setup_event).await?;
    }
    stats.events_generated += 1;

    // Event 2: MfaEnabled (if currently enabled)
    if user_data.status.enabled {
        let enable_event = MfaEvent::MfaEnabled {
            username: username.to_string(),
            timestamp: user_data.status.enabled_at.unwrap_or(now),
        };

        if let Some(log) = event_log {
            log.append(enable_event).await?;
        }
        stats.events_generated += 1;
    }

    // Event 3: BackupCodesGenerated (if backup codes exist)
    if !user_data.backup_codes.is_empty() {
        let backup_event = MfaEvent::BackupCodesGenerated {
            username: username.to_string(),
            codes_count: user_data.backup_codes.len(),
            timestamp: now,
        };

        if let Some(log) = event_log {
            log.append(backup_event).await?;
        }
        stats.events_generated += 1;
    }

    stats.users_migrated += 1;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mfa::{MfaStatus, TotpAlgorithm, TotpSecret};
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_migration_dry_run() {
        let temp_dir = TempDir::new().unwrap();
        let json_dir = temp_dir.path().join("old_mfa");
        fs::create_dir_all(&json_dir).unwrap();

        // Create legacy JSON file
        let secret = TotpSecret::generate(TotpAlgorithm::SHA256, 6, 30).unwrap();
        let mut users = HashMap::new();
        users.insert(
            "alice".to_string(),
            UserMfaData {
                secret,
                status: MfaStatus {
                    enabled: true,
                    required: false,
                    enabled_at: Some(chrono::Utc::now()),
                },
                backup_codes: vec!["ABC123".to_string()],
            },
        );

        let legacy_storage = LegacyMfaStorage { users };
        let json_path = json_dir.join("mfa_data.json");
        fs::write(&json_path, serde_json::to_string_pretty(&legacy_storage).unwrap()).unwrap();

        // Run migration in dry-run mode
        let stats = migrate_json_to_events(&json_dir, temp_dir.path().join("events.log"), true)
            .await
            .unwrap();

        assert_eq!(stats.users_migrated, 1);
        assert_eq!(stats.events_generated, 3); // Setup + Enabled + BackupCodes
        assert!(stats.success());
    }

    #[tokio::test]
    async fn test_migration_full() {
        let temp_dir = TempDir::new().unwrap();
        let json_dir = temp_dir.path().join("old_mfa");
        fs::create_dir_all(&json_dir).unwrap();

        // Create legacy JSON file
        let secret = TotpSecret::generate(TotpAlgorithm::SHA256, 6, 30).unwrap();
        let mut users = HashMap::new();
        users.insert(
            "bob".to_string(),
            UserMfaData {
                secret,
                status: MfaStatus {
                    enabled: true,
                    required: false,
                    enabled_at: Some(chrono::Utc::now()),
                },
                backup_codes: vec![],
            },
        );

        let legacy_storage = LegacyMfaStorage { users };
        let json_path = json_dir.join("mfa_data.json");
        fs::write(&json_path, serde_json::to_string_pretty(&legacy_storage).unwrap()).unwrap();

        // Run actual migration
        let event_log_path = temp_dir.path().join("events.log");
        let stats = migrate_json_to_events(&json_dir, &event_log_path, false).await.unwrap();

        assert_eq!(stats.users_migrated, 1);
        assert_eq!(stats.events_generated, 2); // Setup + Enabled
        assert!(stats.success());

        // Verify events were written
        let event_log = MfaEventLog::new(&event_log_path).unwrap();
        assert_eq!(event_log.event_count().await, 2);
        assert!(event_log.is_enabled("bob").await);
    }
}
