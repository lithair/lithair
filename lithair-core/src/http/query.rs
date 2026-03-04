//! Query parameter parsing for collection endpoints
//!
//! Supports pagination (skip/take), sorting (sort=-price), and filtering
//! (field=value, field=>value, field=<value, field=~value).

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
        let (key, value) = match pair.split_once('=') {
            Some((k, v)) => (k, v),
            None => continue,
        };

        match key {
            "skip" => {
                skip = value.parse().unwrap_or(0);
            }
            "take" => {
                take = value.parse().ok();
            }
            "sort" => {
                if let Some(field) = value.strip_prefix('-') {
                    sort = Some(SortSpec { field: field.to_string(), descending: true });
                } else {
                    sort = Some(SortSpec { field: value.to_string(), descending: false });
                }
            }
            field if !RESERVED_PARAMS.contains(&field) => {
                let (op, val) = parse_filter_value(value);
                filters.push(FilterSpec {
                    field: field.to_string(),
                    op,
                    value: val.to_string(),
                });
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
        FilterOp::Gt => value_compare(field_value, &filter.value) == Some(std::cmp::Ordering::Greater),
        FilterOp::Lt => value_compare(field_value, &filter.value) == Some(std::cmp::Ordering::Less),
        FilterOp::Gte => value_compare(field_value, &filter.value).is_some_and(|o| o != std::cmp::Ordering::Less),
        FilterOp::Lte => value_compare(field_value, &filter.value).is_some_and(|o| o != std::cmp::Ordering::Greater),
        FilterOp::Contains => value_contains(field_value, &filter.value),
    }
}

/// Check if a JSON value equals a string representation
fn value_equals(value: &serde_json::Value, target: &str) -> bool {
    match value {
        serde_json::Value::String(s) => s == target,
        serde_json::Value::Number(n) => n.to_string() == target,
        serde_json::Value::Bool(b) => {
            (target == "true" && *b) || (target == "false" && !*b)
        }
        serde_json::Value::Null => target == "null" || target.is_empty(),
        _ => {
            let s = value.to_string();
            s == target
        }
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
        assert!(matches_filter(&item, &FilterSpec { field: "name".into(), op: FilterOp::Eq, value: "Alice".into() }));
        assert!(!matches_filter(&item, &FilterSpec { field: "name".into(), op: FilterOp::Eq, value: "Bob".into() }));
        assert!(matches_filter(&item, &FilterSpec { field: "age".into(), op: FilterOp::Eq, value: "30".into() }));
        assert!(matches_filter(&item, &FilterSpec { field: "active".into(), op: FilterOp::Eq, value: "true".into() }));
    }

    #[test]
    fn test_matches_filter_gt_lt() {
        let item = json!({"price": 50});
        assert!(matches_filter(&item, &FilterSpec { field: "price".into(), op: FilterOp::Gt, value: "30".into() }));
        assert!(!matches_filter(&item, &FilterSpec { field: "price".into(), op: FilterOp::Gt, value: "50".into() }));
        assert!(matches_filter(&item, &FilterSpec { field: "price".into(), op: FilterOp::Lt, value: "100".into() }));
        assert!(matches_filter(&item, &FilterSpec { field: "price".into(), op: FilterOp::Gte, value: "50".into() }));
        assert!(matches_filter(&item, &FilterSpec { field: "price".into(), op: FilterOp::Lte, value: "50".into() }));
    }

    #[test]
    fn test_matches_filter_contains() {
        let item = json!({"name": "Alice Johnson"});
        assert!(matches_filter(&item, &FilterSpec { field: "name".into(), op: FilterOp::Contains, value: "alice".into() }));
        assert!(matches_filter(&item, &FilterSpec { field: "name".into(), op: FilterOp::Contains, value: "John".into() }));
        assert!(!matches_filter(&item, &FilterSpec { field: "name".into(), op: FilterOp::Contains, value: "Bob".into() }));
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
}
