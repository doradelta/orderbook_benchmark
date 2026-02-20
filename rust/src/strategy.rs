/// Strategy module — a dummy consumer that subscribes to orderbook updates.
///
/// Design: Receives `BookNotification` via a crossbeam bounded channel
/// (lock-free SPSC in practice). Logs best bid/ask to stdout on every update.
/// Also tracks latency from engine → strategy for benchmarking.

use crossbeam_channel::Receiver;
use crate::types::*;

/// Run the strategy consumer loop. Blocks until the channel is closed.
///
/// # Arguments
/// * `rx` — Receiver end of the notification channel.
/// * `clock` — quanta::Clock for high-resolution timing.
/// * `log_enabled` — Whether to log each update to stdout (disable for benchmarks).
///
/// # Returns
/// A `StrategyStats` with latency measurements.
pub fn run_strategy(
    rx: Receiver<BookNotification>,
    clock: &quanta::Clock,
    log_enabled: bool,
) -> StrategyStats {
    let mut stats = StrategyStats::new();

    while let Ok(notif) = rx.recv() {
        let recv_ns = clock.raw();
        let recv_ns_calibrated = clock.delta_as_nanos(0, recv_ns);

        // Compute engine → strategy latency in nanoseconds
        let latency_ns = recv_ns_calibrated.saturating_sub(notif.engine_send_ns);
        stats.record(latency_ns);

        if log_enabled {
            let bid_str = match notif.best_bid {
                Some(level) => format!("{:.2} @ {:.4}", level.price.to_f64(), level.qty.0),
                None => "EMPTY".to_string(),
            };
            let ask_str = match notif.best_ask {
                Some(level) => format!("{:.2} @ {:.4}", level.price.to_f64(), level.qty.0),
                None => "EMPTY".to_string(),
            };
            println!(
                "[strategy] seq={:<6} ts={} | best_bid: {:<22} | best_ask: {:<22} | lat={}ns",
                notif.seq, notif.update_timestamp, bid_str, ask_str, latency_ns
            );
        }
    }

    stats
}

/// Statistics collected by the strategy for benchmarking.
pub struct StrategyStats {
    pub count: u64,
    pub total_latency_ns: u64,
    pub min_latency_ns: u64,
    pub max_latency_ns: u64,
    /// For percentile calculation — store all latencies when benchmarking.
    pub latencies: Vec<u64>,
}

impl StrategyStats {
    pub fn new() -> Self {
        Self {
            count: 0,
            total_latency_ns: 0,
            min_latency_ns: u64::MAX,
            max_latency_ns: 0,
            latencies: Vec::with_capacity(8192),
        }
    }

    #[inline(always)]
    pub fn record(&mut self, latency_ns: u64) {
        self.count += 1;
        self.total_latency_ns += latency_ns;
        if latency_ns < self.min_latency_ns {
            self.min_latency_ns = latency_ns;
        }
        if latency_ns > self.max_latency_ns {
            self.max_latency_ns = latency_ns;
        }
        self.latencies.push(latency_ns);
    }

    pub fn avg_latency_ns(&self) -> u64 {
        if self.count == 0 {
            return 0;
        }
        self.total_latency_ns / self.count
    }

    pub fn percentile(&self, p: f64) -> u64 {
        if self.latencies.is_empty() {
            return 0;
        }
        let mut sorted = self.latencies.clone();
        sorted.sort_unstable();
        let idx = ((p / 100.0) * (sorted.len() as f64 - 1.0)) as usize;
        sorted[idx.min(sorted.len() - 1)]
    }

    pub fn median(&self) -> u64 {
        self.percentile(50.0)
    }
}
