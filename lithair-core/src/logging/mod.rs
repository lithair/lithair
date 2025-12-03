//! Lithair Declarative Logging System
//!
//! This module provides a declarative, zero-configuration logging system built on the standard `log` crate.
//! It follows Lithair's philosophy: configure once declaratively, everything works automatically.
//!
//! # Philosophy
//!
//! - **Explicit, not verbose**: Logs should be clear and actionable, not chatty
//! - **Declarative configuration**: Specify what you want, not how to implement it
//! - **Production-ready defaults**: Structured JSON, file rotation, proper levels
//! - **Zero boilerplate**: No manual logger setup, everything configured via attributes
//! - **Standard compatibility**: Uses standard `log` crate (info!, debug!, error!, etc.)
//!
//! # Example
//!
//! ```rust,no_run
//! use lithair_core::logging::{LoggingConfig, LogOutput, LogFormat, FileRotation};
//!
//! // Configure the logger once
//! let config = LoggingConfig::production()
//!     .with_file_output("./logs/app.log", FileRotation::Size(10_000_000), Some(5))
//!     .with_context_field("service", "lithair");
//!
//! lithair_core::logging::init_logging(&config).unwrap();
//!
//! // Then use standard log macros anywhere in your code
//! log::info!("Server starting on port {}", 8080);
//! log::error!("Failed to connect to database: {}", err);
//! ```

pub mod config;
pub mod destinations;
pub mod formatter;
pub mod rotation;

// Re-export main types for convenience
pub use config::{LogLevel, LoggingConfig};
pub use destinations::{LogDestination, LogOutput};
pub use formatter::LogFormat;
pub use rotation::{FileRotation, RotatingWriter};

use std::sync::{Arc, Once};

static INIT: Once = Once::new();

/// Initialize the Lithair logging system
///
/// This should be called once at application startup. It's safe to call multiple times.
/// Uses the standard `log` crate as backend.
pub fn init_logging(config: &LoggingConfig) -> anyhow::Result<()> {
    INIT.call_once(|| {
        let _ = init_logging_internal(config);
    });
    Ok(())
}

/// Internal logging initialization - called only once
fn init_logging_internal(config: &LoggingConfig) -> anyhow::Result<()> {
    // Create our custom logger implementation
    let logger = RaftstoneLogger::new(config.clone())?;

    // Set as the global logger
    log::set_boxed_logger(Box::new(logger))?;

    // Set the maximum log level based on config
    let max_level = match config.level {
        config::LogLevel::Error => log::LevelFilter::Error,
        config::LogLevel::Warn => log::LevelFilter::Warn,
        config::LogLevel::Info => log::LevelFilter::Info,
        config::LogLevel::Debug => log::LevelFilter::Debug,
        config::LogLevel::Trace => log::LevelFilter::Trace,
    };
    log::set_max_level(max_level);

    Ok(())
}

/// Lithair's implementation of the log::Log trait
struct RaftstoneLogger {
    config: LoggingConfig,
    writers: Vec<Arc<dyn LogWriter>>,
}

impl RaftstoneLogger {
    fn new(config: LoggingConfig) -> anyhow::Result<Self> {
        let mut writers: Vec<Arc<dyn LogWriter>> = Vec::new();

        for output in &config.outputs {
            match output {
                LogOutput::Stdout { format } => {
                    let fmt = format.as_ref().unwrap_or(&config.format);
                    writers.push(Arc::new(StdoutWriter::new(fmt.clone())));
                }
                LogOutput::Stderr { format } => {
                    let fmt = format.as_ref().unwrap_or(&config.format);
                    writers.push(Arc::new(StderrWriter::new(fmt.clone())));
                }
                LogOutput::File { path, rotation, max_files } => {
                    let writer =
                        FileWriter::new(path, rotation.clone(), *max_files, config.format.clone())?;
                    writers.push(Arc::new(writer));
                }
                LogOutput::Loki { .. } | LogOutput::Syslog { .. } => {
                    // Future implementation
                }
            }
        }

        // Default to stdout if no outputs specified
        if writers.is_empty() {
            writers.push(Arc::new(StdoutWriter::new(config.format.clone())));
        }

        Ok(Self { config, writers })
    }
}

impl log::Log for RaftstoneLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        let config_level = match self.config.level {
            config::LogLevel::Error => log::Level::Error,
            config::LogLevel::Warn => log::Level::Warn,
            config::LogLevel::Info => log::Level::Info,
            config::LogLevel::Debug => log::Level::Debug,
            config::LogLevel::Trace => log::Level::Trace,
        };
        metadata.level() <= config_level
    }

    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }

        // Create a LogEntry from the standard log::Record
        let entry = destinations::LogEntry::from_log_record(record, &self.config);

        // Write to all configured outputs
        for writer in &self.writers {
            let _ = writer.write_log(&entry);
        }
    }

    fn flush(&self) {
        for writer in &self.writers {
            let _ = writer.flush();
        }
    }
}

/// Trait for writing logs to different destinations
trait LogWriter: Send + Sync {
    fn write_log(&self, entry: &destinations::LogEntry) -> anyhow::Result<()>;
    fn flush(&self) -> anyhow::Result<()>;
}

/// Stdout writer implementation
struct StdoutWriter {
    format: LogFormat,
}

impl StdoutWriter {
    fn new(format: LogFormat) -> Self {
        Self { format }
    }
}

impl LogWriter for StdoutWriter {
    fn write_log(&self, entry: &destinations::LogEntry) -> anyhow::Result<()> {
        let formatted = self.format.format_entry(entry);
        println!("{}", formatted);
        Ok(())
    }

    fn flush(&self) -> anyhow::Result<()> {
        use std::io::{self, Write};
        io::stdout().flush()?;
        Ok(())
    }
}

/// Stderr writer implementation
struct StderrWriter {
    format: LogFormat,
}

impl StderrWriter {
    fn new(format: LogFormat) -> Self {
        Self { format }
    }
}

impl LogWriter for StderrWriter {
    fn write_log(&self, entry: &destinations::LogEntry) -> anyhow::Result<()> {
        let formatted = self.format.format_entry(entry);
        eprintln!("{}", formatted);
        Ok(())
    }

    fn flush(&self) -> anyhow::Result<()> {
        use std::io::{self, Write};
        io::stderr().flush()?;
        Ok(())
    }
}

/// File writer with rotation support
struct FileWriter {
    rotating_writer: rotation::RotatingWriter,
    format: LogFormat,
}

impl FileWriter {
    fn new(
        path: &str,
        rotation: rotation::FileRotation,
        max_files: Option<u32>,
        format: LogFormat,
    ) -> anyhow::Result<Self> {
        let rotating_writer = rotation::RotatingWriter::new(path, rotation, max_files)?;
        Ok(Self { rotating_writer, format })
    }
}

impl LogWriter for FileWriter {
    fn write_log(&self, entry: &destinations::LogEntry) -> anyhow::Result<()> {
        let formatted = self.format.format_entry(entry);
        self.rotating_writer.write(formatted.as_bytes())?;
        Ok(())
    }

    fn flush(&self) -> anyhow::Result<()> {
        self.rotating_writer.flush()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_logging_config_creation() {
        let config = LoggingConfig {
            level: LogLevel::Info,
            outputs: vec![LogOutput::Stdout { format: None }],
            format: LogFormat::Json,
            structured: true,
            correlation_id: true,
            context_fields: HashMap::new(),
        };

        assert_eq!(config.level, LogLevel::Info);
        assert_eq!(config.outputs.len(), 1);
    }
}
