#pragma once
/// Strategy module — dummy consumer that logs best bid/ask.
/// Receives BookNotification via SPSC queue, measures latency.

#include <cstdio>
#include <vector>
#include <algorithm>
#include <atomic>
#include "types.h"
#include "spsc_queue.h"
#include "clock.h"

struct StrategyStats {
    uint64_t count = 0;
    uint64_t total_latency_ns = 0;
    uint64_t min_latency_ns = UINT64_MAX;
    uint64_t max_latency_ns = 0;
    std::vector<uint64_t> latencies;

    StrategyStats() { latencies.reserve(8192); }

    void record(uint64_t lat) {
        ++count;
        total_latency_ns += lat;
        if (lat < min_latency_ns) min_latency_ns = lat;
        if (lat > max_latency_ns) max_latency_ns = lat;
        latencies.push_back(lat);
    }

    uint64_t avg_ns() const { return count > 0 ? total_latency_ns / count : 0; }

    uint64_t percentile(double p) const {
        if (latencies.empty()) return 0;
        auto sorted = latencies;
        std::sort(sorted.begin(), sorted.end());
        size_t idx = static_cast<size_t>((p / 100.0) * (sorted.size() - 1));
        return sorted[std::min(idx, sorted.size() - 1)];
    }

    uint64_t median() const { return percentile(50.0); }
};

/// Run strategy consumer. Blocks until closed flag is set and queue is drained.
template <size_t QueueCap>
StrategyStats run_strategy(
    SPSCQueue<BookNotification, QueueCap>& queue,
    std::atomic<bool>& closed,
    bool log_enabled)
{
    StrategyStats stats;

    while (true) {
        auto maybe = queue.pop(closed);
        if (!maybe.has_value()) break;

        const auto& notif = *maybe;
        uint64_t recv_ns = Clock::now_ns();
        uint64_t latency_ns = recv_ns - notif.engine_send_ns;
        stats.record(latency_ns);

        if (log_enabled) {
            char bid_buf[64] = "EMPTY";
            char ask_buf[64] = "EMPTY";
            if (notif.best_bid.has_value()) {
                snprintf(bid_buf, sizeof(bid_buf), "%.2f @ %.4f",
                    notif.best_bid->price.to_f64(), notif.best_bid->qty.value);
            }
            if (notif.best_ask.has_value()) {
                snprintf(ask_buf, sizeof(ask_buf), "%.2f @ %.4f",
                    notif.best_ask->price.to_f64(), notif.best_ask->qty.value);
            }
            printf("[strategy] seq=%-6lu ts=%lu | best_bid: %-22s | best_ask: %-22s | lat=%luns\n",
                notif.seq, notif.update_timestamp, bid_buf, ask_buf, latency_ns);
        }
    }

    return stats;
}
