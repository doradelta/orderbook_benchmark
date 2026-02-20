.PHONY: build build-rust build-cpp run run-rust run-cpp benchmark benchmark-rust benchmark-cpp compare clean test

CSV := btc_orderbook_updates.csv

# ── Build both ──────────────────────────────────────────
build: build-rust build-cpp

build-rust:
	@echo "=== Building Rust ==="
	$(MAKE) -C rust build CSV=../$(CSV)

build-cpp:
	@echo "=== Building C++ ==="
	$(MAKE) -C cpp build

# ── Run both ────────────────────────────────────────────
run: run-rust run-cpp

run-rust: build-rust
	@echo "\n=== Running Rust ==="
	$(MAKE) -C rust run CSV=../$(CSV)

run-cpp: build-cpp
	@echo "\n=== Running C++ ==="
	$(MAKE) -C cpp run CSV=../$(CSV)

# ── Benchmark each individually ─────────────────────────
benchmark: benchmark-rust benchmark-cpp

benchmark-rust: build-rust
	@echo "\n=== Rust Benchmark ==="
	$(MAKE) -C rust benchmark CSV=../$(CSV)

benchmark-cpp: build-cpp
	@echo "\n=== C++ Benchmark ==="
	$(MAKE) -C cpp benchmark CSV=../$(CSV)

# ── Head-to-head comparison (100 iterations each) ──────
compare: build
	@chmod +x compare_benchmarks.sh
	./compare_benchmarks.sh 100

# ── Tests ───────────────────────────────────────────────
test:
	$(MAKE) -C rust test

# ── Clean ───────────────────────────────────────────────
clean:
	$(MAKE) -C rust clean
	$(MAKE) -C cpp clean
