#!/usr/bin/env bash
# ──────────────────────────────────────────────────────────────
# Benchmark comparison: Rust vs C++ orderbook systems
# Runs each benchmark N times, collects per-run stats, prints
# a side-by-side comparison table with averages.
# ──────────────────────────────────────────────────────────────
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
CSV="$SCRIPT_DIR/btc_orderbook_updates.csv"
RUST_BIN="$SCRIPT_DIR/rust/target/release/benchmark"
CPP_BIN="$SCRIPT_DIR/cpp/build/benchmark"
ITERATIONS=${1:-100}

# ── Verify binaries exist ─────────────────────────────────────
if [[ ! -x "$RUST_BIN" ]]; then
    echo "ERROR: Rust binary not found. Run 'make build' first."
    exit 1
fi
if [[ ! -x "$CPP_BIN" ]]; then
    echo "ERROR: C++ binary not found. Run 'make build' first."
    exit 1
fi

echo "╔══════════════════════════════════════════════════════════╗"
echo "║     RUST vs C++ ORDERBOOK BENCHMARK COMPARISON          ║"
echo "╠══════════════════════════════════════════════════════════╣"
echo "║  Iterations per language: $ITERATIONS"
echo "║  CSV: $(basename "$CSV")"
echo "╚══════════════════════════════════════════════════════════╝"
echo ""

# ── Helper: extract metrics from a single benchmark run ───────
# Each benchmark prints lines like:
#   Engine throughput: 12345678 updates/sec (best run)
#   Per-update:        35 ns
#   E2E throughput:    12345678 updates/sec (best run)
#   Median (P50):      1234 ns
#   P99 latency:       5678 ns
#   Min parse time:    123.45 µs
extract_metric() {
    local label="$1"
    local output="$2"
    echo "$output" | grep -oP "${label}\s*[:=]\s*\K[0-9.]+" | head -1
}

# ── Run benchmarks ────────────────────────────────────────────
run_bench() {
    local bin="$1"
    local name="$2"
    local iters="$3"

    local sum_parse_tp=0 sum_engine_tp=0 sum_e2e_tp=0
    local sum_per_update=0 sum_median_lat=0 sum_p99_lat=0

    echo "Running $name benchmark ($iters iterations)..."
    for ((i=1; i<=iters; i++)); do
        if (( i % 10 == 0 )); then
            printf "\r  Progress: %d/%d" "$i" "$iters"
        fi

        output=$("$bin" "$CSV" 2>&1)

        local parse_tp=$(extract_metric "Parse throughput" "$output")
        local engine_tp=$(extract_metric "Engine throughput" "$output")
        local e2e_tp=$(extract_metric "E2E throughput" "$output")
        local per_update=$(extract_metric "Per-update" "$output")
        local median_lat=$(extract_metric "Median \\(P50\\)" "$output")
        local p99_lat=$(extract_metric "P99 latency" "$output")

        sum_parse_tp=$(echo "$sum_parse_tp + ${parse_tp:-0}" | bc)
        sum_engine_tp=$(echo "$sum_engine_tp + ${engine_tp:-0}" | bc)
        sum_e2e_tp=$(echo "$sum_e2e_tp + ${e2e_tp:-0}" | bc)
        sum_per_update=$(echo "$sum_per_update + ${per_update:-0}" | bc)
        sum_median_lat=$(echo "$sum_median_lat + ${median_lat:-0}" | bc)
        sum_p99_lat=$(echo "$sum_p99_lat + ${p99_lat:-0}" | bc)
    done
    printf "\r  Progress: %d/%d  ✓\n" "$iters" "$iters"

    # Output averages as tab-separated values
    echo "RESULTS:$name"
    echo "AVG_PARSE_TP:$(echo "scale=0; $sum_parse_tp / $iters" | bc)"
    echo "AVG_ENGINE_TP:$(echo "scale=0; $sum_engine_tp / $iters" | bc)"
    echo "AVG_E2E_TP:$(echo "scale=0; $sum_e2e_tp / $iters" | bc)"
    echo "AVG_PER_UPDATE:$(echo "scale=0; $sum_per_update / $iters" | bc)"
    echo "AVG_MEDIAN_LAT:$(echo "scale=0; $sum_median_lat / $iters" | bc)"
    echo "AVG_P99_LAT:$(echo "scale=0; $sum_p99_lat / $iters" | bc)"
}

# Run both and capture
rust_out=$(run_bench "$RUST_BIN" "Rust" "$ITERATIONS")
echo ""
cpp_out=$(run_bench "$CPP_BIN" "C++" "$ITERATIONS")
echo ""

# ── Extract averages ──────────────────────────────────────────
get_val() { echo "$1" | grep "^$2:" | cut -d: -f2; }

