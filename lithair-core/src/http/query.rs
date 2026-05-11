//! Query parameter parsing for collection endpoints
//!
//! Supports pagination (skip/take), sorting (sort=-price), and filtering
//! (field=value, field=>value, field=<value, field=~value).

/// Default maximum number of items returned per page when `take` is not specified.
/// Prevents unbounded collection responses.
pub const DEFAULT_MAX_TAKE: u64 = 1000;

/// Parsed query parameters for collection list endpoints
#[derive(Debug, Clone)]
pub struct QueryParams {
    pub skip: u64,
    pub take: Option<u64>,
    pub sort: Option<SortSpec>,
    pub filters: Vec<FilterSpec>,
}

/// Sort specification
#[derive(Debug, Clone)]
pub struct SortSpec {
    pub field: String,
    pub descending: bool,
}

/// Filter specification
#[derive(Debug, Clone)]
pub struct FilterSpec {
    pub field: String,
    pub op: FilterOp,
    pub value: String,
}

/// Filter operation
#[derive(Debug, Clone, PartialEq)]
pub enum FilterOp {
    /// Exact equality
    Eq,
    /// Not equal
    Ne,
    /// Greater than
    Gt,
    /// Less than
    Lt,
    /// Greater than or equal
    Gte,
    /// Less than or equal
    Lte,
    /// String contains (case-insensitive)
    Contains,
}

/// Reserved query parameter names that are not treated as filters
const RESERVED_PARAMS: &[&str] = &["skip", "take", "sort"];

/// Decode a percent-encoded URL string (e.g. "hello%20world" → "hello world")
pub fn percent_decode(input: &str) -> String {
    let mut out = Vec::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2])) {
                out.push(hi << 4 | lo);
                i += 3;
                continue;
            }
        } else if bytes[i] == b'+' {
            out.push(b' ');
            i += 1;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| input.to_string())
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Extract a single `key=value` from a query string, percent-decoded.
///
/// This is the right primitive for custom routes that need one literal
/// query parameter (e.g. `GET /api/fs?path=<dir>`). Unlike
/// [`parse_query_params`], it does **not** apply any filter-spec
/// semantics — values like `>foo` are returned as the literal string
/// `">foo"`, not parsed as a `Gt` filter.
///
/// Both the key and the value are percent-decoded via [`percent_decode`],
/// which means `+` is interpreted as a space (the form-urlencoded
/// convention used throughout this module).
///
/// Returns `None` if:
/// - the query string is empty,
/// - the key is absent,
/// - the pair has no `=` (e.g. `?path` with no value),
/// - the decoded value is the empty string.
///
/// When the same key appears multiple times, the **first occurrence wins**.
///
/// # Examples
///
/// ```
/// use lithair_core::http::query;
///
/// assert_eq!(query::param("path=/etc", "path"), Some("/etc".to_string()));
/// assert_eq!(query::param("path=%2Fetc", "path"), Some("/etc".to_string()));
/// assert_eq!(query::param("filter=%3Efoo", "filter"), Some(">foo".to_string()));
/// assert_eq!(query::param("path=", "path"), None);
/// assert_eq!(query::param("", "path"), None);
/// ```
pub fn param(query: &str, key: &str) -> Option<String> {
    if query.is_empty() {
        return None;
    }
    for pair in query.split('&') {
        let (raw_key, raw_value) = match pair.split_once('=') {
            Some(kv) => kv,
            None => continue,
        };
        if percent_decode(raw_key) == key {
            let decoded = percent_decode(raw_value);
            if decoded.is_empty() {
                return None;
            }
            return Some(decoded);
        }
    }
    None
}

/// Parse query string into structured QueryParams
pub fn parse_query_params(query: &str) -> QueryParams {
    let mut skip = 0u64;
    let mut take = None;
    let mut sort = None;
    let mut filters = Vec::new();

    if query.is_empty() {
        return QueryParams { skip, take, sort, filters };
    }

    for pair in query.split('&') {
        let (key, raw_value) = match pair.split_once('=') {
            Some((k, v)) => (k, v),
            None => continue,
        };

        match key {
            "skip" => {
                skip = raw_value.parse().unwrap_or(0);
            }
            "take" => {
                take = raw_value.parse().ok();
            }
            "sort" => {
                let value = percent_decode(raw_value);
                if let Some(field) = value.strip_prefix('-') {
                    sort = Some(SortSpec { field: field.to_string(), descending: true });
                } else {
                    sort = Some(SortSpec { field: value.to_string(), descending: false });
                }
            }
            field if !RESERVED_PARAMS.contains(&field) => {
                let decoded_value = percent_decode(raw_value);
                let (op, val) = parse_filter_value(&decoded_value);
                let decoded_field = percent_decode(field);
                filters.push(FilterSpec { field: decoded_field, op, value: val.to_string() });
            }
            _ => {}
        }
    }

    QueryParams { skip, take, sort, filters }
}

