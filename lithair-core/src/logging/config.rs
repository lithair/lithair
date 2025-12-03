//! Logging configuration structures for declarative setup

use crate::logging::{LogFormat, LogOutput};
use std::collections::HashMap;

/// Main logging configuration for Lithair applications
///
/// This structure defines all logging behavior declaratively. Set it once and forget it.
#[derive(Clone, Debug)]
pub struct LoggingConfig {
    /// Minimum log level to capture
    pub level: LogLevel,
    /// Where logs should be sent
    pub outputs: Vec<LogOutput>,
    /// Default format for all outputs (can be overridden per output)
    pub format: LogFormat,
    /// Enable structured logging with consistent fields
    pub structured: bool,
    /// Include correlation/trace IDs in logs
    pub correlation_id: bool,
    /// Context fields added to every log entry
    pub context_fields: HashMap<String, String>,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: LogLevel::Info,
            outputs: vec![LogOutput::Stdout { format: None }],
            format: LogFormat::Human,
            structured: true,
            correlation_id: true,
            context_fields: HashMap::new(),
        }
    }
}

/// Log levels in order of severity (compatible with standard log crate)
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum LogLevel {
    /// Critical errors that may cause the application to abort
    Error,
    /// Warning conditions that should be investigated
    Warn,
    /// Informational messages about normal operation
    Info,
    /// Detailed information for debugging
    Debug,
    /// Very detailed information for deep debugging
    Trace,
}

impl From<log::Level> for LogLevel {
    fn from(level: log::Level) -> Self {
        match level {
            log::Level::Error => LogLevel::Error,
            log::Level::Warn => LogLevel::Warn,
            log::Level::Info => LogLevel::Info,
            log::Level::Debug => LogLevel::Debug,
            log::Level::Trace => LogLevel::Trace,
        }
    }
}

impl From<LogLevel> for log::Level {
    fn from(level: LogLevel) -> Self {
        match level {
            LogLevel::Error => log::Level::Error,
            LogLevel::Warn => log::Level::Warn,
            LogLevel::Info => log::Level::Info,
            LogLevel::Debug => log::Level::Debug,
            LogLevel::Trace => log::Level::Trace,
        }
    }
}

impl LoggingConfig {
    /// Create a production-ready logging configuration
    ///
    /// # Example
    ///
    /// ```rust
    /// use lithair_core::logging::{LoggingConfig, LogOutput, FileRotation};
    ///
    /// let config = LoggingConfig::production()
    ///     .with_file_output("./logs/app.log", FileRotation::Daily, Some(7))
    ///     .with_context_field("service", "my-service")
    ///     .with_context_field("version", env!("CARGO_PKG_VERSION"));
    /// ```
    pub fn production() -> Self {
        use crate::logging::LogFormat;

        Self {
            level: LogLevel::Info,
            outputs: vec![LogOutput::Stdout { format: Some(LogFormat::Json) }],
            format: LogFormat::Json,
            structured: true,
            correlation_id: true,
            context_fields: HashMap::new(),
        }
    }

    /// Create a development-friendly logging configuration
    pub fn development() -> Self {
        use crate::logging::LogFormat;

        Self {
            level: LogLevel::Debug,
            outputs: vec![LogOutput::Stdout { format: Some(LogFormat::Human) }],
            format: LogFormat::Human,
            structured: false,
            correlation_id: false,
            context_fields: HashMap::new(),
        }
    }

    /// Add a file output with rotation
    pub fn with_file_output(
        mut self,
        path: &str,
        rotation: crate::logging::FileRotation,
        max_files: Option<u32>,
    ) -> Self {
        self.outputs
            .push(LogOutput::File { path: path.to_string(), rotation, max_files });
        self
    }

    /// Add a context field that appears in every log entry
    pub fn with_context_field(mut self, key: &str, value: &str) -> Self {
        self.context_fields.insert(key.to_string(), value.to_string());
        self
    }

    /// Set the minimum log level
    pub fn with_level(mut self, level: LogLevel) -> Self {
        self.level = level;
        self
    }

    /// Add stdout output with specific format
    pub fn with_stdout(mut self, format: LogFormat) -> Self {
        self.outputs.push(LogOutput::Stdout { format: Some(format) });
        self
    }

    /// Add stderr output with specific format
    pub fn with_stderr(mut self, format: LogFormat) -> Self {
        self.outputs.push(LogOutput::Stderr { format: Some(format) });
        self
    }

    /// Enable or disable structured logging
    pub fn with_structured(mut self, structured: bool) -> Self {
        self.structured = structured;
        self
    }

    /// Enable or disable correlation IDs
    pub fn with_correlation_id(mut self, enabled: bool) -> Self {
        self.correlation_id = enabled;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logging::FileRotation;

    #[test]
    fn test_production_config() {
        let config = LoggingConfig::production();
        assert_eq!(config.level, LogLevel::Info);
        assert!(config.structured);
        assert!(config.correlation_id);
    }

    #[test]
    fn test_development_config() {
        let config = LoggingConfig::development();
        assert_eq!(config.level, LogLevel::Debug);
        assert!(!config.structured);
        assert!(!config.correlation_id);
    }

    #[test]
    fn test_builder_pattern() {
        let config = LoggingConfig::production()
            .with_file_output("./test.log", FileRotation::Daily, Some(7))
            .with_context_field("service", "test")
            .with_level(LogLevel::Debug);

        assert_eq!(config.level, LogLevel::Debug);
        assert_eq!(config.outputs.len(), 2); // stdout + file
        assert_eq!(config.context_fields.get("service"), Some(&"test".to_string()));
    }
}
