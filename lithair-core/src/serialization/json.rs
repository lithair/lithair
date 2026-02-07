//! Custom JSON parser and serializer with zero dependencies

use std::collections::HashMap;

/// JSON value representation
#[derive(Debug, Clone, PartialEq)]
pub enum JsonValue {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<JsonValue>),
    Object(HashMap<String, JsonValue>),
}

/// JSON parsing and serialization errors
#[derive(Debug, Clone)]
pub enum JsonError {
    InvalidSyntax(String),
    UnexpectedToken(String),
    UnexpectedEnd,
    InvalidNumber(String),
    InvalidString(String),
    InvalidEscape(String),
}

impl std::fmt::Display for JsonError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            JsonError::InvalidSyntax(msg) => write!(f, "Invalid JSON syntax: {}", msg),
            JsonError::UnexpectedToken(token) => write!(f, "Unexpected token: {}", token),
            JsonError::UnexpectedEnd => write!(f, "Unexpected end of input"),
            JsonError::InvalidNumber(num) => write!(f, "Invalid number: {}", num),
            JsonError::InvalidString(s) => write!(f, "Invalid string: {}", s),
            JsonError::InvalidEscape(esc) => write!(f, "Invalid escape sequence: {}", esc),
        }
    }
}

impl std::error::Error for JsonError {}

/// Parse a JSON string into a JsonValue
///
/// # Example
///
/// ```rust
/// use lithair_core::serialization::json::{parse_json, JsonValue};
///
/// let json = r#"{"name": "John", "age": 30}"#;
/// let value = parse_json(json).unwrap();
///
/// if let JsonValue::Object(obj) = value {
///     assert_eq!(obj.get("name"), Some(&JsonValue::String("John".to_string())));
/// }
/// ```
pub fn parse_json(input: &str) -> Result<JsonValue, JsonError> {
    let mut parser = JsonParser::new(input);
    parser.parse_value()
}

/// Convert a JsonValue to a JSON string
///
/// # Example
///
/// ```rust
/// use lithair_core::serialization::json::{stringify_json, JsonValue};
/// use std::collections::HashMap;
///
/// let mut obj = HashMap::new();
/// obj.insert("name".to_string(), JsonValue::String("John".to_string()));
/// obj.insert("age".to_string(), JsonValue::Number(30.0));
///
/// let value = JsonValue::Object(obj);
/// let json_string = stringify_json(&value);
///
/// assert!(json_string.contains("\"name\":\"John\""));
/// ```
pub fn stringify_json(value: &JsonValue) -> String {
    match value {
        JsonValue::Null => "null".to_string(),
        JsonValue::Bool(b) => b.to_string(),
        JsonValue::Number(n) => {
            if n.fract() == 0.0 && n.is_finite() {
                format!("{}", *n as i64)
            } else {
                n.to_string()
            }
        }
        JsonValue::String(s) => format!("\"{}\"", escape_string(s)),
        JsonValue::Array(arr) => {
            let items: Vec<String> = arr.iter().map(stringify_json).collect();
            format!("[{}]", items.join(","))
        }
        JsonValue::Object(obj) => {
            let items: Vec<String> = obj
                .iter()
                .map(|(k, v)| format!("\"{}\":{}", escape_string(k), stringify_json(v)))
                .collect();
            format!("{{{}}}", items.join(","))
        }
    }
}

/// JSON parser implementation
struct JsonParser {
    chars: Vec<char>,
    pos: usize,
}

impl JsonParser {
    fn new(input: &str) -> Self {
        Self { chars: input.chars().collect(), pos: 0 }
    }

    fn parse_value(&mut self) -> Result<JsonValue, JsonError> {
        self.skip_whitespace();

        if self.pos >= self.chars.len() {
            return Err(JsonError::UnexpectedEnd);
        }

        match self.chars[self.pos] {
            'n' => self.parse_null(),
            't' | 'f' => self.parse_bool(),
            '"' => self.parse_string(),
            '[' => self.parse_array(),
            '{' => self.parse_object(),
            c if c.is_ascii_digit() || c == '-' => self.parse_number(),
            c => Err(JsonError::UnexpectedToken(c.to_string())),
        }
    }

    fn parse_null(&mut self) -> Result<JsonValue, JsonError> {
        if self.consume_word("null") {
            Ok(JsonValue::Null)
        } else {
            Err(JsonError::InvalidSyntax("Expected 'null'".to_string()))
        }
    }

    fn parse_bool(&mut self) -> Result<JsonValue, JsonError> {
        if self.consume_word("true") {
            Ok(JsonValue::Bool(true))
        } else if self.consume_word("false") {
            Ok(JsonValue::Bool(false))
        } else {
            Err(JsonError::InvalidSyntax("Expected 'true' or 'false'".to_string()))
        }
    }

    fn parse_string(&mut self) -> Result<JsonValue, JsonError> {
        // Note: escape sequences inside strings are not yet handled during parsing
        self.pos += 1; // Skip opening quote
        let start = self.pos;

        while self.pos < self.chars.len() && self.chars[self.pos] != '"' {
            self.pos += 1;
        }

        if self.pos >= self.chars.len() {
            return Err(JsonError::UnexpectedEnd);
        }

        let content: String = self.chars[start..self.pos].iter().collect();
        self.pos += 1; // Skip closing quote

        Ok(JsonValue::String(content))
    }

