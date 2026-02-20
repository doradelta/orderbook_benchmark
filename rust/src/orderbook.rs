/// Ultra-low-latency L2 orderbook engine.
///
/// Design choices for performance:
/// - BTreeMap for price levels: O(log n) insert/delete, O(1) best bid/ask via
///   cached extremes. BTreeMap is cache-friendlier than HashMap for ordered data.
/// - Cached best_bid / best_ask updated on every mutation — avoids tree traversal
///   on hot path (strategy read).
/// - Fixed-point prices (u64) for deterministic comparison — no floating-point issues.
/// - All operations are single-threaded on the write path; readers get snapshots
///   via lock-free channel.

use std::collections::BTreeMap;
use crate::types::*;

/// The core L2 orderbook.
#[allow(dead_code)]
pub struct Orderbook {
    /// Bids: price → qty. BTreeMap is sorted ascending; best bid = last entry.
    bids: BTreeMap<Price, Qty>,
    /// Asks: price → qty. BTreeMap is sorted ascending; best ask = first entry.
    asks: BTreeMap<Price, Qty>,
    /// Cached best bid for O(1) lookup.
    cached_best_bid: Option<Level>,
    /// Cached best ask for O(1) lookup.
    cached_best_ask: Option<Level>,
    /// Monotonic sequence counter.
    seq: u64,
}

#[allow(dead_code)]
impl Orderbook {
    /// Create an empty orderbook.
    #[inline]
    pub fn new() -> Self {
        Self {
            bids: BTreeMap::new(),
            asks: BTreeMap::new(),
            cached_best_bid: None,
            cached_best_ask: None,
            seq: 0,
        }
    }

    /// Apply an update and return a notification for strategy consumers.
    #[inline]
    pub fn apply(&mut self, update: &Update, send_ns: u64) -> BookNotification {
        match update {
            Update::Snapshot { timestamp, bids, asks } => {
                self.apply_snapshot(bids, asks);
                self.seq += 1;
                BookNotification {
                    update_timestamp: *timestamp,
                    engine_send_ns: send_ns,
                    best_bid: self.cached_best_bid,
                    best_ask: self.cached_best_ask,
                    seq: self.seq,
                }
            }
            Update::Incremental { timestamp, side, level } => {
                self.apply_incremental(*side, *level);
                self.seq += 1;
                BookNotification {
                    update_timestamp: *timestamp,
                    engine_send_ns: send_ns,
                    best_bid: self.cached_best_bid,
                    best_ask: self.cached_best_ask,
                    seq: self.seq,
                }
            }
        }
    }

    /// Apply a full snapshot: clear existing book and insert all levels.
    #[inline]
    fn apply_snapshot(&mut self, bids: &[Level], asks: &[Level]) {
        self.bids.clear();
        self.asks.clear();
        for level in bids {
            if !level.qty.is_zero() {
                self.bids.insert(level.price, level.qty);
            }
        }
        for level in asks {
            if !level.qty.is_zero() {
                self.asks.insert(level.price, level.qty);
            }
        }
        self.refresh_best_bid();
        self.refresh_best_ask();
    }

    /// Apply a single incremental update.
    #[inline(always)]
    fn apply_incremental(&mut self, side: Side, level: Level) {
        match side {
            Side::Bid => {
                if level.qty.is_zero() {
                    self.bids.remove(&level.price);
                    // Only recompute if we removed the best bid
                    if let Some(ref best) = self.cached_best_bid {
                        if best.price == level.price {
                            self.refresh_best_bid();
                        }
                    }
                } else {
                    self.bids.insert(level.price, level.qty);
                    // Update cache if this is a new best bid (higher price)
                    match self.cached_best_bid {
                        Some(ref best) if level.price <= best.price && level.price != best.price => {
                            // Not a new best, but if same price, update qty
                        }
                        Some(ref best) if level.price == best.price => {
                            self.cached_best_bid = Some(level);
                        }
                        _ => {
                            self.cached_best_bid = Some(level);
                        }
                    }
                }
            }
            Side::Ask => {
                if level.qty.is_zero() {
                    self.asks.remove(&level.price);
                    // Only recompute if we removed the best ask
                    if let Some(ref best) = self.cached_best_ask {
                        if best.price == level.price {
                            self.refresh_best_ask();
                        }
                    }
                } else {
                    self.asks.insert(level.price, level.qty);
                    // Update cache if this is a new best ask (lower price)
                    match self.cached_best_ask {
                        Some(ref best) if level.price >= best.price && level.price != best.price => {
                            // Not a new best, but if same price, update qty
                        }
                        Some(ref best) if level.price == best.price => {
                            self.cached_best_ask = Some(level);
                        }
                        _ => {
                            self.cached_best_ask = Some(level);
                        }
                    }
                }
            }
        }
    }

