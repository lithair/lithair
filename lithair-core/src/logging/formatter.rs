//! Log formatting options for different output styles

use crate::logging::destinations::LogEntry;

/// How logs should be formatted
#[derive(Clone, Debug, PartialEq)]
pub enum LogFormat {
    /// Structured JSON format (production default)
    /// Example: {"timestamp":"2024-01-15T10:30:00Z","level":"INFO","message":"User logged in","user_id":123}
    Json,

    /// Human-readable format (development default)
    /// Example: 2024-01-15 10:30:00 INFO [app::auth] User logged in user_id=123
    Human,

    /// Logfmt format (key=value pairs)
    /// Example: timestamp=2024-01-15T10:30:00Z level=INFO target=app::auth message="User logged in" user_id=123
    Logfmt,

    /// Custom format using template string
    /// Example: "{timestamp} [{level}] {message}" -> "2024-01-15 10:30:00 [INFO] User logged in"
    Custom(String),
}

impl LogFormat {
    /// Format a log entry according to this format
    pub fn format_entry(&self, entry: &LogEntry) -> String {
        match self {
            LogFormat::Json => format_json(entry),
            LogFormat::Human => format_human(entry),
            LogFormat::Logfmt => format_logfmt(entry),
            LogFormat::Custom(template) => format_custom(entry, template),
        }
    }
}

/// Format log entry as JSON
fn format_json(entry: &LogEntry) -> String {
    let mut json = serde_json::Map::new();

    // Core fields
    json.insert("timestamp".to_string(), serde_json::Value::String(entry.timestamp.to_rfc3339()));
    json.insert(
        "level".to_string(),
        serde_json::Value::String(format!("{:?}", entry.level).to_uppercase()),
    );
    json.insert("message".to_string(), serde_json::Value::String(entry.message.clone()));
    json.insert("target".to_string(), serde_json::Value::String(entry.target.clone()));

    // Optional fields
    if let Some(correlation_id) = &entry.correlation_id {
        json.insert(
            "correlation_id".to_string(),
            serde_json::Value::String(correlation_id.clone()),
        );
    }

    if let Some(location) = &entry.location {
        json.insert("file".to_string(), serde_json::Value::String(location.file.clone()));
        json.insert(
            "line".to_string(),
            serde_json::Value::Number(serde_json::Number::from(location.line)),
        );
    }

    // Custom fields
    for (key, value) in &entry.fields {
        json.insert(key.clone(), value.clone());
    }

    serde_json::to_string(&json).unwrap_or_else(|_| "Failed to serialize log entry".to_string())
}

/// Format log entry in human-readable format
fn format_human(entry: &LogEntry) -> String {
    let timestamp = entry.timestamp.format("%Y-%m-%d %H:%M:%S%.3f");
    let level = format!("{:5}", format!("{:?}", entry.level).to_uppercase());

    // Build the base message
    let mut message = format!("{} {} [{}] {}", timestamp, level, entry.target, entry.message);

    // Add correlation ID if present
    if let Some(correlation_id) = &entry.correlation_id {
        message.push_str(&format!(" correlation_id={}", correlation_id));
    }

    // Add custom fields
    for (key, value) in &entry.fields {
        let value_str = match value {
            serde_json::Value::String(s) => s.clone(),
            serde_json::Value::Number(n) => n.to_string(),
            serde_json::Value::Bool(b) => b.to_string(),
            _ => value.to_string(),
        };
        message.push_str(&format!(" {}={}", key, value_str));
    }

    // Add location if present
    if let Some(location) = &entry.location {
        message.push_str(&format!(" ({}:{})", location.file, location.line));
    }

    message
}

