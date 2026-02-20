/// Dedicated benchmark binary for the orderbook system.
///
/// Measures:
/// 1. CSV parsing throughput
/// 2. Orderbook engine throughput (updates/sec) — no channel overhead
/// 3. End-to-end throughput with channel (engine → strategy)
/// 4. Engine → strategy latency distribution

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

const WARMUP_ITERATIONS: usize = 5;
const BENCH_ITERATIONS: usize = 20;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let csv_path = if args.len() > 1 {
        args[1].clone()
    } else {
        "btc_orderbook_updates.csv".to_string()
    };

    println!("╔══════════════════════════════════════════════════════╗");
    println!("║       ORDERBOOK SYSTEM — BENCHMARK SUITE            ║");
    println!("╚══════════════════════════════════════════════════════╝\n");

    let clock = quanta::Clock::new();

    // ── Benchmark 1: CSV Parsing ──────────────────────────────────
    println!("── Benchmark 1: CSV Parsing ──────────────────────────");
    let reader = CsvReader::open(&csv_path).expect("Failed to open CSV");

    // Warmup
    for _ in 0..WARMUP_ITERATIONS {
        let _ = reader.parse_all();
    }

    let mut parse_times_ns = Vec::with_capacity(BENCH_ITERATIONS);
    let updates_ref;
    {
        let mut last_updates = Vec::new();
        for _ in 0..BENCH_ITERATIONS {
            let start = clock.raw();
            let updates = reader.parse_all();
            let end = clock.raw();
            parse_times_ns.push(clock.delta_as_nanos(start, end));
            last_updates = updates;
        }
        updates_ref = last_updates;
    }

    let avg_parse_ns: u64 = parse_times_ns.iter().sum::<u64>() / parse_times_ns.len() as u64;
    let min_parse_ns: u64 = *parse_times_ns.iter().min().unwrap();
    let parse_throughput = (updates_ref.len() as f64 / min_parse_ns as f64) * 1_000_000_000.0;

    println!("  Updates parsed:    {}", updates_ref.len());
    println!("  Avg parse time:    {:.2} µs", avg_parse_ns as f64 / 1000.0);
    println!("  Min parse time:    {:.2} µs", min_parse_ns as f64 / 1000.0);
    println!("  Parse throughput:  {:.0} updates/sec (best run)\n", parse_throughput);

    // ── Benchmark 2: Orderbook Engine (no channel) ────────────────
    println!("── Benchmark 2: Orderbook Engine (isolated) ──────────");

    // Warmup
    for _ in 0..WARMUP_ITERATIONS {
        let mut book = Orderbook::new();
        for update in &updates_ref {
            book.apply(update, 0);
        }
    }

    let mut engine_times_ns = Vec::with_capacity(BENCH_ITERATIONS);
    for _ in 0..BENCH_ITERATIONS {
        let mut book = Orderbook::new();
        let start = clock.raw();
        for update in &updates_ref {
            book.apply(update, 0);
        }
        let end = clock.raw();
        engine_times_ns.push(clock.delta_as_nanos(start, end));
        // Prevent dead-code elimination
        std::hint::black_box(book.best_bid());
    }

    let avg_engine_ns: u64 = engine_times_ns.iter().sum::<u64>() / engine_times_ns.len() as u64;
    let min_engine_ns: u64 = *engine_times_ns.iter().min().unwrap();
    let per_update_ns = min_engine_ns as f64 / updates_ref.len() as f64;
    let engine_throughput = (updates_ref.len() as f64 / min_engine_ns as f64) * 1_000_000_000.0;

    println!("  Updates:           {}", updates_ref.len());
    println!("  Avg engine time:   {:.2} µs", avg_engine_ns as f64 / 1000.0);
    println!("  Min engine time:   {:.2} µs", min_engine_ns as f64 / 1000.0);
    println!("  Per-update:        {:.0} ns", per_update_ns);
    println!("  Engine throughput: {:.0} updates/sec (best run)\n", engine_throughput);

    // ── Benchmark 3: End-to-End with Channel ──────────────────────
    println!("── Benchmark 3: End-to-End (engine + channel + strategy) ──");

    let mut e2e_times_ns = Vec::with_capacity(BENCH_ITERATIONS);
    let mut last_stats = None;

    for i in 0..BENCH_ITERATIONS {
        let (tx, rx) = bounded::<BookNotification>(4096);
        let strategy_clock = clock.clone();

        let strategy_handle = thread::Builder::new()
            .name(format!("bench-strategy-{}", i))
            .spawn(move || run_strategy(rx, &strategy_clock, false))
            .unwrap();

        let mut book = Orderbook::new();
        let start = clock.raw();

        for update in &updates_ref {
            let now_ns = clock.delta_as_nanos(0, clock.raw());
            let notif = book.apply(update, now_ns);
            let _ = tx.send(notif);
        }

        drop(tx);
        let stats = strategy_handle.join().unwrap();
        let end = clock.raw();
        e2e_times_ns.push(clock.delta_as_nanos(start, end));
        last_stats = Some(stats);
    }

    let avg_e2e_ns: u64 = e2e_times_ns.iter().sum::<u64>() / e2e_times_ns.len() as u64;
    let min_e2e_ns: u64 = *e2e_times_ns.iter().min().unwrap();
    let e2e_throughput = (updates_ref.len() as f64 / min_e2e_ns as f64) * 1_000_000_000.0;

    println!("  Avg e2e time:      {:.2} µs", avg_e2e_ns as f64 / 1000.0);
    println!("  Min e2e time:      {:.2} µs", min_e2e_ns as f64 / 1000.0);
    println!("  E2E throughput:    {:.0} updates/sec (best run)\n", e2e_throughput);

    // ── Benchmark 4: Latency Distribution ─────────────────────────
    println!("── Benchmark 4: Engine → Strategy Latency ────────────");

    if let Some(stats) = &last_stats {
        println!("  Samples:           {}", stats.count);
        println!("  Min latency:       {} ns", stats.min_latency_ns);
        println!("  Max latency:       {} ns", stats.max_latency_ns);
        println!("  Avg latency:       {} ns", stats.avg_latency_ns());
        println!("  Median (P50):      {} ns", stats.median());
        println!("  P90 latency:       {} ns", stats.percentile(90.0));
        println!("  P95 latency:       {} ns", stats.percentile(95.0));
        println!("  P99 latency:       {} ns", stats.percentile(99.0));
        println!("  P99.9 latency:     {} ns", stats.percentile(99.9));
    }

    println!("\n╔══════════════════════════════════════════════════════╗");
    println!("║                   SUMMARY                           ║");
    println!("╠══════════════════════════════════════════════════════╣");
    println!("║  CSV parse throughput: {:>12.0} updates/sec     ║", parse_throughput);
    println!("║  Engine throughput:    {:>12.0} updates/sec     ║", engine_throughput);
    println!("║  E2E throughput:       {:>12.0} updates/sec     ║", e2e_throughput);
    println!("║  Per-update latency:   {:>9.0} ns               ║", per_update_ns);
    if let Some(stats) = &last_stats {
        println!("║  Median chan latency:  {:>9} ns               ║", stats.median());
        println!("║  P99 chan latency:     {:>9} ns               ║", stats.percentile(99.0));
    }
    println!("╚══════════════════════════════════════════════════════╝");
}
