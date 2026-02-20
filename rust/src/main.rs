/// Ultra-low-latency orderbook system — main entry point.
///
/// Architecture:
///   [CSV mmap reader] → parse_all() → Vec<Update>
///        ↓
///   [Engine thread] — iterates updates, applies to Orderbook, sends notification
///        ↓ (crossbeam bounded channel, capacity 4096)
///   [Strategy thread] — receives notifications, logs best bid/ask, measures latency
///
/// The bounded channel acts as backpressure. Channel capacity is a power of two
/// for optimal cache-line alignment in crossbeam's internal ring buffer.

mod types;
mod orderbook;
mod parser;
mod strategy;

use std::thread;
use crossbeam_channel::bounded;
use crate::orderbook::Orderbook;
use crate::parser::CsvReader;
use crate::strategy::run_strategy;
use crate::types::BookNotification;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let csv_path = if args.len() > 1 {
        args[1].clone()
    } else {
        "btc_orderbook_updates.csv".to_string()
    };

    println!("=== Orderbook System ===");
    println!("Loading CSV: {}", csv_path);

    // Phase 1: Parse CSV (memory-mapped, fast)
    let reader = CsvReader::open(&csv_path).expect("Failed to open CSV file");
    let updates = reader.parse_all();
    println!("Parsed {} updates from CSV", updates.len());

    if updates.is_empty() {
        eprintln!("No updates found in CSV. Exiting.");
        return;
    }

    // Phase 2: Set up clock and channel
    let clock = quanta::Clock::new();

    // Bounded channel — power-of-two capacity for cache alignment.
    // 4096 slots provides enough buffer without excessive memory.
    let (tx, rx) = bounded::<BookNotification>(4096);

    // Phase 3: Spawn strategy consumer thread
    let strategy_clock = clock.clone();
    let strategy_handle = thread::Builder::new()
        .name("strategy".to_string())
        .spawn(move || {
            // Pin to a core if possible (best-effort, non-critical)
            run_strategy(rx, &strategy_clock, true)
        })
        .expect("Failed to spawn strategy thread");

    // Phase 4: Engine — apply updates and send notifications
    let mut book = Orderbook::new();
    let start = clock.raw();

    for update in &updates {
        let now_ns = clock.delta_as_nanos(0, clock.raw());
        let notif = book.apply(update, now_ns);

        // Send to strategy. If strategy is too slow, this will block (backpressure).
        if tx.send(notif).is_err() {
            eprintln!("Strategy disconnected, stopping engine.");
            break;
        }
    }

    let end = clock.raw();
    let elapsed_ns = clock.delta_as_nanos(start, end);

    // Drop sender to signal strategy to stop
    drop(tx);

    // Phase 5: Wait for strategy to finish and collect stats
    let stats = strategy_handle.join().expect("Strategy thread panicked");

    // Phase 6: Print summary
    let elapsed_us = elapsed_ns as f64 / 1_000.0;
    let elapsed_ms = elapsed_ns as f64 / 1_000_000.0;
    let throughput = if elapsed_ns > 0 {
        (updates.len() as f64 / elapsed_ns as f64) * 1_000_000_000.0
    } else {
        0.0
    };

    println!("\n=== Engine Summary ===");
    println!("Total updates:     {}", updates.len());
    println!("Engine time:       {:.2} ms ({:.2} µs)", elapsed_ms, elapsed_us);
    println!("Throughput:        {:.0} updates/sec", throughput);
    println!("Final book depth:  {} bids, {} asks", book.bid_depth(), book.ask_depth());
    if let Some(bb) = book.best_bid() {
        println!("Final best bid:    {:.2} @ {:.4}", bb.price.to_f64(), bb.qty.0);
    }
    if let Some(ba) = book.best_ask() {
        println!("Final best ask:    {:.2} @ {:.4}", ba.price.to_f64(), ba.qty.0);
    }

    println!("\n=== Strategy Latency (engine→strategy) ===");
    println!("Updates received:  {}", stats.count);
    println!("Min latency:       {} ns", stats.min_latency_ns);
    println!("Max latency:       {} ns", stats.max_latency_ns);
    println!("Avg latency:       {} ns", stats.avg_latency_ns());
    println!("Median latency:    {} ns", stats.median());
    println!("P99 latency:       {} ns", stats.percentile(99.0));
    println!("P99.9 latency:     {} ns", stats.percentile(99.9));
}
