use core::{alloc::GlobalAlloc, ptr::null_mut, sync::atomic::{AtomicUsize, Ordering}};

use crate::syscall::syscall_sbrk;

struct Allocator {
    current_brk : AtomicUsize,
}

#[global_allocator]
static ALLOCATOR : Allocator = Allocator { current_brk: AtomicUsize::new(0) };

const fn align_up(addr: usize, align: usize) -> Option<usize> {
    if !align.is_power_of_two(){
        return None;
    }
    match addr.checked_add(align - 1){
        Some(x) => Some(x & !(align - 1)),
        None => None,
    }
}

#[allow(static_mut_refs)]
unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: core::alloc::Layout) -> *mut u8 {
        // TODO : sbrk of more than real allocation to amortize syscalls ? (chunks of 64 KiB ?)

        if layout.size() == 0 {
            return layout.align() as *mut u8;
        }

        let old_brk = match self.current_brk.load(Ordering::Relaxed) {
            0 => match syscall_sbrk(0) {
                Some(sbrk) => sbrk as usize,
                None => return null_mut(),
            },
            current_brk => current_brk
        };

        let aligned_ptr = match align_up(old_brk, layout.align()){
            Some(ptr) => ptr,
            None => return null_mut(),
        };

        let new_brk = match aligned_ptr.checked_add(layout.size()){
            Some(new_brk) => new_brk,
            None => return null_mut(),
        };

        let increment = match new_brk.checked_sub(old_brk){
            Some(inc) => inc,
            None => return null_mut(),
        };

        if syscall_sbrk(increment as u64).is_none(){
            return null_mut();
        }

        self.current_brk.store(new_brk, Ordering::Relaxed);

        aligned_ptr as *mut u8
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: core::alloc::Layout) {
        // TODO : add freelist for freeing ?
    }
}