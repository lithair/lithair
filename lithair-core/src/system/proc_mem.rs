//! Memory info from `/proc/meminfo`.

/// Parsed memory information.
#[derive(Clone, Debug)]
pub struct MemInfo {
    pub total_bytes: u64,
    pub available_bytes: u64,
}

impl MemInfo {
    pub fn used_bytes(&self) -> u64 {
        self.total_bytes.saturating_sub(self.available_bytes)
    }
}

/// Parse `/proc/meminfo` content.
///
/// Looks for `MemTotal:` and `MemAvailable:` lines (values in kB).
pub fn parse_meminfo(content: &str) -> Option<MemInfo> {
    let mut total_kb: Option<u64> = None;
    let mut avail_kb: Option<u64> = None;

    for line in content.lines() {
        if let Some(rest) = line.strip_prefix("MemTotal:") {
            total_kb = parse_kb_value(rest);
        } else if let Some(rest) = line.strip_prefix("MemAvailable:") {
            avail_kb = parse_kb_value(rest);
        }
        if total_kb.is_some() && avail_kb.is_some() {
            break;
        }
    }

    Some(MemInfo { total_bytes: total_kb? * 1024, available_bytes: avail_kb? * 1024 })
}

/// Parse a value like `  16384000 kB` → 16384000.
fn parse_kb_value(s: &str) -> Option<u64> {
    s.split_whitespace().next()?.parse().ok()
}

/// Read and parse `/proc/meminfo`.
pub fn read_meminfo() -> Option<MemInfo> {
    let content = std::fs::read_to_string("/proc/meminfo").ok()?;
    parse_meminfo(&content)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
MemTotal:       16384000 kB
MemFree:         2048000 kB
MemAvailable:   12288000 kB
Buffers:          512000 kB
";

    #[test]
    fn parse_meminfo_sample() {
        let m = parse_meminfo(SAMPLE).unwrap();
        assert_eq!(m.total_bytes, 16_384_000 * 1024);
        assert_eq!(m.available_bytes, 12_288_000 * 1024);
        assert_eq!(m.used_bytes(), (16_384_000 - 12_288_000) * 1024);
    }

    #[test]
    fn missing_available() {
        let content = "MemTotal:       16384000 kB\n";
        assert!(parse_meminfo(content).is_none());
    }

    #[test]
    fn missing_total() {
        let content = "MemAvailable:   12288000 kB\n";
        assert!(parse_meminfo(content).is_none());
    }
}
