/// Ultra-low-latency orderbook system — main entry point (C++ version).
/// Architecture mirrors the Rust version exactly:
///   [mmap CSV reader] → parse_file() → vector<Update>
///        ↓
///   [Engine thread] — applies to Orderbook, sends notification
///        ↓ (lock-free SPSC queue, 4096 slots)
///   [Strategy thread] — receives, logs best bid/ask, measures latency

#include <cstdio>
#include <thread>
#include <atomic>
#include <string>

#include "types.h"
#include "orderbook.h"
#include "parser.h"
#include "spsc_queue.h"
#include "strategy.h"
#include "clock.h"

static constexpr size_t QUEUE_CAPACITY = 4096;

int main(int argc, char* argv[]) {
    const char* csv_path = (argc > 1) ? argv[1] : "btc_orderbook_updates.csv";

    printf("=== Orderbook System (C++) ===\n");
    printf("Loading CSV: %s\n", csv_path);

    // Phase 1: Parse CSV (mmap, fast)
    auto updates = CsvReader::parse_file(csv_path);
    printf("Parsed %zu updates from CSV\n", updates.size());

    if (updates.empty()) {
        fprintf(stderr, "No updates found. Exiting.\n");
        return 1;
    }

    // Phase 2: Set up queue and closed flag
    auto queue = std::make_unique<SPSCQueue<BookNotification, QUEUE_CAPACITY>>();
    std::atomic<bool> closed{false};

    // Phase 3: Spawn strategy consumer thread
    StrategyStats stats;
    auto* queue_ptr = queue.get();
    std::thread strategy_thread([queue_ptr, &closed, &stats]() {
        stats = run_strategy(*queue_ptr, closed, true);
    });

    // Phase 4: Engine — apply updates and send notifications
    Orderbook book;
    uint64_t start = Clock::now_ns();

    for (const auto& update : updates) {
        uint64_t now = Clock::now_ns();
        auto notif = book.apply(update, now);
        queue_ptr->push(notif);
    }

    uint64_t end_ns = Clock::now_ns();
    uint64_t elapsed_ns = end_ns - start;

    // Signal done and wait
    closed.store(true, std::memory_order_release);
    strategy_thread.join();

    // Phase 5: Print summary
    double elapsed_us = elapsed_ns / 1000.0;
    double elapsed_ms = elapsed_ns / 1'000'000.0;
    double throughput = (elapsed_ns > 0)
        ? (updates.size() / static_cast<double>(elapsed_ns)) * 1'000'000'000.0
        : 0.0;

    printf("\n=== Engine Summary ===\n");
    printf("Total updates:     %zu\n", updates.size());
    printf("Engine time:       %.2f ms (%.2f us)\n", elapsed_ms, elapsed_us);
    printf("Throughput:        %.0f updates/sec\n", throughput);
    printf("Final book depth:  %zu bids, %zu asks\n", book.bid_depth(), book.ask_depth());
    if (auto bb = book.best_bid()) {
        printf("Final best bid:    %.2f @ %.4f\n", bb->price.to_f64(), bb->qty.value);
    }
    if (auto ba = book.best_ask()) {
        printf("Final best ask:    %.2f @ %.4f\n", ba->price.to_f64(), ba->qty.value);
    }

    printf("\n=== Strategy Latency (engine->strategy) ===\n");
    printf("Updates received:  %lu\n", stats.count);
    printf("Min latency:       %lu ns\n", stats.min_latency_ns);
    printf("Max latency:       %lu ns\n", stats.max_latency_ns);
    printf("Avg latency:       %lu ns\n", stats.avg_ns());
    printf("Median latency:    %lu ns\n", stats.median());
    printf("P99 latency:       %lu ns\n", stats.percentile(99.0));
    printf("P99.9 latency:     %lu ns\n", stats.percentile(99.9));

    return 0;
}
