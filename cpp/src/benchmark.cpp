/// Dedicated benchmark binary for the C++ orderbook system.
/// Mirrors the Rust benchmark exactly for fair comparison.

#include <cstdio>
#include <thread>
#include <atomic>
#include <vector>
#include <algorithm>
#include <memory>

#include "types.h"
#include "orderbook.h"
#include "parser.h"
#include "spsc_queue.h"
#include "strategy.h"
#include "clock.h"

static constexpr size_t QUEUE_CAPACITY = 4096;
static constexpr int WARMUP_ITERATIONS = 5;
static constexpr int BENCH_ITERATIONS = 20;

// Prevent dead code elimination
template <typename T>
static void do_not_optimize(const T& val) {
    asm volatile("" : : "r,m"(val) : "memory");
}

int main(int argc, char* argv[]) {
    const char* csv_path = (argc > 1) ? argv[1] : "btc_orderbook_updates.csv";

    printf("╔══════════════════════════════════════════════════════╗\n");
    printf("║    ORDERBOOK SYSTEM (C++) — BENCHMARK SUITE         ║\n");
    printf("╚══════════════════════════════════════════════════════╝\n\n");

    // ── Benchmark 1: CSV Parsing ──
    printf("── Benchmark 1: CSV Parsing ──────────────────────────\n");

    // Warmup
    std::vector<Update> updates;
    for (int i = 0; i < WARMUP_ITERATIONS; ++i) {
        updates = CsvReader::parse_file(csv_path);
    }

    std::vector<uint64_t> parse_times;
    parse_times.reserve(BENCH_ITERATIONS);
    for (int i = 0; i < BENCH_ITERATIONS; ++i) {
        uint64_t start = Clock::now_ns();
        updates = CsvReader::parse_file(csv_path);
        uint64_t end = Clock::now_ns();
        parse_times.push_back(end - start);
    }

    uint64_t avg_parse = 0;
    for (auto t : parse_times) avg_parse += t;
    avg_parse /= parse_times.size();
    uint64_t min_parse = *std::min_element(parse_times.begin(), parse_times.end());
    double parse_tp = (updates.size() / static_cast<double>(min_parse)) * 1e9;

    printf("  Updates parsed:    %zu\n", updates.size());
    printf("  Avg parse time:    %.2f us\n", avg_parse / 1000.0);
    printf("  Min parse time:    %.2f us\n", min_parse / 1000.0);
    printf("  Parse throughput:  %.0f updates/sec (best run)\n\n", parse_tp);

    // ── Benchmark 2: Orderbook Engine (isolated) ──
    printf("── Benchmark 2: Orderbook Engine (isolated) ──────────\n");

    // Warmup
    for (int i = 0; i < WARMUP_ITERATIONS; ++i) {
        Orderbook book;
        for (const auto& u : updates) book.apply(u, 0);
    }

    std::vector<uint64_t> engine_times;
    engine_times.reserve(BENCH_ITERATIONS);
    for (int i = 0; i < BENCH_ITERATIONS; ++i) {
        Orderbook book;
        uint64_t start = Clock::now_ns();
        for (const auto& u : updates) {
            book.apply(u, 0);
        }
        uint64_t end = Clock::now_ns();
        engine_times.push_back(end - start);
        do_not_optimize(book.best_bid());
    }

    uint64_t avg_engine = 0;
    for (auto t : engine_times) avg_engine += t;
    avg_engine /= engine_times.size();
    uint64_t min_engine = *std::min_element(engine_times.begin(), engine_times.end());
    double per_update = static_cast<double>(min_engine) / updates.size();
    double engine_tp = (updates.size() / static_cast<double>(min_engine)) * 1e9;

    printf("  Updates:           %zu\n", updates.size());
    printf("  Avg engine time:   %.2f us\n", avg_engine / 1000.0);
    printf("  Min engine time:   %.2f us\n", min_engine / 1000.0);
    printf("  Per-update:        %.0f ns\n", per_update);
    printf("  Engine throughput: %.0f updates/sec (best run)\n\n", engine_tp);

    // ── Benchmark 3: End-to-End ──
    printf("── Benchmark 3: End-to-End (engine + channel + strategy) ──\n");

    std::vector<uint64_t> e2e_times;
    e2e_times.reserve(BENCH_ITERATIONS);
    StrategyStats last_stats;

    for (int i = 0; i < BENCH_ITERATIONS; ++i) {
        auto queue = std::make_unique<SPSCQueue<BookNotification, QUEUE_CAPACITY>>();
        std::atomic<bool> closed{false};
        StrategyStats stats;

        auto* qp = queue.get();
        std::thread strat([qp, &closed, &stats]() {
            stats = run_strategy(*qp, closed, false);
        });

        Orderbook book;
        uint64_t start = Clock::now_ns();
        for (const auto& u : updates) {
            uint64_t now = Clock::now_ns();
            auto notif = book.apply(u, now);
            qp->push(notif);
        }
        closed.store(true, std::memory_order_release);
        strat.join();
        uint64_t end = Clock::now_ns();
        e2e_times.push_back(end - start);
        last_stats = stats;
    }

    uint64_t avg_e2e = 0;
    for (auto t : e2e_times) avg_e2e += t;
    avg_e2e /= e2e_times.size();
    uint64_t min_e2e = *std::min_element(e2e_times.begin(), e2e_times.end());
    double e2e_tp = (updates.size() / static_cast<double>(min_e2e)) * 1e9;

    printf("  Avg e2e time:      %.2f us\n", avg_e2e / 1000.0);
    printf("  Min e2e time:      %.2f us\n", min_e2e / 1000.0);
    printf("  E2E throughput:    %.0f updates/sec (best run)\n\n", e2e_tp);

    // ── Benchmark 4: Latency ──
    printf("── Benchmark 4: Engine -> Strategy Latency ────────────\n");
    printf("  Samples:           %lu\n", last_stats.count);
    printf("  Min latency:       %lu ns\n", last_stats.min_latency_ns);
    printf("  Max latency:       %lu ns\n", last_stats.max_latency_ns);
    printf("  Avg latency:       %lu ns\n", last_stats.avg_ns());
    printf("  Median (P50):      %lu ns\n", last_stats.median());
    printf("  P90 latency:       %lu ns\n", last_stats.percentile(90.0));
    printf("  P95 latency:       %lu ns\n", last_stats.percentile(95.0));
    printf("  P99 latency:       %lu ns\n", last_stats.percentile(99.0));
    printf("  P99.9 latency:     %lu ns\n", last_stats.percentile(99.9));

    printf("\n╔══════════════════════════════════════════════════════╗\n");
    printf("║                   SUMMARY                           ║\n");
    printf("╠══════════════════════════════════════════════════════╣\n");
    printf("║  CSV parse throughput: %12.0f updates/sec     ║\n", parse_tp);
    printf("║  Engine throughput:    %12.0f updates/sec     ║\n", engine_tp);
    printf("║  E2E throughput:       %12.0f updates/sec     ║\n", e2e_tp);
    printf("║  Per-update latency:   %9.0f ns               ║\n", per_update);
    printf("║  Median chan latency:  %9lu ns               ║\n", last_stats.median());
    printf("║  P99 chan latency:     %9lu ns               ║\n", last_stats.percentile(99.0));
    printf("╚══════════════════════════════════════════════════════╝\n");

    return 0;
}
