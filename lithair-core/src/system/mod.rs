//! System metrics collector.
//!
//! Reads Linux `/proc` pseudo-files to expose CPU, RAM, load average, and
//! process RSS. Also computes request throughput and latency percentiles from
//! the [`AccessLogBuffer`](crate::http::AccessLogBuffer).
//!
//! # Usage
//!
//! ```rust,ignore
//! lithair_core::system::init_system_metrics();
//! let snap = lithair_core::system::system_metrics().unwrap().snapshot();
//! ```

pub mod proc_cpu;
pub mod proc_load;
pub mod proc_mem;
pub mod proc_self;
pub mod request_stats;

pub use proc_cpu::CpuJiffies;
pub use request_stats::{compute_request_stats, RequestStats};

use std::sync::{Mutex, OnceLock};

/// A point-in-time system snapshot.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SystemSnapshot {
    pub cpu_usage_percent: f64,
    pub ram_total_bytes: u64,
    pub ram_used_bytes: u64,
    pub ram_available_bytes: u64,
    pub load_avg_1: f64,
    pub load_avg_5: f64,
    pub load_avg_15: f64,
    pub process_rss_bytes: u64,
    pub timestamp: String,
}

/// Global system metrics collector.
///
/// Holds the previous CPU reading so successive calls to [`snapshot()`](Self::snapshot)
/// can compute a delta-based CPU usage percentage.
pub struct SystemMetrics {
    prev_cpu: Mutex<Option<CpuJiffies>>,
}

impl SystemMetrics {
    fn new() -> Self {
        Self { prev_cpu: Mutex::new(None) }
    }

    /// Collect a full system snapshot.
    ///
    /// The first call returns `cpu_usage_percent = 0.0` because there is no
    /// previous baseline. Subsequent calls compute the real delta.
    pub fn snapshot(&self) -> SystemSnapshot {
        // CPU
        let cpu_pct = {
            let curr = proc_cpu::read_cpu_jiffies();
            let mut prev_guard = self.prev_cpu.lock().unwrap_or_else(|e| e.into_inner());
            let pct = match (&*prev_guard, &curr) {
                (Some(prev), Some(curr)) => proc_cpu::cpu_usage_percent(prev, curr),
                _ => 0.0,
            };
            *prev_guard = curr;
            pct
        };

        // Memory
        let mem = proc_mem::read_meminfo();
        let (ram_total, ram_available) =
            mem.as_ref().map(|m| (m.total_bytes, m.available_bytes)).unwrap_or((0, 0));

        // Load average
        let load = proc_load::read_loadavg();
        let (l1, l5, l15) = load
            .as_ref()
            .map(|l| (l.load_1, l.load_5, l.load_15))
            .unwrap_or((0.0, 0.0, 0.0));

        // Process RSS
        let rss = proc_self::read_process_rss().unwrap_or(0);

        SystemSnapshot {
            cpu_usage_percent: cpu_pct,
            ram_total_bytes: ram_total,
            ram_used_bytes: ram_total.saturating_sub(ram_available),
            ram_available_bytes: ram_available,
            load_avg_1: l1,
            load_avg_5: l5,
            load_avg_15: l15,
            process_rss_bytes: rss,
            timestamp: chrono::Utc::now().to_rfc3339(),
        }
    }
}

// ---------------------------------------------------------------------------
// Global singleton (same pattern as AccessLogBuffer)
// ---------------------------------------------------------------------------

static SYSTEM_METRICS: OnceLock<SystemMetrics> = OnceLock::new();

/// Initialize the global system metrics collector. Call once from `serve()`.
pub fn init_system_metrics() {
    let _ = SYSTEM_METRICS.set(SystemMetrics::new());
}

/// Get a reference to the global collector (`None` if not initialized).
pub fn system_metrics() -> Option<&'static SystemMetrics> {
    SYSTEM_METRICS.get()
}

/// Escape a string for use as a Prometheus label value.
///
/// Backslashes, double quotes, and newlines are escaped per the Prometheus
/// text exposition format specification.
fn escape_prometheus_label(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            c => out.push(c),
        }
    }
    out
}

