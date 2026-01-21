//! External source fetching and parsing
//!
//! Fetches data from external URLs and parses various formats (CSV, JSON, plain text).
//! Used for automatic blacklist integration, external configuration, etc.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Format for parsing data from external sources
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SourceFormat {
    /// Plain text format - one entry per line
    PlainText,
    /// CSV format with specified column index (0-based)
    Csv { column: usize },
    /// JSON format with JSONPath to array
    Json { path: String },
}

/// Error types for external source operations
#[derive(Debug)]
pub enum FetchError {
    /// HTTP request failed
    HttpError(String),
    /// Parsing failed
    ParseError(String),
    /// Invalid format
    InvalidFormat(String),
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FetchError::HttpError(msg) => write!(f, "HTTP error: {}", msg),
            FetchError::ParseError(msg) => write!(f, "Parse error: {}", msg),
            FetchError::InvalidFormat(msg) => write!(f, "Invalid format: {}", msg),
        }
    }
}

impl std::error::Error for FetchError {}

/// Fetches and parses data from external sources
pub struct ExternalSourceFetcher {
    client: reqwest::Client,
    headers: HashMap<String, String>,
}

impl ExternalSourceFetcher {
    /// Create a new fetcher with default settings
    pub fn new() -> Self {
        Self { client: reqwest::Client::new(), headers: HashMap::new() }
    }

    /// Create a new fetcher with custom headers
    pub fn with_headers(headers: HashMap<String, String>) -> Self {
        Self { client: reqwest::Client::new(), headers }
    }

    /// Fetch and parse data from a URL
    ///
    /// # Arguments
    /// * `url` - The URL to fetch from
    /// * `format` - The format of the data
    ///
    /// # Returns
    /// A vector of parsed entries
    pub async fn fetch_and_parse(
        &self,
        url: &str,
        format: &SourceFormat,
    ) -> Result<Vec<String>, FetchError> {
        // Build request with custom headers
        let mut request = self.client.get(url);
        for (key, value) in &self.headers {
            request = request.header(key, value);
        }

        // Fetch data
        let response = request.send().await.map_err(|e| FetchError::HttpError(e.to_string()))?;

        let text = response.text().await.map_err(|e| FetchError::HttpError(e.to_string()))?;

        // Parse based on format
        self.parse(&text, format)
    }

    /// Parse text data according to format
    fn parse(&self, text: &str, format: &SourceFormat) -> Result<Vec<String>, FetchError> {
        match format {
            SourceFormat::PlainText => self.parse_plain_text(text),
            SourceFormat::Csv { column } => self.parse_csv(text, *column),
            SourceFormat::Json { path } => self.parse_json(text, path),
        }
    }

    /// Parse plain text (one entry per line)
    fn parse_plain_text(&self, text: &str) -> Result<Vec<String>, FetchError> {
        Ok(text
            .lines()
            .map(|line| line.trim())
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .map(|line| line.to_string())
            .collect())
    }

    /// Parse CSV and extract specified column
    fn parse_csv(&self, text: &str, column: usize) -> Result<Vec<String>, FetchError> {
        let mut reader = csv::ReaderBuilder::new().has_headers(false).from_reader(text.as_bytes());

        let mut results = Vec::new();
        for result in reader.records() {
            let record = result.map_err(|e| FetchError::ParseError(e.to_string()))?;
            if let Some(value) = record.get(column) {
                let trimmed = value.trim();
                if !trimmed.is_empty() {
                    results.push(trimmed.to_string());
                }
            }
        }

        Ok(results)
    }

    /// Parse JSON and extract array at JSONPath
    fn parse_json(&self, text: &str, path: &str) -> Result<Vec<String>, FetchError> {
        // TODO: Implement JSONPath parsing
        // For now, simple implementation
        let value: serde_json::Value =
            serde_json::from_str(text).map_err(|e| FetchError::ParseError(e.to_string()))?;

        if let Some(array) = value.as_array() {
            Ok(array.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
        } else {
            Err(FetchError::InvalidFormat(format!("Expected array at path: {}", path)))
        }
    }
}

impl Default for ExternalSourceFetcher {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_plain_text() {
        let fetcher = ExternalSourceFetcher::new();
        let text = "192.168.1.1\n192.168.1.2\n# comment\n\n192.168.1.3";
        let result = fetcher.parse_plain_text(text).unwrap();
        assert_eq!(result, vec!["192.168.1.1", "192.168.1.2", "192.168.1.3"]);
    }

    #[test]
    fn test_parse_csv() {
        let fetcher = ExternalSourceFetcher::new();
        let text = "192.168.1.1,malware,high\n192.168.1.2,spam,low";
        let result = fetcher.parse_csv(text, 0).unwrap();
        assert_eq!(result, vec!["192.168.1.1", "192.168.1.2"]);
    }
}
