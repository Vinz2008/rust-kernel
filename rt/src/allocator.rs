use core::{alloc::GlobalAlloc, ptr::{NonNull, null_mut}, sync::atomic::{AtomicPtr, AtomicUsize, Ordering}};

use crate::syscall::syscall_sbrk;

struct Allocator {
    heap_start : AtomicUsize,
    current_brk : AtomicUsize,
    free_list_start : AtomicPtr<FreeListNode>,
}

pub struct FreeListNode {
    size : usize,
    next : Option<NonNull<FreeListNode>>,
}

// TODO : use a fixed-size approach by rounding to power of 2 ?

#[global_allocator]
static ALLOCATOR : Allocator = Allocator {
    heap_start: AtomicUsize::new(0),
    current_brk: AtomicUsize::new(0),
    free_list_start: AtomicPtr::new(null_mut()),
};

impl Allocator {
    fn get_free_list_start(&self) -> Option<NonNull<FreeListNode>> {
        let free_list_start_ptr = self.free_list_start.load(Ordering::Relaxed);
        match free_list_start_ptr as usize {
            0 => None,
            _ =>  Some(NonNull::new(free_list_start_ptr)?),
        }
    }

    fn set_free_list_start(&self, start : Option<NonNull<FreeListNode>>){
        let start_ptr = match start {
            Some(start) => start.as_ptr(),
            None => null_mut(),
        };
        self.free_list_start.store(start_ptr, Ordering::Relaxed);
    }
}

const fn align_up(addr: usize, align: usize) -> Option<usize> {
    if !align.is_power_of_two(){
        return None;
    }
    match addr.checked_add(align - 1){
        Some(x) => Some(x & !(align - 1)),
        None => None,
    }
}

fn grow_heap(allocator : &Allocator, increment : usize) -> Option<()> {
    let old_brk = syscall_sbrk(increment as u64)? as usize;
    let new_brk = old_brk.checked_add(increment)?;

    allocator.current_brk.store(new_brk, Ordering::Relaxed);

    add_free_region(allocator, old_brk, increment)?;
    Some(())
}

fn try_allocate_in_node(node : &FreeListNode, size : usize, align: usize) -> Option<usize> {
    let region_start = node as *const FreeListNode as usize;
    let region_end = region_start.checked_add(node.size)?;
    let aligned_ptr = align_up(region_start, align)?;
    let alloc_end = aligned_ptr.checked_add(size)?;
    

    if alloc_end <= region_end {
        Some(aligned_ptr)
    } else {
        None
    }
}

fn find_free_node_alloc(allocator : &Allocator, size : usize, align: usize) -> Option<(NonNull<FreeListNode>, usize)> {
    let mut prev: Option<NonNull<FreeListNode>> = None;
    let mut current = allocator.get_free_list_start();

    while let Some(mut node) = current {
        let node_ref = unsafe { node.as_mut() };

        if let Some(alloc_ptr) = try_allocate_in_node(node_ref, size, align) {
            let next = node_ref.next;
            match prev {
                Some(mut prev) => {
                    unsafe {
                        prev.as_mut().next = next;
                    }
                }
                None => {
                    allocator.set_free_list_start(next);
                }
            }
            node_ref.next = None;
            return Some((node, alloc_ptr));
        }

        current = node_ref.next;
        prev = Some(node);
    }
    None
}

const HEAP_CHUNK_SIZE : usize = 4096;

const MIN_WORTHWHILE_BLOCK_SIZE : usize = 8;

fn add_free_region(allocator : &Allocator, addr : usize, size : usize) -> Option<()> {
    
    let aligned_addr = align_up(addr, align_of::<FreeListNode>())?;
    let padding = aligned_addr.checked_sub(addr)?;
    let size = size.checked_sub(padding)?;

    if size < size_of::<FreeListNode>() + MIN_WORTHWHILE_BLOCK_SIZE {
        return Some(());
    }
    
    let free_list_start = allocator.get_free_list_start();
    let new_block = FreeListNode {
        size,
        next: free_list_start,
    };
    let new_block_ptr = aligned_addr as *mut FreeListNode;
    unsafe {
        new_block_ptr.write(new_block);
    }
    allocator.free_list_start.store(new_block_ptr, Ordering::Relaxed);
    Some(())
}

fn init_allocator(allocator : &Allocator) -> Option<usize> {
    let current_brk = syscall_sbrk(0)? as usize;
    allocator.heap_start.store(current_brk, Ordering::Relaxed);
    grow_heap(allocator, HEAP_CHUNK_SIZE)?;
    
    Some(current_brk)
}

fn add_free_block_before_and_after(allocator : &Allocator, free_node : NonNull<FreeListNode>, alloc_ptr : usize, size : usize) -> Option<()> {
    let region_start = free_node.as_ptr() as usize;
    let region_size = unsafe { free_node.as_ref().size };
    let region_end = region_start.checked_add(region_size)?;

    let alloc_end = alloc_ptr.checked_add(size)?;
    let empty_size_before = alloc_ptr - region_start;
    let empty_size_after  = region_end - alloc_end;

    add_free_region(allocator, region_start, empty_size_before)?;

    add_free_region(allocator, alloc_end, empty_size_after)?;

    Some(())
}

#[allow(static_mut_refs)]
unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: core::alloc::Layout) -> *mut u8 {
        // TODO : sbrk of more than real allocation to amortize syscalls ? (chunks of 64 KiB ?)

        if layout.size() == 0 {
            return layout.align() as *mut u8;
        }

        if self.current_brk.load(Ordering::Relaxed) == 0 && init_allocator(self).is_none() {
            return null_mut()
        };

        let (free_node, alloc_ptr) = match find_free_node_alloc(self, layout.size(), layout.align()){
            Some((node, alloc)) => (node, alloc),
            None => {
                let needed = match layout.size().checked_add(layout.align()) {
                    Some(n) => n,
                    None => return null_mut(),
                };
                let needed_with_meta = match needed.checked_add(size_of::<FreeListNode>()){
                    Some(n) => n,
                    None => return null_mut(),
                };
                let chunks_nb = needed_with_meta.div_ceil(HEAP_CHUNK_SIZE);
                let increment = match chunks_nb.checked_mul(HEAP_CHUNK_SIZE) {
                    Some(i) => i,
                    None => return null_mut(),
                };
                if grow_heap(self, increment).is_none() {
                    return null_mut();
                }
                match find_free_node_alloc(self, layout.size(), layout.align()){
                    Some((node, alloc)) => (node, alloc),
                    None => return null_mut(),
                }
            }
        };

        if add_free_block_before_and_after(self, free_node, alloc_ptr, layout.size()).is_none() {
            return null_mut();
        }

        alloc_ptr as *mut u8
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: core::alloc::Layout) {
        // TODO : add freelist for freeing ?
    }
}