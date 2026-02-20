/// Core types for the ultra-low-latency orderbook system.
/// All types are designed to be small, Copy, and cache-friendly.

/// Represents a price level in the orderbook.
/// Using u64 fixed-point (price * 100) to avoid floating-point overhead.
/// For BTCUSDT with 2 decimal places, this gives us exact representation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct Price(pub u64);

impl Price {
    /// Convert from f64 price to fixed-point. Rounds to 2 decimal places.
    #[inline(always)]
    pub fn from_f64(p: f64) -> Self {
        // Multiply by 100 and round to get fixed-point representation
        Price((p * 100.0 + 0.5) as u64)
    }

    /// Convert back to f64 for display purposes only.
    #[inline(always)]
    pub fn to_f64(self) -> f64 {
        self.0 as f64 / 100.0
    }
}

/// Quantity stored as raw f64 — no arithmetic needed, just storage & display.
#[derive(Debug, Clone, Copy)]
pub struct Qty(pub f64);

impl Qty {
    #[inline(always)]
    pub fn is_zero(self) -> bool {
        self.0 <= f64::EPSILON
    }
}

/// Side of the order book.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Side {
    Bid = 0,
    Ask = 1,
}

/// A single price level update.
#[derive(Debug, Clone, Copy)]
pub struct Level {
    pub price: Price,
    pub qty: Qty,
}

/// Timestamp in nanoseconds (from the exchange data).
pub type Timestamp = u64;

/// An orderbook update event — either a full snapshot or a single incremental.
#[derive(Debug, Clone)]
pub enum Update {
    Snapshot {
        timestamp: Timestamp,
        bids: Vec<Level>,
        asks: Vec<Level>,
    },
    Incremental {
        timestamp: Timestamp,
        side: Side,
        level: Level,
    },
}

/// Notification sent from the orderbook engine to strategy consumers.
/// Contains the best bid/ask after each update. Kept small for cache efficiency.
#[derive(Debug, Clone, Copy)]
pub struct BookNotification {
    /// Timestamp of the update that triggered this notification (ns).
    pub update_timestamp: Timestamp,
    /// High-resolution monotonic clock timestamp when notification was sent (ns).
    pub engine_send_ns: u64,
    /// Best bid price and quantity (None if empty book).
    pub best_bid: Option<Level>,
    /// Best ask price and quantity (None if empty book).
    pub best_ask: Option<Level>,
    /// Sequence number for ordering.
    pub seq: u64,
}
