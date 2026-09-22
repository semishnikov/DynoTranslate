//! A global allocator that counts, for the soak and chaos runs.
//!
//! The plan's soak target is "memory growth under 5 percent, flat". Asking the operating system
//! for the process residency would measure the runtime, and the answer differs between Linux
//! and Windows for the same allocation. Counting the allocations the pipeline itself makes is
//! platform-stable and, with a warmed baseline, says exactly what a long run leaked. The
//! counter is installed for the whole binary; in the ordinary modes it is harmless, it only
//! counts.

use std::alloc::{GlobalAlloc, Layout};
use std::sync::atomic::{AtomicUsize, Ordering};

struct CountingAllocator {
    live_bytes: AtomicUsize,
    peak_bytes: AtomicUsize,
    allocations: AtomicUsize,
    deallocations: AtomicUsize,
}

impl CountingAllocator {
    const fn new() -> Self {
        Self {
            live_bytes: AtomicUsize::new(0),
            peak_bytes: AtomicUsize::new(0),
            allocations: AtomicUsize::new(0),
            deallocations: AtomicUsize::new(0),
        }
    }

    fn note_growth(&self, size: usize) {
        let live = self.live_bytes.fetch_add(size, Ordering::Relaxed) + size;
        let mut peak = self.peak_bytes.load(Ordering::Relaxed);
        while live > peak {
            match self
                .peak_bytes
                .compare_exchange_weak(peak, live, Ordering::Relaxed, Ordering::Relaxed)
            {
                Ok(_) => break,
                Err(current) => peak = current,
            }
        }
    }

    fn note_shrink(&self, size: usize) {
        self.live_bytes.fetch_sub(size, Ordering::Relaxed);
    }
}

// The counter is a bag of atomics: it never fails and never blocks.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = std::alloc::alloc(layout);
        if !pointer.is_null() {
            self.allocations.fetch_add(1, Ordering::Relaxed);
            self.note_growth(layout.size());
        }
        pointer
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        self.deallocations.fetch_add(1, Ordering::Relaxed);
        self.note_shrink(layout.size());
        std::alloc::dealloc(ptr, layout);
    }
}

/// A point-in-time reading of the counter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AllocSnapshot {
    pub live_bytes: usize,
    pub peak_bytes: usize,
    pub allocations: u64,
    pub deallocations: u64,
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator::new();

/// The current reading, for the soak and chaos reports.
pub fn snapshot() -> AllocSnapshot {
    AllocSnapshot {
        live_bytes: ALLOCATOR.live_bytes.load(Ordering::Relaxed),
        peak_bytes: ALLOCATOR.peak_bytes.load(Ordering::Relaxed),
        allocations: ALLOCATOR.allocations.load(Ordering::Relaxed) as u64,
        deallocations: ALLOCATOR.deallocations.load(Ordering::Relaxed) as u64,
    }
}