r_parse=$(get_val "$rust_out" "AVG_PARSE_TP")
r_engine=$(get_val "$rust_out" "AVG_ENGINE_TP")
r_e2e=$(get_val "$rust_out" "AVG_E2E_TP")
r_per=$(get_val "$rust_out" "AVG_PER_UPDATE")
r_med=$(get_val "$rust_out" "AVG_MEDIAN_LAT")
r_p99=$(get_val "$rust_out" "AVG_P99_LAT")

c_parse=$(get_val "$cpp_out" "AVG_PARSE_TP")
c_engine=$(get_val "$cpp_out" "AVG_ENGINE_TP")
c_e2e=$(get_val "$cpp_out" "AVG_E2E_TP")
c_per=$(get_val "$cpp_out" "AVG_PER_UPDATE")
c_med=$(get_val "$cpp_out" "AVG_MEDIAN_LAT")
c_p99=$(get_val "$cpp_out" "AVG_P99_LAT")

# ── Determine winners ────────────────────────────────────────
winner_higher() {
    if (( $1 > $2 )); then echo "Rust"; else echo "C++"; fi
}
winner_lower() {
    if (( $1 < $2 )); then echo "Rust"; else echo "C++"; fi
}

# ── Print comparison table ────────────────────────────────────
echo "╔════════════════════════════════════════════════════════════════════════════╗"
echo "║              BENCHMARK COMPARISON — AVERAGE OF $ITERATIONS RUNS"
echo "╠════════════════════════════════════════════════════════════════════════════╣"
printf "║  %-30s %15s %15s %10s  ║\n" "METRIC" "RUST" "C++" "WINNER"
echo "╠════════════════════════════════════════════════════════════════════════════╣"
printf "║  %-30s %12s/s %12s/s %10s  ║\n" "CSV Parse Throughput" "$r_parse" "$c_parse" "$(winner_higher ${r_parse:-0} ${c_parse:-0})"
printf "║  %-30s %12s/s %12s/s %10s  ║\n" "Engine Throughput" "$r_engine" "$c_engine" "$(winner_higher ${r_engine:-0} ${c_engine:-0})"
printf "║  %-30s %12s/s %12s/s %10s  ║\n" "E2E Throughput" "$r_e2e" "$c_e2e" "$(winner_higher ${r_e2e:-0} ${c_e2e:-0})"
printf "║  %-30s %10s ns %10s ns %10s  ║\n" "Per-Update Latency" "$r_per" "$c_per" "$(winner_lower ${r_per:-0} ${c_per:-0})"
printf "║  %-30s %10s ns %10s ns %10s  ║\n" "Channel Median Latency" "$r_med" "$c_med" "$(winner_lower ${r_med:-0} ${c_med:-0})"
printf "║  %-30s %10s ns %10s ns %10s  ║\n" "Channel P99 Latency" "$r_p99" "$c_p99" "$(winner_lower ${r_p99:-0} ${c_p99:-0})"
echo "╚════════════════════════════════════════════════════════════════════════════╝"

# ── Save to file ──────────────────────────────────────────────
REPORT="$SCRIPT_DIR/BENCHMARK_COMPARISON.txt"
{
    echo "BENCHMARK COMPARISON — Rust vs C++ Orderbook System"
    echo "==================================================="
    echo "Date: $(date)"
    echo "Iterations per language: $ITERATIONS"
    echo ""
    printf "%-32s %15s %15s %10s\n" "METRIC" "RUST" "C++" "WINNER"
    echo "-------------------------------------------------------------------"
    printf "%-32s %12s/s %12s/s %10s\n" "CSV Parse Throughput" "$r_parse" "$c_parse" "$(winner_higher ${r_parse:-0} ${c_parse:-0})"
    printf "%-32s %12s/s %12s/s %10s\n" "Engine Throughput" "$r_engine" "$c_engine" "$(winner_higher ${r_engine:-0} ${c_engine:-0})"
    printf "%-32s %12s/s %12s/s %10s\n" "E2E Throughput" "$r_e2e" "$c_e2e" "$(winner_higher ${r_e2e:-0} ${c_e2e:-0})"
    printf "%-32s %10s ns %10s ns %10s\n" "Per-Update Latency" "$r_per" "$c_per" "$(winner_lower ${r_per:-0} ${c_per:-0})"
    printf "%-32s %10s ns %10s ns %10s\n" "Channel Median Latency" "$r_med" "$c_med" "$(winner_lower ${r_med:-0} ${c_med:-0})"
    printf "%-32s %10s ns %10s ns %10s\n" "Channel P99 Latency" "$r_p99" "$c_p99" "$(winner_lower ${r_p99:-0} ${c_p99:-0})"
} > "$REPORT"

echo ""
echo "Results saved to: BENCHMARK_COMPARISON.txt"
