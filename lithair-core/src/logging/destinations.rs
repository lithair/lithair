//! Log output destinations - where logs are sent

use crate::logging::{FileRotation, LogFormat};

/// Where logs should be sent
#[derive(Clone, Debug)]
pub enum LogOutput {
    /// Write to stdout
    Stdout {
        /// Override the default format for this output
        format: Option<LogFormat>,
    },
    /// Write to stderr
    Stderr {
        /// Override the default format for this output
        format: Option<LogFormat>,
    },
    /// Write to a file with rotation
    File {
        /// File path (supports templates like "./logs/{date}/app.log")
        path: String,
        /// How to rotate the file
        rotation: FileRotation,
        /// Maximum number of rotated files to keep
        max_files: Option<u32>,
    },
    /// Future: Send to Loki
    #[allow(dead_code)]
    Loki { endpoint: String, labels: std::collections::HashMap<String, String> },
    /// Future: Send to syslog
    #[allow(dead_code)]
    Syslog { facility: String },
}

/// Trait for custom log destinations
pub trait LogDestination: Send + Sync {
    /// Write a log entry to this destination
    fn write_log(&self, entry: &LogEntry) -> anyhow::Result<()>;

    /// Flush any buffered logs
    fn flush(&self) -> anyhow::Result<()>;
}

/// A structured log entry
#[derive(Debug, Clone)]
pub struct LogEntry {
    /// Timestamp when the log was created
    pub timestamp: chrono::DateTime<chrono::Utc>,
    /// Log level
    pub level: crate::logging::LogLevel,
    /// The log message
    pub message: String,
    /// Target (usually module path)
    pub target: String,
    /// Correlation/trace ID if available
    pub correlation_id: Option<String>,
    /// Additional structured fields
    pub fields: std::collections::HashMap<String, serde_json::Value>,
    /// Source file and line number
    pub location: Option<LogLocation>,
}

/// Source code location for a log entry
#[derive(Debug, Clone)]
pub struct LogLocation {
    pub file: String,
    pub line: u32,
    pub module: String,
}

impl LogEntry {
    /// Create a new log entry
    pub fn new(level: crate::logging::LogLevel, message: String, target: String) -> Self {
        Self {
            timestamp: chrono::Utc::now(),
            level,
            message,
            target,
            correlation_id: None,
            fields: std::collections::HashMap::new(),
            location: None,
        }
    }

    /// Create a LogEntry from a standard log::Record
    pub fn from_log_record(record: &log::Record, config: &crate::logging::LoggingConfig) -> Self {
        // Convert log::Level to our LogLevel
        let level = match record.level() {
            log::Level::Error => crate::logging::LogLevel::Error,
            log::Level::Warn => crate::logging::LogLevel::Warn,
            log::Level::Info => crate::logging::LogLevel::Info,
            log::Level::Debug => crate::logging::LogLevel::Debug,
            log::Level::Trace => crate::logging::LogLevel::Trace,
        };

        let mut entry = Self {
            timestamp: chrono::Utc::now(),
            level,
            message: record.args().to_string(),
            target: record.target().to_string(),
            correlation_id: None,
            fields: std::collections::HashMap::new(),
            location: None,
        };

        // Add source location if available
        if let (Some(file), Some(line)) = (record.file(), record.line()) {
            entry.location = Some(LogLocation {
                file: file.to_string(),
                line,
                module: record.module_path().unwrap_or("unknown").to_string(),
            });
        }

        // Add context fields from config
        for (key, value) in &config.context_fields {
            entry.fields.insert(key.clone(), serde_json::Value::String(value.clone()));
        }

        // Add correlation ID if enabled (would need to be set from context in real implementation)
        if config.correlation_id {
            // For now, we'll leave this empty - in a real implementation,
            // this would come from thread-local storage or async context
        }

        entry
    }

    /// Add a structured field to the log entry
    pub fn with_field(mut self, key: &str, value: serde_json::Value) -> Self {
        self.fields.insert(key.to_string(), value);
        self
    }

    /// Add correlation ID
    pub fn with_correlation_id(mut self, id: String) -> Self {
        self.correlation_id = Some(id);
        self
    }

    /// Add source location
    pub fn with_location(mut self, file: &str, line: u32, module: &str) -> Self {
        self.location =
            Some(LogLocation { file: file.to_string(), line, module: module.to_string() });
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logging::LogLevel;

    #[test]
    fn test_log_entry_creation() {
        let entry =
            LogEntry::new(LogLevel::Info, "Test message".to_string(), "test::module".to_string());

        assert_eq!(entry.level, LogLevel::Info);
        assert_eq!(entry.message, "Test message");
        assert_eq!(entry.target, "test::module");
        assert!(entry.correlation_id.is_none());
    }

    #[test]
    fn test_log_entry_with_fields() {
        let entry = LogEntry::new(
            LogLevel::Error,
            "Error occurred".to_string(),
            "app::handler".to_string(),
        )
        .with_field("user_id", serde_json::Value::Number(serde_json::Number::from(123)))
        .with_correlation_id("req-456".to_string());

        assert_eq!(entry.correlation_id, Some("req-456".to_string()));
        assert_eq!(entry.fields.len(), 1);
        assert_eq!(
            entry.fields.get("user_id"),
            Some(&serde_json::Value::Number(serde_json::Number::from(123)))
        );
    }
}
