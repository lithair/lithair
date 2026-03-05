//! CPU usage from `/proc/stat`.
//!
//! Reads the aggregate `cpu` line and computes usage as a delta between two
//! readings (idle time vs total time).

/// Raw jiffies parsed from the first `cpu` line of `/proc/stat`.
#[derive(Clone, Debug)]
pub struct CpuJiffies {
    pub idle: u64,
    pub total: u64,
}

/// Parse the aggregate `cpu` line from `/proc/stat` content.
///
/// Format: `cpu  user nice system idle iowait irq softirq steal [guest guest_nice]`
pub fn parse_proc_stat(content: &str) -> Option<CpuJiffies> {
    let line = content.lines().find(|l| l.starts_with("cpu "))?;
    let fields: Vec<u64> = line
        .split_whitespace()
        .skip(1) // skip "cpu"
        .filter_map(|s| s.parse().ok())
        .collect();

    if fields.len() < 4 {
        return None;
    }

    // user(0) + nice(1) + system(2) + idle(3) + iowait(4) + irq(5) + softirq(6) + steal(7)
    let total: u64 = fields.iter().sum();
    let idle = fields[3] + fields.get(4).copied().unwrap_or(0); // idle + iowait

    Some(CpuJiffies { idle, total })
}

/// Read and parse `/proc/stat` from the filesystem.
pub fn read_cpu_jiffies() -> Option<CpuJiffies> {
    let content = std::fs::read_to_string("/proc/stat").ok()?;
    parse_proc_stat(&content)
}

/// Compute CPU usage percentage from two successive readings.
///
/// Returns 0.0 if there is no delta (identical readings).
pub fn cpu_usage_percent(prev: &CpuJiffies, curr: &CpuJiffies) -> f64 {
    let total_delta = curr.total.saturating_sub(prev.total);
    if total_delta == 0 {
        return 0.0;
    }
    let idle_delta = curr.idle.saturating_sub(prev.idle);
    let used = total_delta - idle_delta;
    (used as f64 / total_delta as f64) * 100.0
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_PROC_STAT: &str = "\
cpu  10132153 290696 3084719 46828483 16683 0 25195 0 0 0
cpu0 1393280 32966 572056 13343292 6130 0 17875 0 0 0
";

    #[test]
    fn parse_jiffies() {
        let j = parse_proc_stat(SAMPLE_PROC_STAT).unwrap();
        // total = 10132153+290696+3084719+46828483+16683+0+25195+0+0+0 = 60377929
        assert_eq!(j.total, 60_377_929);
        // idle = 46828483 + 16683 = 46845166
        assert_eq!(j.idle, 46_845_166);
    }

    #[test]
    fn cpu_usage_delta() {
        let prev = CpuJiffies { idle: 100, total: 200 };
        let curr = CpuJiffies { idle: 110, total: 230 };
        // delta total=30, delta idle=10, used=20 → 66.67%
        let pct = cpu_usage_percent(&prev, &curr);
        assert!((pct - 66.666).abs() < 0.1);
    }

    #[test]
    fn cpu_usage_no_delta() {
        let j = CpuJiffies { idle: 100, total: 200 };
        assert_eq!(cpu_usage_percent(&j, &j), 0.0);
    }

    #[test]
    fn parse_empty() {
        assert!(parse_proc_stat("").is_none());
    }

    #[test]
    fn parse_malformed() {
        assert!(parse_proc_stat("cpu  abc def").is_none());
    }
}
