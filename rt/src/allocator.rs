// TODO : add real heap allocation

use core::{alloc::GlobalAlloc, ptr::null_mut};

const HEAP_SIZE : usize = 64 * 1024;
static mut HEAP : [u8; HEAP_SIZE] = [0; HEAP_SIZE];
static mut HEAP_COUNT : usize = 0;

struct Allocator;

#[global_allocator]
static ALLOCATOR : Allocator = Allocator;

const fn align_up(addr: usize, align: usize) -> usize {
    debug_assert!(align.is_power_of_two());
    (addr + align - 1) & !(align - 1)
}

#[allow(static_mut_refs)]
unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: core::alloc::Layout) -> *mut u8 {
        unsafe {
            let heap_next = HEAP.as_mut_ptr().add(HEAP_COUNT) as usize;
            
            let heap_end = HEAP.as_mut_ptr().add(HEAP_SIZE) as usize;


            let aligned_ptr = align_up(heap_next, layout.align());
            let next_ptr = match aligned_ptr.checked_add(layout.size()){
                Some(next) => next,
                None => return null_mut(),
            };

            if next_ptr > heap_end {
                return null_mut();
            }

            HEAP_COUNT = next_ptr - HEAP.as_ptr() as usize;

            next_ptr as *mut u8
        }
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: core::alloc::Layout) {
        
    }
}