/// Format system + request metrics as Prometheus text exposition.
pub fn prometheus_metrics(model_name: &str) -> String {
    let model_name = escape_prometheus_label(model_name);
    let snap = SYSTEM_METRICS.get().map(|m| m.snapshot());
    let req = compute_request_stats(60);

    let mut out = String::with_capacity(1024);

    // System metrics
    out.push_str("# HELP lithair_cpu_usage_percent CPU usage percentage\n");
    out.push_str("# TYPE lithair_cpu_usage_percent gauge\n");
    out.push_str(&format!(
        "lithair_cpu_usage_percent{{model=\"{}\"}} {:.2}\n",
        model_name,
        snap.as_ref().map(|s| s.cpu_usage_percent).unwrap_or(0.0)
    ));

    out.push_str("# HELP lithair_memory_used_bytes Memory used in bytes\n");
    out.push_str("# TYPE lithair_memory_used_bytes gauge\n");
    out.push_str(&format!(
        "lithair_memory_used_bytes{{model=\"{}\"}} {}\n",
        model_name,
        snap.as_ref().map(|s| s.ram_used_bytes).unwrap_or(0)
    ));

    out.push_str("# HELP lithair_memory_total_bytes Total memory in bytes\n");
    out.push_str("# TYPE lithair_memory_total_bytes gauge\n");
    out.push_str(&format!(
        "lithair_memory_total_bytes{{model=\"{}\"}} {}\n",
        model_name,
        snap.as_ref().map(|s| s.ram_total_bytes).unwrap_or(0)
    ));

    out.push_str("# HELP lithair_memory_available_bytes Available memory in bytes\n");
    out.push_str("# TYPE lithair_memory_available_bytes gauge\n");
    out.push_str(&format!(
        "lithair_memory_available_bytes{{model=\"{}\"}} {}\n",
        model_name,
        snap.as_ref().map(|s| s.ram_available_bytes).unwrap_or(0)
    ));

    out.push_str("# HELP lithair_load_avg_1 Load average 1 minute\n");
    out.push_str("# TYPE lithair_load_avg_1 gauge\n");
    out.push_str(&format!(
        "lithair_load_avg_1{{model=\"{}\"}} {:.2}\n",
        model_name,
        snap.as_ref().map(|s| s.load_avg_1).unwrap_or(0.0)
    ));

    out.push_str("# HELP lithair_process_rss_bytes Process resident set size in bytes\n");
    out.push_str("# TYPE lithair_process_rss_bytes gauge\n");
    out.push_str(&format!(
        "lithair_process_rss_bytes{{model=\"{}\"}} {}\n",
        model_name,
        snap.as_ref().map(|s| s.process_rss_bytes).unwrap_or(0)
    ));

    // Request metrics
    out.push_str("# HELP lithair_requests_per_second Requests per second (60s window)\n");
    out.push_str("# TYPE lithair_requests_per_second gauge\n");
    out.push_str(&format!(
        "lithair_requests_per_second{{model=\"{}\"}} {:.2}\n",
        model_name,
        req.as_ref().map(|r| r.rps).unwrap_or(0.0)
    ));

    out.push_str("# HELP lithair_latency_p50_ms 50th percentile latency in ms\n");
    out.push_str("# TYPE lithair_latency_p50_ms gauge\n");
    out.push_str(&format!(
        "lithair_latency_p50_ms{{model=\"{}\"}} {:.2}\n",
        model_name,
        req.as_ref().map(|r| r.latency_p50_ms).unwrap_or(0.0)
    ));

    out.push_str("# HELP lithair_latency_p95_ms 95th percentile latency in ms\n");
    out.push_str("# TYPE lithair_latency_p95_ms gauge\n");
    out.push_str(&format!(
        "lithair_latency_p95_ms{{model=\"{}\"}} {:.2}\n",
        model_name,
        req.as_ref().map(|r| r.latency_p95_ms).unwrap_or(0.0)
    ));

    out.push_str("# HELP lithair_latency_p99_ms 99th percentile latency in ms\n");
    out.push_str("# TYPE lithair_latency_p99_ms gauge\n");
    out.push_str(&format!(
        "lithair_latency_p99_ms{{model=\"{}\"}} {:.2}\n",
        model_name,
        req.as_ref().map(|r| r.latency_p99_ms).unwrap_or(0.0)
    ));

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_returns_zero_on_first_call() {
        let m = SystemMetrics::new();
        let s = m.snapshot();
        assert_eq!(s.cpu_usage_percent, 0.0);
        assert!(!s.timestamp.is_empty());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn snapshot_reads_real_proc() {
        let m = SystemMetrics::new();
        // First call: baseline
        let _ = m.snapshot();
        // Small delay so CPU jiffies advance
        std::thread::sleep(std::time::Duration::from_millis(50));
        let s = m.snapshot();
        // On a real Linux box RAM total should be > 0
        assert!(s.ram_total_bytes > 0, "ram_total should be > 0");
        assert!(s.ram_available_bytes > 0, "ram_available should be > 0");
        assert!(s.process_rss_bytes > 0, "rss should be > 0");
    }

    #[test]
    fn prometheus_output_contains_expected_metrics() {
        init_system_metrics();
        let output = prometheus_metrics("test_model");
        assert!(output.contains("lithair_cpu_usage_percent"));
        assert!(output.contains("lithair_memory_used_bytes"));
        assert!(output.contains("lithair_memory_total_bytes"));
        assert!(output.contains("lithair_memory_available_bytes"));
        assert!(output.contains("lithair_load_avg_1"));
        assert!(output.contains("lithair_process_rss_bytes"));
        assert!(output.contains("lithair_requests_per_second"));
        assert!(output.contains("lithair_latency_p50_ms"));
        assert!(output.contains("lithair_latency_p95_ms"));
        assert!(output.contains("lithair_latency_p99_ms"));
        assert!(output.contains("test_model"));
    }
}
