use linked_list_allocator::LockedHeap;
use spin::{Mutex, Once};
use x86_64::{PhysAddr, VirtAddr, structures::paging::{FrameAllocator, Mapper, OffsetPageTable, Page, PageTable, PageTableFlags, PhysFrame, Size4KiB, Translate, mapper::{MapToError, TranslateResult}}};

use crate::paging::{BootInfoFrameAllocator, PHYSICAL_MEMORY_OFFSET, active_level_4_table};

pub const HEAP_START: usize = 0x_4444_4444_0000;
pub const HEAP_SIZE: usize = 10 * 1024 * 1024; // 10MB, if needed, increase it

#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty(); 

pub struct MemoryManager {
    mapper: OffsetPageTable<'static>,
    frame_allocator : BootInfoFrameAllocator,
}

pub static MEMORY_MANAGER: Once<Mutex<MemoryManager>> = Once::new();


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

    pub fn get_page_flags(&self, virt_addr: VirtAddr) -> Option<PageTableFlags> {
        match self.mapper.translate(virt_addr){
            TranslateResult::Mapped { frame, offset, flags } => Some(flags),
            TranslateResult::NotMapped | TranslateResult::InvalidFrameAddress(_) => None
        }
    }
}

// allocate physical frames and map them to the address
pub fn map_page_at(virt_addr: VirtAddr, flags: PageTableFlags){
    let mut mem_manager_lock = MEMORY_MANAGER.get().unwrap().lock();
    mem_manager_lock.map_page_at(virt_addr, flags);
}


pub fn allocate_userspace_level_4_table() -> PhysFrame {
    let physical_memory_offset = *PHYSICAL_MEMORY_OFFSET.get().unwrap();
    let kernel_page_table = unsafe { active_level_4_table() };
    let new_table_frame = MEMORY_MANAGER.get().unwrap().lock().frame_allocator.allocate_frame().unwrap();
    let new_table_phys = new_table_frame.start_address();
    let new_table_virt = physical_memory_offset + new_table_phys.as_u64();
    let page_table_ptr: *mut PageTable = new_table_virt.as_mut_ptr();

    let page_table = unsafe { &mut *page_table_ptr };
    page_table.zero();
    for i in 256..512 {
        page_table[i] = kernel_page_table[i].clone();
    }

    new_table_frame
}