/// Parse a filter value to extract the operation
/// Syntax: `value` (eq), `!value` (ne), `>value` (gt), `<value` (lt),
/// `>=value` (gte), `<=value` (lte), `~value` (contains)
fn parse_filter_value(value: &str) -> (FilterOp, &str) {
    if let Some(v) = value.strip_prefix(">=") {
        (FilterOp::Gte, v)
    } else if let Some(v) = value.strip_prefix("<=") {
        (FilterOp::Lte, v)
    } else if let Some(v) = value.strip_prefix('>') {
        (FilterOp::Gt, v)
    } else if let Some(v) = value.strip_prefix('<') {
        (FilterOp::Lt, v)
    } else if let Some(v) = value.strip_prefix('~') {
        (FilterOp::Contains, v)
    } else if let Some(v) = value.strip_prefix('!') {
        (FilterOp::Ne, v)
    } else {
        (FilterOp::Eq, value)
    }
}

/// Apply a filter to a JSON value
pub fn matches_filter(item: &serde_json::Value, filter: &FilterSpec) -> bool {
    let field_value = match item.get(&filter.field) {
        Some(v) => v,
        None => return false,
    };

    match filter.op {
        FilterOp::Eq => value_equals(field_value, &filter.value),
        FilterOp::Ne => !value_equals(field_value, &filter.value),
        FilterOp::Gt => {
            value_compare(field_value, &filter.value) == Some(std::cmp::Ordering::Greater)
        }
        FilterOp::Lt => value_compare(field_value, &filter.value) == Some(std::cmp::Ordering::Less),
        FilterOp::Gte => {
            value_compare(field_value, &filter.value).is_some_and(|o| o != std::cmp::Ordering::Less)
        }
        FilterOp::Lte => value_compare(field_value, &filter.value)
            .is_some_and(|o| o != std::cmp::Ordering::Greater),
        FilterOp::Contains => value_contains(field_value, &filter.value),
    }
}

/// Check if a JSON value equals a string representation
fn value_equals(value: &serde_json::Value, target: &str) -> bool {
    match value {
        serde_json::Value::String(s) => s == target,
        serde_json::Value::Number(n) => {
            if let (Some(val), Ok(tgt)) = (n.as_f64(), target.parse::<f64>()) {
                (val - tgt).abs() < f64::EPSILON
            } else {
                n.to_string() == target
            }
        }
        serde_json::Value::Bool(b) => (target == "true" && *b) || (target == "false" && !*b),
        serde_json::Value::Null => target == "null" || target.is_empty(),
        // Arrays and objects cannot be meaningfully compared via query string
        _ => false,
    }
}

/// Compare a JSON value with a string representation
fn value_compare(value: &serde_json::Value, target: &str) -> Option<std::cmp::Ordering> {
    match value {
        serde_json::Value::Number(n) => {
            let target_num = target.parse::<f64>().ok()?;
            let val_num = n.as_f64()?;
            val_num.partial_cmp(&target_num)
        }
        serde_json::Value::String(s) => Some(s.as_str().cmp(target)),
        serde_json::Value::Bool(b) => {
            let target_bool = match target {
                "true" => true,
                "false" => false,
                _ => return None,
            };
            Some(b.cmp(&target_bool))
        }
        _ => None,
    }
}

/// Check if a JSON value contains a substring (case-insensitive)
fn value_contains(value: &serde_json::Value, needle: &str) -> bool {
    let needle_lower = needle.to_lowercase();
    match value {
        serde_json::Value::String(s) => s.to_lowercase().contains(&needle_lower),
        serde_json::Value::Number(n) => n.to_string().contains(needle),
        _ => value.to_string().to_lowercase().contains(&needle_lower),
    }
}