    /// Refresh cached best bid from the BTreeMap. Best bid = highest price = last entry.
    #[inline(always)]
    fn refresh_best_bid(&mut self) {
        self.cached_best_bid = self.bids.iter().next_back().map(|(&p, &q)| Level {
            price: p,
            qty: q,
        });
    }

    /// Refresh cached best ask from the BTreeMap. Best ask = lowest price = first entry.
    #[inline(always)]
    fn refresh_best_ask(&mut self) {
        self.cached_best_ask = self.asks.iter().next().map(|(&p, &q)| Level {
            price: p,
            qty: q,
        });
    }

    /// Get best bid (O(1) from cache).
    #[inline(always)]
    pub fn best_bid(&self) -> Option<Level> {
        self.cached_best_bid
    }

    /// Get best ask (O(1) from cache).
    #[inline(always)]
    pub fn best_ask(&self) -> Option<Level> {
        self.cached_best_ask
    }

    /// Number of bid levels.
    #[inline(always)]
    pub fn bid_depth(&self) -> usize {
        self.bids.len()
    }

    /// Number of ask levels.
    #[inline(always)]
    pub fn ask_depth(&self) -> usize {
        self.asks.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_snapshot_and_best() {
        let mut book = Orderbook::new();
        let update = Update::Snapshot {
            timestamp: 1,
            bids: vec![
                Level { price: Price::from_f64(100.0), qty: Qty(1.0) },
                Level { price: Price::from_f64(99.0), qty: Qty(2.0) },
            ],
            asks: vec![
                Level { price: Price::from_f64(101.0), qty: Qty(1.5) },
                Level { price: Price::from_f64(102.0), qty: Qty(3.0) },
            ],
        };
        book.apply(&update, 0);
        assert_eq!(book.best_bid().unwrap().price, Price::from_f64(100.0));
        assert_eq!(book.best_ask().unwrap().price, Price::from_f64(101.0));
    }

    #[test]
    fn test_incremental_delete() {
        let mut book = Orderbook::new();
        let snap = Update::Snapshot {
            timestamp: 1,
            bids: vec![
                Level { price: Price::from_f64(100.0), qty: Qty(1.0) },
                Level { price: Price::from_f64(99.0), qty: Qty(2.0) },
            ],
            asks: vec![
                Level { price: Price::from_f64(101.0), qty: Qty(1.5) },
            ],
        };
        book.apply(&snap, 0);

        // Delete best bid
        let del = Update::Incremental {
            timestamp: 2,
            side: Side::Bid,
            level: Level { price: Price::from_f64(100.0), qty: Qty(0.0) },
        };
        book.apply(&del, 0);
        assert_eq!(book.best_bid().unwrap().price, Price::from_f64(99.0));
    }

    #[test]
    fn test_incremental_new_best() {
        let mut book = Orderbook::new();
        let snap = Update::Snapshot {
            timestamp: 1,
            bids: vec![
                Level { price: Price::from_f64(100.0), qty: Qty(1.0) },
            ],
            asks: vec![
                Level { price: Price::from_f64(102.0), qty: Qty(1.0) },
            ],
        };
        book.apply(&snap, 0);

        // New best ask (lower)
        let upd = Update::Incremental {
            timestamp: 2,
            side: Side::Ask,
            level: Level { price: Price::from_f64(101.0), qty: Qty(0.5) },
        };
        book.apply(&upd, 0);
        assert_eq!(book.best_ask().unwrap().price, Price::from_f64(101.0));
    }
}
