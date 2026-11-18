//! Heap tracking allocator for peak-memory profiling.
//!
//! Install in a binary with `#[global_allocator]`. Counters are process-wide.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

static CURRENT: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

/// System allocator wrapper that tracks live and peak heap bytes.
pub struct TrackingAlloc;

fn grow(bytes: usize) {
    let now = CURRENT.fetch_add(bytes, Relaxed) + bytes;
    PEAK.fetch_max(now, Relaxed);
}

fn shrink(bytes: usize) {
    CURRENT.fetch_sub(bytes, Relaxed);
}

unsafe impl GlobalAlloc for TrackingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            grow(layout.size());
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) };
        shrink(layout.size());
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let new = unsafe { System.realloc(ptr, layout, new_size) };
        if !new.is_null() {
            if new_size >= layout.size() {
                grow(new_size - layout.size());
            } else {
                shrink(layout.size() - new_size);
            }
        }
        new
    }
}

/// Live heap bytes.
pub fn current() -> usize {
    CURRENT.load(Relaxed)
}

/// Highest live heap bytes since the last [`reset_peak`].
pub fn peak() -> usize {
    PEAK.load(Relaxed)
}

/// Reset the peak to the current live size and return it.
pub fn reset_peak() -> usize {
    let now = current();
    PEAK.store(now, Relaxed);
    now
}
