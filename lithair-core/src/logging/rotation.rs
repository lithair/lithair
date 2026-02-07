//! File rotation strategies for Lithair logging
//!
//! Implements size-based and time-based log rotation with configurable retention policies.
//! Follows Lithair's explicit, not verbose philosophy.

use chrono::{DateTime, Datelike, Utc};
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// File rotation strategy
#[derive(Clone, Debug)]
pub enum FileRotation {
    /// Rotate when file reaches specified size in bytes
    Size(u64),
    /// Rotate daily at midnight UTC
    Daily,
    /// Rotate hourly
    Hourly,
    /// Rotate weekly on Sunday at midnight UTC
    Weekly,
    /// No rotation - single file grows indefinitely
    None,
}

/// Thread-safe rotating file writer
pub struct RotatingWriter {
    config: RotationConfig,
    current_writer: Arc<Mutex<Option<BufWriter<File>>>>,
    current_path: Arc<Mutex<PathBuf>>,
    current_size: Arc<Mutex<u64>>,
    last_rotation: Arc<Mutex<DateTime<Utc>>>,
}

#[derive(Clone, Debug)]
struct RotationConfig {
    base_path: String,
    rotation: FileRotation,
    max_files: Option<u32>,
}

impl RotatingWriter {
    /// Create a new rotating writer
    pub fn new(path: &str, rotation: FileRotation, max_files: Option<u32>) -> anyhow::Result<Self> {
        let config = RotationConfig { base_path: path.to_string(), rotation, max_files };

        let current_path = PathBuf::from(path);

        // Ensure parent directory exists
        if let Some(parent) = current_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let writer = Self {
            config,
            current_writer: Arc::new(Mutex::new(None)),
            current_path: Arc::new(Mutex::new(current_path)),
            current_size: Arc::new(Mutex::new(0)),
            last_rotation: Arc::new(Mutex::new(Utc::now())),
        };

        writer.ensure_writer()?;
        Ok(writer)
    }

    /// Write log data to the file
    pub fn write(&self, data: &[u8]) -> anyhow::Result<()> {
        self.check_rotation(data.len())?;

        let mut writer_guard = self.current_writer.lock().expect("current writer lock poisoned");
        if let Some(ref mut writer) = writer_guard.as_mut() {
            writer.write_all(data)?;
            writer.write_all(b"\n")?;

            // Update current size
            let mut size_guard = self.current_size.lock().expect("current size lock poisoned");
            *size_guard += data.len() as u64 + 1; // +1 for newline
        }

        Ok(())
    }

    /// Flush buffered data
    pub fn flush(&self) -> anyhow::Result<()> {
        let mut writer_guard = self.current_writer.lock().expect("current writer lock poisoned");
        if let Some(ref mut writer) = writer_guard.as_mut() {
            writer.flush()?;
        }
        Ok(())
    }

    /// Check if rotation is needed and perform it
    fn check_rotation(&self, incoming_size: usize) -> anyhow::Result<()> {
        let should_rotate = match &self.config.rotation {
            FileRotation::Size(max_size) => {
                let current_size = *self.current_size.lock().expect("current size lock poisoned");
                current_size + incoming_size as u64 > *max_size
            }
            FileRotation::Daily => {
                let last_rotation = *self.last_rotation.lock().expect("last rotation lock poisoned");
                let now = Utc::now();
                now.date_naive() > last_rotation.date_naive()
            }
            FileRotation::Hourly => {
                let last_rotation = *self.last_rotation.lock().expect("last rotation lock poisoned");
                let now = Utc::now();
                now.format("%Y%m%d%H").to_string() != last_rotation.format("%Y%m%d%H").to_string()
            }
            FileRotation::Weekly => {
                let last_rotation = *self.last_rotation.lock().expect("last rotation lock poisoned");
                let now = Utc::now();
                now.iso_week() != last_rotation.iso_week()
            }
            FileRotation::None => false,
        };

        if should_rotate {
            self.rotate()?;
        }

        Ok(())
    }

    /// Perform the actual rotation
    fn rotate(&self) -> anyhow::Result<()> {
        // Close current writer
        {
            let mut writer_guard = self.current_writer.lock().expect("current writer lock poisoned");
            if let Some(mut writer) = writer_guard.take() {
                writer.flush()?;
            }
        }

        // Generate rotated filename
        let rotated_path = self.generate_rotated_filename()?;
        let current_path = self.current_path.lock().expect("current path lock poisoned").clone();

        // Move current file to rotated name
        if current_path.exists() {
            std::fs::rename(&current_path, &rotated_path)?;
        }

        // Clean up old files if max_files is set
        if let Some(max_files) = self.config.max_files {
            self.cleanup_old_files(max_files)?;
        }

        // Reset state and create new writer
        *self.current_size.lock().expect("current size lock poisoned") = 0;
        *self.last_rotation.lock().expect("last rotation lock poisoned") = Utc::now();

        self.ensure_writer()?;

        Ok(())
    }

