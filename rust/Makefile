CSV ?= ../btc_orderbook_updates.csv

.PHONY: build run benchmark clean test

# Build in release mode with native CPU optimizations
build:
	RUSTFLAGS="-C target-cpu=native" cargo build --release

# Run the orderbook system (live-style streaming with strategy logging)
run: build
	./target/release/orderbook_system $(CSV)

# Run the dedicated benchmark suite
benchmark: build
	./target/release/benchmark $(CSV)

# Run unit tests
test:
	cargo test

# Clean build artifacts
clean:
	cargo clean