/// Format log entry in logfmt format
fn format_logfmt(entry: &LogEntry) -> String {
    let mut parts = Vec::new();

    // Core fields
    parts.push(format!("timestamp={}", entry.timestamp.to_rfc3339()));
    parts.push(format!("level={}", format!("{:?}", entry.level).to_uppercase()));
    parts.push(format!("target={}", entry.target));
    parts.push(format!("message=\"{}\"", entry.message.replace("\"", "\\\"")));

    // Optional fields
    if let Some(correlation_id) = &entry.correlation_id {
        parts.push(format!("correlation_id={}", correlation_id));
    }

    if let Some(location) = &entry.location {
        parts.push(format!("file=\"{}\"", location.file));
        parts.push(format!("line={}", location.line));
    }

    // Custom fields
    for (key, value) in &entry.fields {
        let value_str = match value {
            serde_json::Value::String(s) => format!("\"{}\"", s.replace("\"", "\\\"")),
            serde_json::Value::Number(n) => n.to_string(),
            serde_json::Value::Bool(b) => b.to_string(),
            _ => format!("\"{}\"", value.to_string().replace("\"", "\\\"")),
        };
        parts.push(format!("{}={}", key, value_str));
    }

    parts.join(" ")
}

/// Format log entry using custom template
fn format_custom(entry: &LogEntry, template: &str) -> String {
    let mut result = template.to_string();

    // Replace standard placeholders
    result = result.replace("{timestamp}", &entry.timestamp.to_rfc3339());
    result = result.replace("{level}", &format!("{:?}", entry.level).to_uppercase());
    result = result.replace("{message}", &entry.message);
    result = result.replace("{target}", &entry.target);

    if let Some(correlation_id) = &entry.correlation_id {
        result = result.replace("{correlation_id}", correlation_id);
    } else {
        result = result.replace("{correlation_id}", "");
    }

    if let Some(location) = &entry.location {
        result = result.replace("{file}", &location.file);
        result = result.replace("{line}", &location.line.to_string());
    } else {
        result = result.replace("{file}", "");
        result = result.replace("{line}", "");
    }

    // Replace custom field placeholders
    for (key, value) in &entry.fields {
        let placeholder = format!("{{{}}}", key);
        let value_str = match value {
            serde_json::Value::String(s) => s.clone(),
            serde_json::Value::Number(n) => n.to_string(),
            serde_json::Value::Bool(b) => b.to_string(),
            _ => value.to_string(),
        };
        result = result.replace(&placeholder, &value_str);
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logging::{destinations::LogEntry, LogLevel};

    #[test]
    fn test_json_format() {
        let entry =
            LogEntry::new(LogLevel::Info, "Test message".to_string(), "test::module".to_string());

        let formatted = LogFormat::Json.format_entry(&entry);

        // Should be valid JSON
        let parsed: serde_json::Value = serde_json::from_str(&formatted).unwrap();
        assert_eq!(parsed["message"], "Test message");
        assert_eq!(parsed["level"], "INFO");
        assert_eq!(parsed["target"], "test::module");
    }

    #[test]
    fn test_human_format() {
        let entry = LogEntry::new(
            LogLevel::Error,
            "Error occurred".to_string(),
            "app::handler".to_string(),
        )
        .with_field("user_id", serde_json::Value::Number(serde_json::Number::from(123)));

        let formatted = LogFormat::Human.format_entry(&entry);

        assert!(formatted.contains("ERROR"));
        assert!(formatted.contains("Error occurred"));
        assert!(formatted.contains("[app::handler]"));
        assert!(formatted.contains("user_id=123"));
    }

    #[test]
    fn test_logfmt_format() {
        let entry = LogEntry::new(
            LogLevel::Warn,
            "Warning message".to_string(),
            "app::service".to_string(),
        );

        let formatted = LogFormat::Logfmt.format_entry(&entry);

        assert!(formatted.contains("level=WARN"));
        assert!(formatted.contains("target=app::service"));
        assert!(formatted.contains("message=\"Warning message\""));
    }

    #[test]
    fn test_custom_format() {
        let entry =
            LogEntry::new(LogLevel::Debug, "Debug info".to_string(), "debug::module".to_string());

        let template = "[{level}] {target}: {message}";
        let formatted = LogFormat::Custom(template.to_string()).format_entry(&entry);

        assert_eq!(formatted, "[DEBUG] debug::module: Debug info");
    }
}
