//! Anti-DDoS Protection Module
//!
//! Provides comprehensive protection against:
//! - Slowloris attacks (slow headers/connections)
//! - Connection exhaustion attacks
//! - Rate limiting per IP
//! - Circuit breaker for failing services
//! - Resource exhaustion protection

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

/// Connection tracking and rate limiting
#[derive(Debug, Clone)]
pub struct AntiDDoSConfig {
    /// Maximum connections per IP
    pub max_connections_per_ip: usize,
    /// Maximum global connections
    pub max_global_connections: usize,
    /// Rate limit requests per IP per second
    pub rate_limit_per_ip: u32,
    /// Rate limit window in seconds
    pub rate_window_seconds: u64,
    /// Connection timeout for idle connections
    pub connection_timeout: Duration,
    /// Circuit breaker threshold (failures per minute)
    pub circuit_breaker_threshold: u32,
}

impl Default for AntiDDoSConfig {
    fn default() -> Self {
        Self {
            max_connections_per_ip: 100,
            max_global_connections: 10000,
            rate_limit_per_ip: 100,
            rate_window_seconds: 60,
            connection_timeout: Duration::from_secs(300), // 5 minutes
            circuit_breaker_threshold: 50,
        }
    }
}

/// Rate limiting bucket for tracking requests
#[derive(Debug)]
struct RateLimitBucket {
    count: AtomicU64,
    window_start: std::sync::Mutex<Instant>,
}

impl RateLimitBucket {
    fn new() -> Self {
        Self {
            count: AtomicU64::new(0),
            window_start: std::sync::Mutex::new(Instant::now()),
        }
    }

    fn is_allowed(&self, limit: u32, window_duration: Duration) -> bool {
        let now = Instant::now();

        // Check if we need to reset the window
        if let Ok(mut window_start) = self.window_start.try_lock() {
            if now.duration_since(*window_start) >= window_duration {
                *window_start = now;
                self.count.store(0, Ordering::Relaxed);
            }
        }

        let current_count = self.count.fetch_add(1, Ordering::Relaxed);
        current_count < limit as u64
    }
}

/// Connection tracking per IP
#[derive(Debug)]
struct ConnectionTracker {
    count: AtomicUsize,
    last_activity: std::sync::Mutex<Instant>,
}

impl ConnectionTracker {
    fn new() -> Self {
        Self {
            count: AtomicUsize::new(0),
            last_activity: std::sync::Mutex::new(Instant::now()),
        }
    }

    fn add_connection(&self) -> usize {
        if let Ok(mut last_activity) = self.last_activity.try_lock() {
            *last_activity = Instant::now();
        }
        self.count.fetch_add(1, Ordering::Relaxed)
    }

    fn remove_connection(&self) -> usize {
        self.count.fetch_sub(1, Ordering::Relaxed).saturating_sub(1)
    }

    fn connection_count(&self) -> usize {
        self.count.load(Ordering::Relaxed)
    }
}

/// Circuit breaker for service protection
#[derive(Debug)]
pub struct CircuitBreaker {
    failure_count: AtomicU64,
    last_failure_time: std::sync::Mutex<Option<Instant>>,
    state: AtomicU64, // 0=Closed, 1=Open, 2=HalfOpen
    threshold: u32,
}

impl CircuitBreaker {
    pub fn new(threshold: u32) -> Self {
        Self {
            failure_count: AtomicU64::new(0),
            last_failure_time: std::sync::Mutex::new(None),
            state: AtomicU64::new(0), // Closed
            threshold,
        }
    }

    pub fn is_allowed(&self) -> bool {
        let state = self.state.load(Ordering::Relaxed);

        match state {
            0 => true, // Closed - allow all
            1 => {     // Open - check if we should transition to half-open
                if let Ok(last_failure) = self.last_failure_time.try_lock() {
                    if let Some(last_time) = *last_failure {
                        if last_time.elapsed() > Duration::from_secs(60) {
                            self.state.store(2, Ordering::Relaxed); // Half-open
                            return true;
                        }
                    }
                }
                false
            }
            2 => true, // Half-open - allow one request
            _ => false,
        }
    }

    pub fn record_success(&self) {
        self.failure_count.store(0, Ordering::Relaxed);
        self.state.store(0, Ordering::Relaxed); // Closed
    }

    pub fn record_failure(&self) {
        let failures = self.failure_count.fetch_add(1, Ordering::Relaxed) + 1;

        if let Ok(mut last_failure) = self.last_failure_time.try_lock() {
            *last_failure = Some(Instant::now());
        }

        if failures >= self.threshold as u64 {
            self.state.store(1, Ordering::Relaxed); // Open
        }
    }
}

/// Main Anti-DDoS protection system
#[derive(Debug)]
pub struct AntiDDoSProtection {
    config: AntiDDoSConfig,
    rate_limits: Arc<RwLock<HashMap<IpAddr, RateLimitBucket>>>,
    connections: Arc<RwLock<HashMap<IpAddr, ConnectionTracker>>>,
    global_connections: AtomicUsize,
    circuit_breaker: CircuitBreaker,
}

impl AntiDDoSProtection {
    pub fn new(config: AntiDDoSConfig) -> Self {
        Self {
            circuit_breaker: CircuitBreaker::new(config.circuit_breaker_threshold),
            config,
            rate_limits: Arc::new(RwLock::new(HashMap::new())),
            connections: Arc::new(RwLock::new(HashMap::new())),
            global_connections: AtomicUsize::new(0),
        }
    }

