#pragma once
/// Lock-free SPSC (Single Producer, Single Consumer) bounded ring buffer.
/// Cache-line padded to avoid false sharing. Direct equivalent of
/// crossbeam bounded channel in the Rust version.

#include <atomic>
#include <cstddef>
#include <optional>
#include <new>
#include <cstring>

#ifdef __cpp_lib_hardware_interference_size
    inline constexpr size_t CACHE_LINE = std::hardware_destructive_interference_size;
#else
    inline constexpr size_t CACHE_LINE = 64;
#endif

template <typename T, size_t Capacity>
class SPSCQueue {
    static_assert((Capacity & (Capacity - 1)) == 0, "Capacity must be power of 2");
    static constexpr size_t MASK = Capacity - 1;

    struct alignas(CACHE_LINE) Slot {
        std::atomic<uint64_t> seq;
        alignas(T) unsigned char storage[sizeof(T)];

        T* ptr() { return reinterpret_cast<T*>(storage); }
        const T* ptr() const { return reinterpret_cast<const T*>(storage); }
    };

    // Producer and consumer positions on separate cache lines to avoid false sharing
    alignas(CACHE_LINE) std::atomic<uint64_t> head_{0};   // producer writes here
    alignas(CACHE_LINE) std::atomic<uint64_t> tail_{0};   // consumer reads here
    alignas(CACHE_LINE) Slot slots_[Capacity];

public:
    SPSCQueue() {
        for (size_t i = 0; i < Capacity; ++i) {
            slots_[i].seq.store(i, std::memory_order_relaxed);
        }
    }

    /// Try to push an element. Returns false if full.
    bool try_push(const T& item) {
        uint64_t pos = head_.load(std::memory_order_relaxed);
        Slot& slot = slots_[pos & MASK];
        uint64_t seq = slot.seq.load(std::memory_order_acquire);
        if (seq != pos) return false; // full
        head_.store(pos + 1, std::memory_order_relaxed);
        new (slot.storage) T(item);
        slot.seq.store(pos + 1, std::memory_order_release);
        return true;
    }

    /// Blocking push — spins until slot available.
    void push(const T& item) {
        uint64_t pos = head_.load(std::memory_order_relaxed);
        Slot& slot = slots_[pos & MASK];
        while (slot.seq.load(std::memory_order_acquire) != pos) {
            // spin — for SPSC with fast consumer this rarely iterates
            #if defined(__x86_64__)
                __builtin_ia32_pause();
            #elif defined(__aarch64__)
                asm volatile("yield");
            #endif
        }
        head_.store(pos + 1, std::memory_order_relaxed);
        new (slot.storage) T(item);
        slot.seq.store(pos + 1, std::memory_order_release);
    }

    /// Try to pop an element. Returns nullopt if empty.
    std::optional<T> try_pop() {
        uint64_t pos = tail_.load(std::memory_order_relaxed);
        Slot& slot = slots_[pos & MASK];
        uint64_t seq = slot.seq.load(std::memory_order_acquire);
        if (seq != pos + 1) return std::nullopt; // empty
        tail_.store(pos + 1, std::memory_order_relaxed);
        T item = std::move(*slot.ptr());
        slot.ptr()->~T();
        slot.seq.store(pos + Capacity, std::memory_order_release);
        return item;
    }

    /// Blocking pop — spins until element available.
    /// Returns nullopt only if `closed` flag is set and queue is empty.
    std::optional<T> pop(const std::atomic<bool>& closed) {
        uint64_t pos = tail_.load(std::memory_order_relaxed);
        Slot& slot = slots_[pos & MASK];
        while (true) {
            uint64_t seq = slot.seq.load(std::memory_order_acquire);
            if (seq == pos + 1) break; // data ready
            if (closed.load(std::memory_order_acquire)) {
                // Check one more time in case producer wrote between checks
                if (slot.seq.load(std::memory_order_acquire) == pos + 1) break;
                return std::nullopt;
            }
            #if defined(__x86_64__)
                __builtin_ia32_pause();
            #elif defined(__aarch64__)
                asm volatile("yield");
            #endif
        }
        tail_.store(pos + 1, std::memory_order_relaxed);
        T item = std::move(*slot.ptr());
        slot.ptr()->~T();
        slot.seq.store(pos + Capacity, std::memory_order_release);
        return item;
    }
};
