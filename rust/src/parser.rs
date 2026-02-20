/// Ultra-fast CSV parser for orderbook updates.
///
/// Design choices:
/// - Memory-mapped file I/O via `memmap2` — zero-copy read, OS handles paging.
/// - Manual byte-level parsing — avoids allocation from csv crate overhead.
/// - JSON arrays parsed with minimal serde_json — only for snapshot rows.
/// - Incremental rows are parsed entirely without allocation.

use memmap2::Mmap;
use std::fs::File;
use std::path::Path;
use crate::types::*;

/// Memory-mapped CSV reader. Holds the mmap and yields updates.
pub struct CsvReader {
    mmap: Mmap,
}

impl CsvReader {
    /// Open and memory-map the CSV file.
    pub fn open<P: AsRef<Path>>(path: P) -> std::io::Result<Self> {
        let file = File::open(path)?;
        let mmap = unsafe { Mmap::map(&file)? };
        // Advise the OS for sequential access
        #[cfg(unix)]
        {
            let _ = mmap.advise(memmap2::Advice::Sequential);
        }
        Ok(Self { mmap })
    }

    /// Parse all updates from the CSV into a pre-allocated Vec.
    /// We parse everything upfront to avoid allocation during the hot loop.
    pub fn parse_all(&self) -> Vec<Update> {
        let data = &self.mmap[..];
        let mut updates = Vec::with_capacity(4096);
        let mut pos = 0;

        // Skip header line
        pos = skip_line(data, pos);

        while pos < data.len() {
            let line_start = pos;
            // Find the \n (or end of data)
            let newline_pos = find_newline(data, pos);
            // Content end: strip trailing \r if present
            let content_end = if newline_pos > line_start && data[newline_pos - 1] == b'\r' {
                newline_pos - 1
            } else {
                newline_pos
            };
            // Advance past the \n
            pos = if newline_pos < data.len() { newline_pos + 1 } else { newline_pos };

            if content_end <= line_start {
                continue;
            }
            let line = &data[line_start..content_end];

            if let Some(update) = parse_line(line) {
                updates.push(update);
            }
        }

        updates
    }
}

/// Skip to the end of the current line.
#[inline(always)]
fn skip_line(data: &[u8], mut pos: usize) -> usize {
    while pos < data.len() && data[pos] != b'\n' {
        pos += 1;
    }
    pos + 1
}

/// Find position of next \n or end of data.
#[inline(always)]
fn find_newline(data: &[u8], mut pos: usize) -> usize {
    while pos < data.len() && data[pos] != b'\n' {
        pos += 1;
    }
    pos
}

/// Parse a single CSV line into an Update.
fn parse_line(line: &[u8]) -> Option<Update> {
    if line.is_empty() {
        return None;
    }

    // Determine type by first character: 's' for snapshot, 'i' for incremental
    match line[0] {
        b's' => parse_snapshot_line(line),
        b'i' => parse_incremental_line(line),
        _ => None,
    }
}

/// Parse a snapshot line. Format:
/// snapshot,binance,BTC/USDT,<timestamp>,,"[[p,s],...]","[[p,s],...]",,
fn parse_snapshot_line(line: &[u8]) -> Option<Update> {
    // Fields: type(0), exchange(1), symbol(2), timestamp(3), side(4), bids(5), asks(6), price(7), size(8)
    // For snapshot: side, price, size are empty. bids and asks are JSON arrays potentially quoted.

    let s = std::str::from_utf8(line).ok()?;

    // Split carefully — bids/asks contain commas inside JSON, so we need to handle quoted fields.
    let fields = parse_csv_fields(s);
    if fields.len() < 7 {
        return None;
    }

    let timestamp: Timestamp = fields[3].parse().ok()?;

    let bids_str = fields[5].trim_matches('"');
    let asks_str = fields[6].trim_matches('"');

    let bids = parse_levels_json(bids_str)?;
    let asks = parse_levels_json(asks_str)?;

    Some(Update::Snapshot { timestamp, bids, asks })
}

