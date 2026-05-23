use linked_list_allocator::LockedHeap;
use x86_64::{VirtAddr, structures::paging::{FrameAllocator, Mapper, Page, PageTableFlags, Size4KiB, mapper::MapToError}};

pub const HEAP_START: usize = 0x_4444_4444_0000;
pub const HEAP_SIZE: usize = 100 * 1024; // if needed, increase it

#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();

fn init_heap_mapping(mapper: &mut impl Mapper<Size4KiB>, frame_allocator : &mut impl FrameAllocator<Size4KiB>) -> Result<(), MapToError<Size4KiB>>{
    let heap_start = VirtAddr::new(HEAP_START as u64);
    let heap_end = heap_start + (HEAP_SIZE as u64) - 1;
    let heap_start_page = Page::containing_address(heap_start);
    let heap_end_page = Page::containing_address(heap_end);
    let page_range = Page::range_inclusive(heap_start_page, heap_end_page);

    for page in page_range {
        let frame = frame_allocator.allocate_frame().ok_or(MapToError::FrameAllocationFailed)?;
        let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;
        unsafe {
            mapper.map_to(page, frame, flags, frame_allocator)?.flush();
        }
    }
    Ok(())
}

pub fn init_heap(mapper: &mut impl Mapper<Size4KiB>, frame_allocator : &mut impl FrameAllocator<Size4KiB>) -> Result<(), MapToError<Size4KiB>> {
    init_heap_mapping(mapper, frame_allocator)?;

    unsafe {
        ALLOCATOR.lock().init(HEAP_START as *mut u8, HEAP_SIZE);
    }

    Ok(())
}