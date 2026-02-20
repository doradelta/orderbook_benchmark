#pragma once
/// Ultra-low-latency L2 orderbook engine (C++ version).
/// Uses std::map (red-black tree, equivalent to Rust BTreeMap for this purpose)
/// with cached best bid/ask for O(1) lookups.

#include <map>
#include "types.h"

class Orderbook {
public:
    /// Apply an update and return a notification.
    BookNotification apply(const Update& update, uint64_t send_ns) {
        if (update.type == Update::Type::Snapshot) {
            apply_snapshot(update.bids, update.asks);
        } else {
            apply_incremental(update.side, update.level);
        }
        ++seq_;
        return BookNotification{
            update.timestamp,
            send_ns,
            cached_best_bid_,
            cached_best_ask_,
            seq_
        };
    }

    std::optional<Level> best_bid() const { return cached_best_bid_; }
    std::optional<Level> best_ask() const { return cached_best_ask_; }
    size_t bid_depth() const { return bids_.size(); }
    size_t ask_depth() const { return asks_.size(); }

private:
    // Bids: sorted ascending, best bid = rbegin (highest price)
    std::map<uint64_t, double> bids_;
    // Asks: sorted ascending, best ask = begin (lowest price)
    std::map<uint64_t, double> asks_;

    std::optional<Level> cached_best_bid_;
    std::optional<Level> cached_best_ask_;
    uint64_t seq_ = 0;

    void apply_snapshot(const std::vector<Level>& bids, const std::vector<Level>& asks) {
        bids_.clear();
        asks_.clear();
        for (const auto& l : bids) {
            if (!l.qty.is_zero())
                bids_[l.price.raw] = l.qty.value;
        }
        for (const auto& l : asks) {
            if (!l.qty.is_zero())
                asks_[l.price.raw] = l.qty.value;
        }
        refresh_best_bid();
        refresh_best_ask();
    }

    void apply_incremental(Side side, Level level) {
        if (side == Side::Bid) {
            if (level.qty.is_zero()) {
                bids_.erase(level.price.raw);
                if (cached_best_bid_ && cached_best_bid_->price == level.price) {
                    refresh_best_bid();
                }
            } else {
                bids_[level.price.raw] = level.qty.value;
                if (!cached_best_bid_ || level.price >= cached_best_bid_->price) {
                    cached_best_bid_ = level;
                }
            }
        } else {
            if (level.qty.is_zero()) {
                asks_.erase(level.price.raw);
                if (cached_best_ask_ && cached_best_ask_->price == level.price) {
                    refresh_best_ask();
                }
            } else {
                asks_[level.price.raw] = level.qty.value;
                if (!cached_best_ask_ || level.price <= cached_best_ask_->price) {
                    cached_best_ask_ = level;
                }
            }
        }
    }

    void refresh_best_bid() {
        if (bids_.empty()) {
            cached_best_bid_ = std::nullopt;
        } else {
            auto it = bids_.rbegin();
            cached_best_bid_ = Level{Price(it->first), Qty(it->second)};
        }
    }

    void refresh_best_ask() {
        if (asks_.empty()) {
            cached_best_ask_ = std::nullopt;
        } else {
            auto it = asks_.begin();
            cached_best_ask_ = Level{Price(it->first), Qty(it->second)};
        }
    }
};