/// Parse a CSV line respecting quoted fields (for JSON arrays with commas).
fn parse_csv_fields(s: &str) -> Vec<&str> {
    let mut fields = Vec::with_capacity(9);
    let mut start = 0;
    let mut in_quotes = false;
    let bytes = s.as_bytes();

    for i in 0..bytes.len() {
        match bytes[i] {
            b'"' => in_quotes = !in_quotes,
            b',' if !in_quotes => {
                fields.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    fields.push(&s[start..]);
    fields
}

/// Parse a JSON array of [price, size] pairs into Levels.
/// Input: "[[99999.99, 0.527], [99998.86, 3.1404], ...]"
fn parse_levels_json(s: &str) -> Option<Vec<Level>> {
    let parsed: Vec<Vec<f64>> = serde_json::from_str(s).ok()?;
    let levels: Vec<Level> = parsed
        .iter()
        .map(|pair| Level {
            price: Price::from_f64(pair[0]),
            qty: Qty(pair[1]),
        })
        .collect();
    Some(levels)
}

/// Parse an incremental line. Format:
/// incremental,binance,BTC/USDT,<timestamp>,bid/ask,,,<price>,<size>
fn parse_incremental_line(line: &[u8]) -> Option<Update> {
    // Fast manual parsing — no allocation.
    let mut field_idx = 0;
    let mut field_start = 0;
    let mut timestamp: Timestamp = 0;
    let mut side = Side::Bid;
    let mut price = 0.0f64;
    let mut size = 0.0f64;

    for i in 0..=line.len() {
        let is_end = i == line.len();
        let is_comma = !is_end && line[i] == b',';

        if is_comma || is_end {
            let field = &line[field_start..i];
            match field_idx {
                // 0 = type (skip, we know it's incremental)
                // 1 = exchange (skip)
                // 2 = symbol (skip)
                3 => {
                    // timestamp — parse u64 manually for speed
                    timestamp = parse_u64_fast(field);
                }
                4 => {
                    // side
                    if field.len() >= 3 && field[0] == b'b' {
                        side = Side::Bid;
                    } else {
                        side = Side::Ask;
                    }
                }
                // 5 = bids (empty)
                // 6 = asks (empty)
                7 => {
                    // price
                    price = parse_f64_fast(field);
                }
                8 => {
                    // size
                    size = parse_f64_fast(field);
                }
                _ => {}
            }
            field_start = i + 1;
            field_idx += 1;
        }
    }

    Some(Update::Incremental {
        timestamp,
        side,
        level: Level {
            price: Price::from_f64(price),
            qty: Qty(size),
        },
    })
}

/// Ultra-fast u64 parsing from ASCII bytes — no bounds checks, no allocation.
#[inline(always)]
fn parse_u64_fast(bytes: &[u8]) -> u64 {
    let mut result: u64 = 0;
    for &b in bytes {
        result = result.wrapping_mul(10).wrapping_add((b.wrapping_sub(b'0')) as u64);
    }
    result
}

/// Fast f64 parsing from ASCII bytes.
#[inline(always)]
fn parse_f64_fast(bytes: &[u8]) -> f64 {
    // Use fast_float or fallback to std. For our data this is sufficient.
    if let Ok(s) = std::str::from_utf8(bytes) {
        s.parse::<f64>().unwrap_or(0.0)
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_incremental() {
        let line = b"incremental,binance,BTC/USDT,1700000000100,bid,,,99999.99,0.0";
        let update = parse_line(line).unwrap();
        match update {
            Update::Incremental { timestamp, side, level } => {
                assert_eq!(timestamp, 1700000000100);
                assert_eq!(side, Side::Bid);
                assert_eq!(level.price, Price::from_f64(99999.99));
                assert!(level.qty.is_zero());
            }
            _ => panic!("Expected incremental"),
        }
    }

    #[test]
    fn test_parse_u64_fast() {
        assert_eq!(parse_u64_fast(b"1700000000100"), 1700000000100u64);
        assert_eq!(parse_u64_fast(b"0"), 0);
    }
}
