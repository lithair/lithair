//! Request statistics computed from the AccessLogBuffer.

/// Request throughput and latency percentiles.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct RequestStats {
    pub rps_60s: f64,
    pub latency_p50_ms: f64,
    pub latency_p95_ms: f64,
    pub latency_p99_ms: f64,
    pub total_in_buffer: usize,
}

/// Compute request statistics over a rolling window.
///
/// Uses the global [`AccessLogBuffer`](crate::http::AccessLogBuffer) to derive
/// requests-per-second (over `window_seconds`) and latency percentiles.
pub fn compute_request_stats(window_seconds: u64) -> Option<RequestStats> {
    let buf = crate::http::access_log_buffer()?;
    let total_in_buffer = buf.len();

    let since = chrono::Utc::now() - chrono::Duration::seconds(window_seconds as i64);
    let since_str = since.to_rfc3339();
    let entries = buf.entries_since(&since_str);

    let count = entries.len();
    let rps = if window_seconds > 0 { count as f64 / window_seconds as f64 } else { 0.0 };

    let (p50, p95, p99) = if count == 0 {
        (0.0, 0.0, 0.0)
    } else {
        let mut durations: Vec<f64> = entries.iter().map(|e| e.dur_us as f64 / 1000.0).collect();
        durations.sort_unstable_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        (
            percentile(&durations, 50.0),
            percentile(&durations, 95.0),
            percentile(&durations, 99.0),
        )
    };

    Some(RequestStats {
        rps_60s: rps,
        latency_p50_ms: p50,
        latency_p95_ms: p95,
        latency_p99_ms: p99,
        total_in_buffer,
    })
}

/// Nearest-rank percentile on a sorted slice.
fn percentile(sorted: &[f64], pct: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = ((pct / 100.0) * sorted.len() as f64).ceil() as usize;
    sorted[idx.saturating_sub(1).min(sorted.len() - 1)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentile_basic() {
        let data = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0];
        assert!((percentile(&data, 50.0) - 5.0).abs() < f64::EPSILON);
        assert!((percentile(&data, 95.0) - 10.0).abs() < f64::EPSILON);
        assert!((percentile(&data, 99.0) - 10.0).abs() < f64::EPSILON);
    }

    #[test]
    fn percentile_single() {
        assert!((percentile(&[42.0], 50.0) - 42.0).abs() < f64::EPSILON);
    }

    #[test]
    fn percentile_empty() {
        assert_eq!(percentile(&[], 50.0), 0.0);
    }
}