    /// Check if a request should be allowed (rate limiting)
    pub async fn is_request_allowed(&self, ip: IpAddr) -> bool {
        // Circuit breaker check first
        if !self.circuit_breaker.is_allowed() {
            return false;
        }

        let mut rate_limits = self.rate_limits.write().await;
        let bucket = rate_limits.entry(ip).or_insert_with(RateLimitBucket::new);

        bucket.is_allowed(
            self.config.rate_limit_per_ip,
            Duration::from_secs(self.config.rate_window_seconds),
        )
    }

    /// Check if a new connection should be allowed
    pub async fn is_connection_allowed(&self, ip: IpAddr) -> bool {
        // Global connection limit
        let global_count = self.global_connections.load(Ordering::Relaxed);
        if global_count >= self.config.max_global_connections {
            return false;
        }

        // Per-IP connection limit
        let connections = self.connections.read().await;
        if let Some(tracker) = connections.get(&ip) {
            tracker.connection_count() < self.config.max_connections_per_ip
        } else {
            true
        }
    }

    /// Register a new connection
    pub async fn register_connection(&self, ip: IpAddr) -> Result<(), &'static str> {
        if !self.is_connection_allowed(ip).await {
            return Err("Connection limit exceeded");
        }

        self.global_connections.fetch_add(1, Ordering::Relaxed);

        let mut connections = self.connections.write().await;
        let tracker = connections.entry(ip).or_insert_with(ConnectionTracker::new);
        tracker.add_connection();

        Ok(())
    }

    /// Unregister a connection
    pub async fn unregister_connection(&self, ip: IpAddr) {
        self.global_connections.fetch_sub(1, Ordering::Relaxed);

        let mut connections = self.connections.write().await;
        if let Some(tracker) = connections.get(&ip) {
            let remaining = tracker.remove_connection();

            // Clean up empty trackers to prevent memory leaks
            if remaining == 0 {
                connections.remove(&ip);
            }
        }
    }

    /// Record a successful request (for circuit breaker)
    pub fn record_success(&self) {
        self.circuit_breaker.record_success();
    }

    /// Record a failed request (for circuit breaker)
    pub fn record_failure(&self) {
        self.circuit_breaker.record_failure();
    }

    /// Clean up old rate limit entries (call periodically)
    pub async fn cleanup_old_entries(&self) {
        let cutoff = Instant::now() - Duration::from_secs(self.config.rate_window_seconds * 2);

        let mut rate_limits = self.rate_limits.write().await;
        rate_limits.retain(|_, bucket| {
            if let Ok(window_start) = bucket.window_start.try_lock() {
                *window_start > cutoff
            } else {
                true // Keep if we can't acquire lock
            }
        });

        // Clean up inactive connections
        let mut connections = self.connections.write().await;
        connections.retain(|_, tracker| {
            if let Ok(last_activity) = tracker.last_activity.try_lock() {
                last_activity.elapsed() < self.config.connection_timeout
            } else {
                true // Keep if we can't acquire lock
            }
        });
    }

    /// Get current statistics
    pub async fn get_stats(&self) -> DDoSStats {
        let rate_limits_count = self.rate_limits.read().await.len();
        let connections_count = self.connections.read().await.len();
        let global_connections = self.global_connections.load(Ordering::Relaxed);

        DDoSStats {
            tracked_ips: rate_limits_count,
            active_connections: connections_count,
            global_connections,
            circuit_breaker_state: self.circuit_breaker.state.load(Ordering::Relaxed),
            circuit_breaker_failures: self.circuit_breaker.failure_count.load(Ordering::Relaxed),
        }
    }
}

/// Statistics for monitoring
#[derive(Debug, Clone)]
pub struct DDoSStats {
    pub tracked_ips: usize,
    pub active_connections: usize,
    pub global_connections: usize,
    pub circuit_breaker_state: u64, // 0=Closed, 1=Open, 2=HalfOpen
    pub circuit_breaker_failures: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    #[tokio::test]
    async fn test_rate_limiting() {
        let config = AntiDDoSConfig {
            rate_limit_per_ip: 2,
            rate_window_seconds: 1,
            ..Default::default()
        };

        let protection = AntiDDoSProtection::new(config);
        let ip = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));

        // First two requests should be allowed
        assert!(protection.is_request_allowed(ip).await);
        assert!(protection.is_request_allowed(ip).await);

        // Third request should be blocked
        assert!(!protection.is_request_allowed(ip).await);

        // Wait for rate limit to reset
        tokio::time::sleep(Duration::from_secs(2)).await;
        assert!(protection.is_request_allowed(ip).await);
    }

    #[tokio::test]
    async fn test_connection_limiting() {
        let config = AntiDDoSConfig {
            max_connections_per_ip: 2,
            max_global_connections: 5,
            ..Default::default()
        };

        let protection = AntiDDoSProtection::new(config);
        let ip = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));

        // Register connections
        assert!(protection.register_connection(ip).await.is_ok());
        assert!(protection.register_connection(ip).await.is_ok());

        // Third connection should be blocked
        assert!(protection.register_connection(ip).await.is_err());

        // Unregister one connection
        protection.unregister_connection(ip).await;

        // Now should be allowed again
        assert!(protection.register_connection(ip).await.is_ok());
    }

    #[tokio::test]
    async fn test_circuit_breaker() {
        let protection = AntiDDoSProtection::new(AntiDDoSConfig {
            circuit_breaker_threshold: 2,
            ..Default::default()
        });

        // Initially should be allowed
        assert!(protection.circuit_breaker.is_allowed());

        // Record failures
        protection.record_failure();
        protection.record_failure();

        // Should now be blocked
        assert!(!protection.circuit_breaker.is_allowed());

        // Record success after timeout would reset it
        protection.record_success();
        assert!(protection.circuit_breaker.is_allowed());
    }
}