    /// Generate filename for rotated file
    fn generate_rotated_filename(&self) -> anyhow::Result<PathBuf> {
        let base_path = Path::new(&self.config.base_path);
        let stem = base_path.file_stem().and_then(|s| s.to_str()).unwrap_or("app");
        let extension = base_path.extension().and_then(|s| s.to_str()).unwrap_or("log");

        let parent = base_path.parent().unwrap_or(Path::new("."));

        let timestamp = match &self.config.rotation {
            FileRotation::Size(_) => Utc::now().format("%Y%m%d_%H%M%S").to_string(),
            FileRotation::Daily => Utc::now().format("%Y%m%d").to_string(),
            FileRotation::Hourly => Utc::now().format("%Y%m%d_%H").to_string(),
            FileRotation::Weekly => {
                let now = Utc::now();
                format!("{}_week{:02}", now.format("%Y"), now.iso_week().week())
            }
            FileRotation::None => unreachable!(),
        };

        let rotated_name = format!("{}_{}.{}", stem, timestamp, extension);
        Ok(parent.join(rotated_name))
    }

    /// Clean up old rotated files beyond max_files limit
    fn cleanup_old_files(&self, max_files: u32) -> anyhow::Result<()> {
        let base_path = Path::new(&self.config.base_path);
        let parent = base_path.parent().unwrap_or(Path::new("."));
        let stem = base_path.file_stem().and_then(|s| s.to_str()).unwrap_or("app");
        let extension = base_path.extension().and_then(|s| s.to_str()).unwrap_or("log");

        // Find all rotated files
        let mut rotated_files = Vec::new();

        if let Ok(entries) = std::fs::read_dir(parent) {
            for entry in entries.flatten() {
                let path = entry.path();
                if let Some(filename) = path.file_name().and_then(|n| n.to_str()) {
                    if filename.starts_with(&format!("{}_", stem))
                        && filename.ends_with(&format!(".{}", extension))
                    {
                        if let Ok(metadata) = entry.metadata() {
                            if let Ok(created) = metadata.created() {
                                rotated_files.push((path, created));
                            }
                        }
                    }
                }
            }
        }

        // Sort by creation time (oldest first)
        rotated_files.sort_by_key(|(_, created)| *created);

        // Remove files beyond the limit
        if rotated_files.len() > max_files as usize {
            let files_to_remove = rotated_files.len() - max_files as usize;
            for (path, _) in rotated_files.iter().take(files_to_remove) {
                let _ = std::fs::remove_file(path); // Ignore errors for cleanup
            }
        }

        Ok(())
    }

    /// Ensure a writer is available
    fn ensure_writer(&self) -> anyhow::Result<()> {
        let mut writer_guard = self.current_writer.lock().expect("current writer lock poisoned");
        if writer_guard.is_none() {
            let current_path = self.current_path.lock().expect("current path lock poisoned").clone();
            let file = OpenOptions::new().create(true).append(true).open(&current_path)?;

            // Update current size based on existing file
            if let Ok(metadata) = file.metadata() {
                *self.current_size.lock().expect("current size lock poisoned") = metadata.len();
            }

            *writer_guard = Some(BufWriter::new(file));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_size_rotation() {
        let temp_dir = TempDir::new().unwrap();
        let log_path = temp_dir.path().join("test.log");

        let writer = RotatingWriter::new(
            log_path.to_str().unwrap(),
            FileRotation::Size(100), // 100 bytes
            Some(3),
        )
        .unwrap();

        // Write data that exceeds size limit
        for i in 0..5 {
            let data = format!("This is log entry {} with enough text to trigger rotation", i);
            writer.write(data.as_bytes()).unwrap();
        }

        writer.flush().unwrap();

        // Check that rotation occurred
        let entries: Vec<_> = std::fs::read_dir(temp_dir.path()).unwrap().collect();

        assert!(entries.len() > 1, "Should have multiple files after rotation");
    }

    #[test]
    fn test_no_rotation() {
        let temp_dir = TempDir::new().unwrap();
        let log_path = temp_dir.path().join("test.log");

        let writer =
            RotatingWriter::new(log_path.to_str().unwrap(), FileRotation::None, None).unwrap();

        // Write multiple entries
        for i in 0..10 {
            let data = format!("Log entry {}", i);
            writer.write(data.as_bytes()).unwrap();
        }

        writer.flush().unwrap();

        // Should only have one file
        let entries: Vec<_> = std::fs::read_dir(temp_dir.path()).unwrap().collect();

        assert_eq!(entries.len(), 1, "Should have only one file with no rotation");
    }
}
