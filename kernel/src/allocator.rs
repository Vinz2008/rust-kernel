use linked_list_allocator::LockedHeap;
use spin::{Mutex, Once};
use x86_64::{VirtAddr, structures::paging::{FrameAllocator, Mapper, OffsetPageTable, Page, PageTableFlags, Size4KiB, mapper::MapToError}};

use crate::paging::BootInfoFrameAllocator;

pub const HEAP_START: usize = 0x_4444_4444_0000;
pub const HEAP_SIZE: usize = 10 * 1024 * 1024; // 10MB, if needed, increase it

#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty(); 

struct MemoryManager {
    mapper: OffsetPageTable<'static>,
    frame_allocator : BootInfoFrameAllocator,
}

static MEMORY_MANAGER: Once<Mutex<MemoryManager>> = Once::new();


fn init_heap_mapping(mapper: &mut impl Mapper<Size4KiB>, frame_allocator : &mut impl FrameAllocator<Size4KiB>) -> Result<(), MapToError<Size4KiB>>{
    let heap_start = VirtAddr::new(HEAP_START as u64);
    let heap_end = heap_start + (HEAP_SIZE as u64) - 1;
    let heap_start_page = Page::containing_address(heap_start);
    let heap_end_page = Page::containing_address(heap_end);
    let page_range = Page::range_inclusive(heap_start_page, heap_end_page);

    for page in page_range {
        let frame = frame_allocator.allocate_frame().ok_or(MapToError::FrameAllocationFailed)?;
        let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::NO_EXECUTE;
        unsafe {
            mapper.map_to(page, frame, flags, frame_allocator)?.flush();
        }
    }
    Ok(())
}

pub fn init_heap(mut mapper: OffsetPageTable<'static>, mut frame_allocator : BootInfoFrameAllocator) -> Result<(), MapToError<Size4KiB>> {
    init_heap_mapping(&mut mapper, &mut frame_allocator)?;
    MEMORY_MANAGER.call_once(|| Mutex::new(MemoryManager {
        mapper,
        frame_allocator,
    }));

    unsafe {
        ALLOCATOR.lock().init(HEAP_START as *mut u8, HEAP_SIZE);
    }

    Ok(())
}

impl MemoryManager {
    fn map_page_at(&mut self, virt_addr: VirtAddr, flags: PageTableFlags){
        let page = Page::containing_address(virt_addr);
        let phys_frame = self.frame_allocator.allocate_frame().expect("no frame available");
        unsafe {
            self.mapper.map_to(page, phys_frame, flags, &mut self.frame_allocator).expect("error when mapping page").flush();
        }
    }
}

// allocate physical frames and map them to the address
pub fn map_page_at(virt_addr: VirtAddr, flags: PageTableFlags){
    let mut mem_manager_lock = MEMORY_MANAGER.get().unwrap().lock();
    mem_manager_lock.map_page_at(virt_addr, flags);
}