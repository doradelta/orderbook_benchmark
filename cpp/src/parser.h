#pragma once
/// Ultra-fast CSV parser using mmap (C++ version).
/// Direct equivalent of the Rust parser: mmap + manual byte parsing.

#include <vector>
#include <cstring>
#include <cstdlib>
#include <fcntl.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <unistd.h>
#include <string>
#include <string_view>
#include "types.h"

class CsvReader {
public:
    static std::vector<Update> parse_file(const char* path) {
        // Open and mmap the file
        int fd = open(path, O_RDONLY);
        if (fd < 0) {
            perror("open");
            return {};
        }
        struct stat st;
        fstat(fd, &st);
        size_t size = static_cast<size_t>(st.st_size);

        const char* data = static_cast<const char*>(
            mmap(nullptr, size, PROT_READ, MAP_PRIVATE, fd, 0));
        if (data == MAP_FAILED) {
            perror("mmap");
            close(fd);
            return {};
        }
        // Advise sequential access for prefetch
        madvise(const_cast<char*>(data), size, MADV_SEQUENTIAL);
        close(fd);

        std::vector<Update> updates;
        updates.reserve(4096);

        const char* end = data + size;
        const char* pos = data;

        // Skip header line
        pos = skip_line(pos, end);

        while (pos < end) {
            const char* line_start = pos;
            const char* newline = find_newline(pos, end);
            const char* content_end = newline;

            // Strip \r if present (CRLF handling)
            if (content_end > line_start && *(content_end - 1) == '\r') {
                content_end--;
            }

            // Advance past \n
            pos = (newline < end) ? newline + 1 : newline;

            if (content_end <= line_start) continue;

            parse_line(line_start, content_end, updates);
        }

        munmap(const_cast<char*>(data), size);
        return updates;
    }

private:
    static const char* skip_line(const char* pos, const char* end) {
        while (pos < end && *pos != '\n') ++pos;
        return (pos < end) ? pos + 1 : pos;
    }

    static const char* find_newline(const char* pos, const char* end) {
        // Use memchr for SIMD-accelerated scan
        const char* nl = static_cast<const char*>(memchr(pos, '\n', end - pos));
        return nl ? nl : end;
    }

    static void parse_line(const char* start, const char* end, std::vector<Update>& out) {
        if (start >= end) return;

        if (*start == 's') {
            parse_snapshot(start, end, out);
        } else if (*start == 'i') {
            parse_incremental(start, end, out);
        }
    }

    /// Parse: incremental,binance,BTC/USDT,<ts>,bid/ask,,,<price>,<size>
    static void parse_incremental(const char* start, const char* end, std::vector<Update>& out) {
        Update u;
        u.type = Update::Type::Incremental;

        int field = 0;
        const char* field_start = start;

        for (const char* p = start; ; ++p) {
            bool at_end = (p == end);
            bool at_comma = (!at_end && *p == ',');

            if (at_comma || at_end) {
                switch (field) {
                    case 3: // timestamp
                        u.timestamp = parse_u64(field_start, p);
                        break;
                    case 4: // side
                        u.side = (*field_start == 'b') ? Side::Bid : Side::Ask;
                        break;
                    case 7: // price
                        u.level.price = Price::from_f64(parse_double(field_start, p));
                        break;
                    case 8: // size
                        u.level.qty = Qty(parse_double(field_start, p));
                        break;
                }
                field_start = p + 1;
                ++field;
            }
            if (at_end) break;
        }

        out.push_back(std::move(u));
    }

    /// Parse snapshot with JSON bid/ask arrays.
    static void parse_snapshot(const char* start, const char* end, std::vector<Update>& out) {
        Update u;
        u.type = Update::Type::Snapshot;

        // Parse fields respecting quotes (JSON arrays contain commas)
        std::vector<std::string_view> fields;
        fields.reserve(9);
        const char* fs = start;
        bool in_quotes = false;

        for (const char* p = start; ; ++p) {
            bool at_end = (p == end);
            if (!at_end) {
                if (*p == '"') in_quotes = !in_quotes;
                if (*p == ',' && !in_quotes) {
                    fields.emplace_back(fs, p - fs);
                    fs = p + 1;
                    continue;
                }
            }
            if (at_end) {
                fields.emplace_back(fs, p - fs);
                break;
            }
        }

        if (fields.size() < 7) return;

        u.timestamp = parse_u64(fields[3].data(), fields[3].data() + fields[3].size());

        // Parse bids JSON: strip quotes, then parse [[price, size], ...]
        auto bids_sv = strip_quotes(fields[5]);
        auto asks_sv = strip_quotes(fields[6]);

        u.bids = parse_levels_json(bids_sv);
        u.asks = parse_levels_json(asks_sv);

        out.push_back(std::move(u));
    }

    static std::string_view strip_quotes(std::string_view sv) {
        if (sv.size() >= 2 && sv.front() == '"' && sv.back() == '"') {
            return sv.substr(1, sv.size() - 2);
        }
        return sv;
    }

    /// Parse [[price, size], [price, size], ...] manually for speed.
    static std::vector<Level> parse_levels_json(std::string_view sv) {
        std::vector<Level> levels;
        levels.reserve(16);

        // State machine: find pairs of numbers between [ ]
        const char* p = sv.data();
        const char* end = p + sv.size();

        while (p < end) {
            // Find inner '['
            while (p < end && *p != '[') ++p;
            ++p; // skip '['
            if (p >= end) break;
            // Check if this is the outer '[' by seeing if next non-space is '['
            if (*p == '[') continue;

            // Parse price
            while (p < end && (*p == ' ' || *p == '\t')) ++p;
            const char* num_start = p;
            while (p < end && *p != ',' && *p != ']') ++p;
            double price = parse_double(num_start, p);

            // Skip comma
            if (p < end && *p == ',') ++p;

            // Parse size
            while (p < end && (*p == ' ' || *p == '\t')) ++p;
            num_start = p;
            while (p < end && *p != ']') ++p;
            double size = parse_double(num_start, p);

            levels.push_back(Level{Price::from_f64(price), Qty(size)});

            if (p < end) ++p; // skip ']'
        }

        return levels;
    }

    /// Fast u64 parsing from ASCII.
    static uint64_t parse_u64(const char* start, const char* end) {
        uint64_t result = 0;
        for (const char* p = start; p < end; ++p) {
            result = result * 10 + static_cast<uint64_t>(*p - '0');
        }
        return result;
    }

    /// Fast double parsing.
    static double parse_double(const char* start, const char* end) {
        // strtod needs null-terminated string, use a small buffer
        char buf[64];
        size_t len = std::min(static_cast<size_t>(end - start), sizeof(buf) - 1);
        memcpy(buf, start, len);
        buf[len] = '\0';
        return strtod(buf, nullptr);
    }
};
