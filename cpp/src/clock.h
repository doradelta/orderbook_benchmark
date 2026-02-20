#pragma once
/// High-resolution clock using clock_gettime(CLOCK_MONOTONIC_RAW).
/// Equivalent to quanta::Clock in the Rust version — gives nanosecond precision.

#include <cstdint>
#include <time.h>

class Clock {
public:
    /// Get current time in nanoseconds.
    static inline uint64_t now_ns() {
        struct timespec ts;
        clock_gettime(CLOCK_MONOTONIC_RAW, &ts);
        return static_cast<uint64_t>(ts.tv_sec) * 1'000'000'000ULL +
               static_cast<uint64_t>(ts.tv_nsec);
    }

    /// Get raw TSC if available (x86), else fallback to clock_gettime.
    static inline uint64_t rdtsc() {
        #if defined(__x86_64__)
            uint32_t lo, hi;
            __asm__ volatile("rdtsc" : "=a"(lo), "=d"(hi));
            return (static_cast<uint64_t>(hi) << 32) | lo;
        #else
            return now_ns();
        #endif
    }
};
