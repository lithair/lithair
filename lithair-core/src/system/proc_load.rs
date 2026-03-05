//! Load average from `/proc/loadavg`.

/// Parsed load average values.
#[derive(Clone, Debug)]
pub struct LoadAvg {
    pub load_1: f64,
    pub load_5: f64,
    pub load_15: f64,
}

/// Parse `/proc/loadavg` content.
///
/// Format: `0.50 0.35 0.25 1/234 5678`
pub fn parse_loadavg(content: &str) -> Option<LoadAvg> {
    let mut parts = content.split_whitespace();
    let load_1: f64 = parts.next()?.parse().ok()?;
    let load_5: f64 = parts.next()?.parse().ok()?;
    let load_15: f64 = parts.next()?.parse().ok()?;
    Some(LoadAvg { load_1, load_5, load_15 })
}

/// Read and parse `/proc/loadavg`.
pub fn read_loadavg() -> Option<LoadAvg> {
    let content = std::fs::read_to_string("/proc/loadavg").ok()?;
    parse_loadavg(&content)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_loadavg_sample() {
        let la = parse_loadavg("0.50 0.35 0.25 1/234 5678\n").unwrap();
        assert!((la.load_1 - 0.50).abs() < f64::EPSILON);
        assert!((la.load_5 - 0.35).abs() < f64::EPSILON);
        assert!((la.load_15 - 0.25).abs() < f64::EPSILON);
    }

    #[test]
    fn parse_empty() {
        assert!(parse_loadavg("").is_none());
    }

    #[test]
    fn parse_partial() {
        assert!(parse_loadavg("0.50 0.35").is_none());
    }
}