/// Compare two JSON values for sorting
pub fn compare_json_values(a: &serde_json::Value, b: &serde_json::Value) -> std::cmp::Ordering {
    match (a, b) {
        (serde_json::Value::Number(a), serde_json::Value::Number(b)) => {
            let af = a.as_f64().unwrap_or(0.0);
            let bf = b.as_f64().unwrap_or(0.0);
            af.partial_cmp(&bf).unwrap_or(std::cmp::Ordering::Equal)
        }
        (serde_json::Value::String(a), serde_json::Value::String(b)) => a.cmp(b),
        (serde_json::Value::Bool(a), serde_json::Value::Bool(b)) => a.cmp(b),
        (serde_json::Value::Null, serde_json::Value::Null) => std::cmp::Ordering::Equal,
        (serde_json::Value::Null, _) => std::cmp::Ordering::Less,
        (_, serde_json::Value::Null) => std::cmp::Ordering::Greater,
        _ => std::cmp::Ordering::Equal,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_parse_empty_query() {
        let params = parse_query_params("");
        assert_eq!(params.skip, 0);
        assert!(params.take.is_none());
        assert!(params.sort.is_none());
        assert!(params.filters.is_empty());
    }

    #[test]
    fn test_parse_pagination() {
        let params = parse_query_params("skip=10&take=20");
        assert_eq!(params.skip, 10);
        assert_eq!(params.take, Some(20));
    }

    #[test]
    fn test_parse_sort_asc() {
        let params = parse_query_params("sort=name");
        let sort = params.sort.unwrap();
        assert_eq!(sort.field, "name");
        assert!(!sort.descending);
    }

    #[test]
    fn test_parse_sort_desc() {
        let params = parse_query_params("sort=-price");
        let sort = params.sort.unwrap();
        assert_eq!(sort.field, "price");
        assert!(sort.descending);
    }

    #[test]
    fn test_parse_filters() {
        let params = parse_query_params("status=active&price=>100&name=~foo");
        assert_eq!(params.filters.len(), 3);
        assert_eq!(params.filters[0].field, "status");
        assert_eq!(params.filters[0].op, FilterOp::Eq);
        assert_eq!(params.filters[0].value, "active");
        assert_eq!(params.filters[1].field, "price");
        assert_eq!(params.filters[1].op, FilterOp::Gt);
        assert_eq!(params.filters[1].value, "100");
        assert_eq!(params.filters[2].field, "name");
        assert_eq!(params.filters[2].op, FilterOp::Contains);
        assert_eq!(params.filters[2].value, "foo");
    }

    #[test]
    fn test_matches_filter_eq() {
        let item = json!({"name": "Alice", "age": 30, "active": true});
        assert!(matches_filter(
            &item,
            &FilterSpec { field: "name".into(), op: FilterOp::Eq, value: "Alice".into() }
        ));
        assert!(!matches_filter(
            &item,
            &FilterSpec { field: "name".into(), op: FilterOp::Eq, value: "Bob".into() }
        ));
        assert!(matches_filter(
            &item,
            &FilterSpec { field: "age".into(), op: FilterOp::Eq, value: "30".into() }
        ));
        assert!(matches_filter(
            &item,
            &FilterSpec { field: "active".into(), op: FilterOp::Eq, value: "true".into() }
        ));
    }

    #[test]
    fn test_matches_filter_gt_lt() {
        let item = json!({"price": 50});
        assert!(matches_filter(
            &item,
            &FilterSpec { field: "price".into(), op: FilterOp::Gt, value: "30".into() }
        ));
        assert!(!matches_filter(
            &item,
            &FilterSpec { field: "price".into(), op: FilterOp::Gt, value: "50".into() }
        ));
        assert!(matches_filter(
            &item,
            &FilterSpec { field: "price".into(), op: FilterOp::Lt, value: "100".into() }
        ));
        assert!(matches_filter(
            &item,
            &FilterSpec { field: "price".into(), op: FilterOp::Gte, value: "50".into() }
        ));
        assert!(matches_filter(
            &item,
            &FilterSpec { field: "price".into(), op: FilterOp::Lte, value: "50".into() }
        ));
    }

    #[test]
    fn test_matches_filter_contains() {
        let item = json!({"name": "Alice Johnson"});
        assert!(matches_filter(
            &item,
            &FilterSpec { field: "name".into(), op: FilterOp::Contains, value: "alice".into() }
        ));
        assert!(matches_filter(
            &item,
            &FilterSpec { field: "name".into(), op: FilterOp::Contains, value: "John".into() }
        ));
        assert!(!matches_filter(
            &item,
            &FilterSpec { field: "name".into(), op: FilterOp::Contains, value: "Bob".into() }
        ));
    }

    #[test]
    fn test_compare_json_values() {
        assert_eq!(compare_json_values(&json!(1), &json!(2)), std::cmp::Ordering::Less);
        assert_eq!(compare_json_values(&json!("a"), &json!("b")), std::cmp::Ordering::Less);
        assert_eq!(compare_json_values(&json!(null), &json!(1)), std::cmp::Ordering::Less);
    }

    #[test]
    fn test_parse_gte_lte() {
        let params = parse_query_params("price=>=100&age=<=30");
        assert_eq!(params.filters.len(), 2);
        assert_eq!(params.filters[0].op, FilterOp::Gte);
        assert_eq!(params.filters[0].value, "100");
        assert_eq!(params.filters[1].op, FilterOp::Lte);
        assert_eq!(params.filters[1].value, "30");
    }

    #[test]
    fn test_percent_decode_basic() {
        assert_eq!(percent_decode("hello%20world"), "hello world");
        assert_eq!(percent_decode("foo%26bar"), "foo&bar");
        assert_eq!(percent_decode("100%25"), "100%");
        assert_eq!(percent_decode("no+encoding+needed"), "no encoding needed");
    }

    #[test]
    fn test_percent_decode_passthrough() {
        assert_eq!(percent_decode("simple"), "simple");
        assert_eq!(percent_decode(""), "");
        assert_eq!(percent_decode("abc-123_def.txt"), "abc-123_def.txt");
    }

    #[test]
    fn test_percent_decode_incomplete() {
        // Incomplete sequences are left as-is
        assert_eq!(percent_decode("foo%2"), "foo%2");
        assert_eq!(percent_decode("foo%"), "foo%");
        assert_eq!(percent_decode("foo%ZZ"), "foo%ZZ");
    }

    #[test]
    fn test_filter_with_encoded_value() {
        let params = parse_query_params("name=hello%20world&city=S%C3%A3o+Paulo");
        assert_eq!(params.filters.len(), 2);
        assert_eq!(params.filters[0].value, "hello world");
        assert_eq!(params.filters[1].value, "São Paulo");
    }

    // ---------- query::param ----------

    #[test]
    fn test_param_empty_query() {
        assert_eq!(param("", "path"), None);
    }

    #[test]
    fn test_param_basic_match() {
        assert_eq!(param("path=/etc", "path"), Some("/etc".to_string()));
    }

    #[test]
    fn test_param_empty_value_returns_none() {
        // Per spec: empty value after decode → None.
        assert_eq!(param("path=", "path"), None);
    }

    #[test]
    fn test_param_picks_correct_key_among_many() {
        assert_eq!(param("foo=1&path=/etc&bar=2", "path"), Some("/etc".to_string()));
        assert_eq!(param("foo=1&path=/etc&bar=2", "foo"), Some("1".to_string()));
        assert_eq!(param("foo=1&path=/etc&bar=2", "bar"), Some("2".to_string()));
    }

    #[test]
    fn test_param_percent_decodes_value() {
        assert_eq!(param("path=%2Fetc", "path"), Some("/etc".to_string()));
        assert_eq!(param("name=hello%20world", "name"), Some("hello world".to_string()));
    }

    #[test]
    fn test_param_missing_key_returns_none() {
        assert_eq!(param("foo=1", "missing"), None);
    }

    #[test]
    fn test_param_key_without_equals_returns_none() {
        // Bare key with no `=` is not a value-bearing pair.
        assert_eq!(param("path", "path"), None);
        // …but a value-bearing pair after a bare key is still findable.
        assert_eq!(param("flag&path=/etc", "path"), Some("/etc".to_string()));
        // And asking for the bare key still returns None.
        assert_eq!(param("flag&path=/etc", "flag"), None);
    }

    #[test]
    fn test_param_multiple_occurrences_first_wins() {
        // Documented convention: first occurrence wins.
        assert_eq!(param("path=/first&path=/second", "path"), Some("/first".to_string()));
    }

    #[test]
    fn test_param_does_not_split_on_encoded_equals() {
        // `%3D` inside the value must NOT be confused with the key/value
        // delimiter. The raw `=` that separates key from value is the
        // first literal `=` in the pair.
        assert_eq!(param("filter=a%3Db", "filter"), Some("a=b".to_string()));
    }

    #[test]
    fn test_param_does_not_apply_filter_semantics() {
        // The whole point of this helper: ">foo" comes back as ">foo",
        // not as a Gt operator. (Compare with parse_query_params which
        // would parse this as FilterOp::Gt + value="foo".)
        assert_eq!(param("filter=%3Efoo", "filter"), Some(">foo".to_string()));
        // Same for the literal (non-encoded) form.
        assert_eq!(param("filter=>foo", "filter"), Some(">foo".to_string()));
        assert_eq!(param("filter=!bar", "filter"), Some("!bar".to_string()));
        assert_eq!(param("filter=~baz", "filter"), Some("~baz".to_string()));
    }

    #[test]
    fn test_param_decodes_key() {
        // The key in the query string is also percent-decoded before
        // matching, mirroring parse_query_params behavior.
        assert_eq!(param("my%20key=value", "my key"), Some("value".to_string()));
    }

    #[test]
    fn test_param_value_with_equals_kept_intact() {
        // Only the FIRST `=` splits key from value; further `=` are
        // part of the value (e.g. base64 padding).
        assert_eq!(param("token=abc=def==", "token"), Some("abc=def==".to_string()));
    }
}