    fn parse_number(&mut self) -> Result<JsonValue, JsonError> {
        let start = self.pos;

        if self.chars[self.pos] == '-' {
            self.pos += 1;
        }

        while self.pos < self.chars.len()
            && (self.chars[self.pos].is_ascii_digit() || self.chars[self.pos] == '.')
        {
            self.pos += 1;
        }

        let num_str: String = self.chars[start..self.pos].iter().collect();
        num_str
            .parse::<f64>()
            .map(JsonValue::Number)
            .map_err(|_| JsonError::InvalidNumber(num_str))
    }

    fn parse_array(&mut self) -> Result<JsonValue, JsonError> {
        self.pos += 1; // Skip '['
        self.skip_whitespace();

        let mut items = Vec::new();

        if self.pos < self.chars.len() && self.chars[self.pos] == ']' {
            self.pos += 1;
            return Ok(JsonValue::Array(items));
        }

        loop {
            items.push(self.parse_value()?);
            self.skip_whitespace();

            if self.pos >= self.chars.len() {
                return Err(JsonError::UnexpectedEnd);
            }

            match self.chars[self.pos] {
                ']' => {
                    self.pos += 1;
                    break;
                }
                ',' => {
                    self.pos += 1;
                    self.skip_whitespace();
                }
                c => return Err(JsonError::UnexpectedToken(c.to_string())),
            }
        }

        Ok(JsonValue::Array(items))
    }

    fn parse_object(&mut self) -> Result<JsonValue, JsonError> {
        self.pos += 1; // Skip '{'
        self.skip_whitespace();

        let mut obj = HashMap::new();

        if self.pos < self.chars.len() && self.chars[self.pos] == '}' {
            self.pos += 1;
            return Ok(JsonValue::Object(obj));
        }

        loop {
            // Parse key
            let key = match self.parse_string()? {
                JsonValue::String(s) => s,
                _ => return Err(JsonError::InvalidSyntax("Object key must be string".to_string())),
            };

            self.skip_whitespace();

            // Expect ':'
            if self.pos >= self.chars.len() || self.chars[self.pos] != ':' {
                return Err(JsonError::InvalidSyntax("Expected ':' after object key".to_string()));
            }
            self.pos += 1;

            // Parse value
            let value = self.parse_value()?;
            obj.insert(key, value);

            self.skip_whitespace();

            if self.pos >= self.chars.len() {
                return Err(JsonError::UnexpectedEnd);
            }

            match self.chars[self.pos] {
                '}' => {
                    self.pos += 1;
                    break;
                }
                ',' => {
                    self.pos += 1;
                    self.skip_whitespace();
                }
                c => return Err(JsonError::UnexpectedToken(c.to_string())),
            }
        }

        Ok(JsonValue::Object(obj))
    }

    fn skip_whitespace(&mut self) {
        while self.pos < self.chars.len() && self.chars[self.pos].is_whitespace() {
            self.pos += 1;
        }
    }

    fn consume_word(&mut self, word: &str) -> bool {
        if self.pos + word.len() > self.chars.len() {
            return false;
        }

        let slice: String = self.chars[self.pos..self.pos + word.len()].iter().collect();
        if slice == word {
            self.pos += word.len();
            true
        } else {
            false
        }
    }
}

/// Escape a string for JSON output
fn escape_string(s: &str) -> String {
    let mut result = String::new();

    for c in s.chars() {
        match c {
            '"' => result.push_str("\\\""),
            '\\' => result.push_str("\\\\"),
            '\n' => result.push_str("\\n"),
            '\r' => result.push_str("\\r"),
            '\t' => result.push_str("\\t"),
            c => result.push(c),
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_null() {
        assert_eq!(parse_json("null").unwrap(), JsonValue::Null);
    }

    #[test]
    fn test_parse_bool() {
        assert_eq!(parse_json("true").unwrap(), JsonValue::Bool(true));
        assert_eq!(parse_json("false").unwrap(), JsonValue::Bool(false));
    }

    #[test]
    fn test_parse_number() {
        assert_eq!(parse_json("42").unwrap(), JsonValue::Number(42.0));
        let pi = std::f64::consts::PI;
        let s = format!("-{}", pi);
        assert_eq!(parse_json(&s).unwrap(), JsonValue::Number(-pi));
    }

    #[test]
    fn test_parse_string() {
        assert_eq!(parse_json("\"hello\"").unwrap(), JsonValue::String("hello".to_string()));
    }

    #[test]
    fn test_parse_array() {
        let result = parse_json("[1, 2, 3]").unwrap();
        if let JsonValue::Array(arr) = result {
            assert_eq!(arr.len(), 3);
            assert_eq!(arr[0], JsonValue::Number(1.0));
        } else {
            panic!("Expected array");
        }
    }

    #[test]
    fn test_stringify() {
        let value = JsonValue::String("hello".to_string());
        assert_eq!(stringify_json(&value), "\"hello\"");

        let value = JsonValue::Number(42.0);
        assert_eq!(stringify_json(&value), "42");
    }
}
