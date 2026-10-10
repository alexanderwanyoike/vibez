//! Callback allocation and ownership-destruction observations.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
thread_local! {static DEALLOCATIONS:Cell<usize>=const {Cell::new(0)};static ACTIVE:Cell<bool>=const {Cell::new(false)};static COUNT:Cell<usize>=const {Cell::new(0)};}
pub struct AllocationCounter;
unsafe impl GlobalAlloc for AllocationCounter {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ACTIVE.with(|active| {
            if active.get() {
                COUNT.with(|count| count.set(count.get() + 1));
            }
        });
        System.alloc(layout)
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        ACTIVE.with(|active| {
            if active.get() {
                DEALLOCATIONS.with(|count| count.set(count.get() + 1));
            }
        });
        System.dealloc(pointer, layout)
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        ACTIVE.with(|active| {
            if active.get() {
                COUNT.with(|count| count.set(count.get() + 1));
            }
        });
        System.realloc(pointer, layout, size)
    }
}
pub fn count_allocations(action: impl FnOnce()) -> usize {
    count_activity(action).0
}
pub fn count_activity(action: impl FnOnce()) -> (usize, usize) {
    DEALLOCATIONS.with(|count| count.set(0));
    COUNT.with(|count| count.set(0));
    ACTIVE.with(|active| active.set(true));
    action();
    ACTIVE.with(|active| active.set(false));
    (COUNT.with(Cell::get), DEALLOCATIONS.with(Cell::get))
}
