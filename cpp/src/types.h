#pragma once
/// Core types for the ultra-low-latency orderbook system (C++ version).
/// All types designed to be small, trivially copyable, and cache-friendly.

#include <cstdint>
#include <cmath>
#include <vector>
#include <optional>

/// Fixed-point price: price * 100 stored as u64.
/// Avoids floating-point comparison issues entirely.
struct Price {
    uint64_t raw;

    Price() : raw(0) {}
    explicit Price(uint64_t r) : raw(r) {}

    static Price from_f64(double p) {
        return Price(static_cast<uint64_t>(p * 100.0 + 0.5));
    }

    double to_f64() const { return static_cast<double>(raw) / 100.0; }

    bool operator==(Price o) const { return raw == o.raw; }
    bool operator!=(Price o) const { return raw != o.raw; }
    bool operator<(Price o) const { return raw < o.raw; }
    bool operator>(Price o) const { return raw > o.raw; }
    bool operator<=(Price o) const { return raw <= o.raw; }
    bool operator>=(Price o) const { return raw >= o.raw; }
};

/// Quantity stored as raw double.
struct Qty {
    double value;

    Qty() : value(0.0) {}
    explicit Qty(double v) : value(v) {}

    bool is_zero() const { return value <= 1e-15; }
};

/// A single price level.
struct Level {
    Price price;
    Qty   qty;
};

enum class Side : uint8_t { Bid = 0, Ask = 1 };

using Timestamp = uint64_t;

/// An orderbook update — snapshot or incremental.
struct Update {
    enum class Type : uint8_t { Snapshot, Incremental };

    Type      type;
    Timestamp timestamp;
    Side      side;       // only for incremental
    Level     level;      // only for incremental

    // only for snapshot
    std::vector<Level> bids;
    std::vector<Level> asks;
};

/// Notification sent from engine to strategy.
/// Kept small (fits in 1-2 cache lines) for fast channel transfer.
struct alignas(64) BookNotification {
    Timestamp            update_timestamp;
    uint64_t             engine_send_ns;
    std::optional<Level> best_bid;
    std::optional<Level> best_ask;
    uint64_t             seq;
};
