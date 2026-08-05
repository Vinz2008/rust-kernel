use alloc::vec;
use alloc::vec::{Vec};
use bootloader::bootinfo::MemoryRegionType;
use linked_list_allocator::LockedHeap;
use spin::{Mutex, Once};
use x86_64::align_down;
use x86_64::structures::paging::FrameDeallocator;
use x86_64::structures::paging::mapper::MapperFlush;
use x86_64::{PhysAddr, VirtAddr, align_up, structures::paging::{FrameAllocator, Mapper, OffsetPageTable, Page, PageSize, PageTable, PageTableFlags, PhysFrame, Size2MiB, Size4KiB, Translate, mapper::{MapToError, TranslateResult}}};

use crate::paging::reload_cr3;
use crate::serial_println;
use crate::{paging::{BootInfoFrameAllocator, PHYSICAL_MEMORY_OFFSET, active_level_4_table}};

pub const KERNEL_HEAP_START: usize = 0xffff_9000_0000_0000;
pub const KERNEL_HEAP_SIZE: usize = 32 * 1024 * 1024; // 32MB, if needed, increase it

#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty(); 

pub struct MemoryManager {
    pub frame_allocator : BitMapFrameAllocator,
}

pub static MEMORY_MANAGER: Once<Mutex<MemoryManager>> = Once::new();


fn init_heap_mapping<F>(mapper: &mut impl Mapper<Size2MiB>, boot_frame_allocator : &mut F) -> Result<(), MapToError<Size2MiB>>
    where F : FrameAllocator<Size4KiB> + FrameAllocator<Size2MiB>
{
    let heap_start = VirtAddr::new(KERNEL_HEAP_START as u64);
    let heap_end = heap_start + (KERNEL_HEAP_SIZE as u64) - 1;
    let heap_start_page = Page::<Size2MiB>::containing_address(heap_start);
    let heap_end_page = Page::<Size2MiB>::containing_address(heap_end);
    let page_range = Page::range_inclusive(heap_start_page, heap_end_page);

    for page in page_range {
        let frame = boot_frame_allocator.allocate_frame().ok_or(MapToError::FrameAllocationFailed)?;
        let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::NO_EXECUTE;
        unsafe {
            mapper.map_to(page, frame, flags, boot_frame_allocator)?.ignore();
        }
    }
    reload_cr3(); // to flush tlb, because of the ignore

    Ok(())
}

pub struct BitMapFrameAllocator {
    bitmap : Vec<u64>,
    frame_count : usize,
    next_hint : usize,
}

unsafe impl<S : PageSize> FrameAllocator<S> for BitMapFrameAllocator {
    fn allocate_frame(&mut self) -> Option<PhysFrame<S>> {
        let frame_count = (S::SIZE / Size4KiB::SIZE) as usize;
        let frame = self.allocate_contiguous(frame_count, frame_count)?;
        PhysFrame::<S>::from_start_address(frame.start_address()).ok()
    }
}

impl<S: PageSize> FrameDeallocator<S> for BitMapFrameAllocator {
    unsafe fn deallocate_frame(&mut self, frame: PhysFrame<S>) {
        let frame_start = frame.start_address().as_u64();
        self.mark_range_free(frame_start, frame_start + S::SIZE);

        let start_idx = (frame_start / Size4KiB::SIZE) as usize;
        self.next_hint = self.next_hint.min(start_idx);
    }
}


const PHYSMAP_SIZE : u64 = 512 * 1024 * 1024 * 1024;

fn init_bitmap_frame_allocator(mapper: &mut OffsetPageTable<'static>, boot_frame_allocator : BootInfoFrameAllocator) -> BitMapFrameAllocator {
    let highest_usable_end = 
        boot_frame_allocator.memory_map.iter().filter(|region| region.region_type == MemoryRegionType::Usable).map(|region| region.range.end_addr()).max().unwrap();

    debug_assert!(highest_usable_end <= PHYSMAP_SIZE, "usable physical memory exceeds the 512 GiB physmap");

    let frame_count = highest_usable_end.div_ceil(Size4KiB::SIZE) as usize;
    let bitmap_word_count = frame_count.div_ceil(u64::BITS as usize);

    let bitmap = vec![u64::MAX; bitmap_word_count];

    let bitmap_virt = VirtAddr::new(bitmap.as_ptr() as u64);

    serial_println!(
        "bitmap translation: {:?}",
        mapper.translate_addr(bitmap_virt),
    );

    let mut frame_alloc = BitMapFrameAllocator {
        bitmap,
        frame_count,
        next_hint: 0,
    };

    for (idx, region) in boot_frame_allocator.memory_map.iter().enumerate() {
        if region.region_type != MemoryRegionType::Usable {
            continue;
        }
        let region_start = align_up(region.range.start_addr(), Size4KiB::SIZE);
        let region_end = align_down(region.range.end_addr(), Size4KiB::SIZE);
    
        if region_start >= region_end {
            continue;
        }

        let free_start = if idx < boot_frame_allocator.region_idx {
            continue;
        } else if idx == boot_frame_allocator.region_idx {
            align_up(boot_frame_allocator.next_addr as u64, Size4KiB::SIZE).max(region_start)
        } else {
            region_start
        };

        if free_start < region_end {
            serial_println!(
                "boot region_idx={}, next_addr={:#x}; freeing region {}: {:#x}..{:#x}",
                boot_frame_allocator.region_idx,
                boot_frame_allocator.next_addr,
                idx,
                free_start,
                region_end,
            );
            frame_alloc.mark_range_free(free_start, region_end);
        }
    }

    frame_alloc.next_hint = frame_alloc.find_first_free_contiguous(1, 1).unwrap_or(frame_count);

    frame_alloc
}

impl BitMapFrameAllocator {
    fn find_free_in_contiguous(&self, start_idx : usize, end_idx : usize, page_nb : usize, alignement : usize) -> Option<usize> {
        if page_nb == 0 || alignement == 0 || !alignement.is_power_of_two() || start_idx >= end_idx {
            return None;
        }

        let mut candidate = align_up(start_idx as u64, alignement as u64) as usize;

        while candidate.checked_add(page_nb)? <= end_idx {
            let candidate_end = candidate + page_nb;
            match (candidate..candidate_end).find(|&frame_idx| self.is_frame_used(frame_idx)){
                Some(used_idx) => {
                    candidate = align_up(used_idx.checked_add(1)? as u64, alignement as u64) as usize;
                },
                None => return Some(candidate),
            }
        }

        None
    }

    fn is_frame_used(&self, frame_idx : usize) -> bool {
        debug_assert!(frame_idx < self.frame_count);

        const WORD_BITS: usize = u64::BITS as usize;

        let word_idx = frame_idx / WORD_BITS;


        let bit_idx = frame_idx % WORD_BITS;
        self.bitmap[word_idx] & ((1 as u64) << bit_idx) != 0
    }

    fn set_is_used(&mut self, frame_idx : usize, used : bool){
        debug_assert!(frame_idx < self.frame_count);
        const WORD_BITS: usize = u64::BITS as usize;

        let word_idx = frame_idx / WORD_BITS;
        let bit_idx = frame_idx % WORD_BITS;
        let mask = (1 as u64) << bit_idx;
        if used {
            self.bitmap[word_idx] |= mask;
        } else {
            self.bitmap[word_idx] &= !mask;
        }
    }

    fn find_first_free_contiguous(&self, page_nb : usize, alignement : usize) -> Option<usize> {
        self.find_free_in_contiguous(self.next_hint, self.frame_count, page_nb, alignement).or_else(|| self.find_free_in_contiguous(0, self.next_hint, page_nb, alignement))
    }

    fn allocate_contiguous(&mut self, page_nb : usize, alignement : usize) -> Option<PhysFrame<Size4KiB>> {
        let first_idx = self.find_first_free_contiguous(page_nb, alignement)?;
        let last_idx = first_idx.checked_add(page_nb)?;

        for frame_idx in first_idx..last_idx {
            self.set_is_used(frame_idx, true);
        }

        self.next_hint = if last_idx == self.frame_count {
            0
        } else {
            last_idx
        };

        let addr = (first_idx as u64).checked_mul(Size4KiB::SIZE)?;
        let phys_frame = PhysFrame::containing_address(PhysAddr::new(addr));

        Some(phys_frame)
    }

    fn mark_range_free(&mut self, start : u64, end : u64){
        const PAGE_SIZE: u64 = Size4KiB::SIZE;
        
        debug_assert_eq!(start % PAGE_SIZE, 0);
        debug_assert_eq!(end % PAGE_SIZE, 0);
        debug_assert!(start <= end);

        let start_idx = (start / PAGE_SIZE) as usize;
        let end_idx = (end / PAGE_SIZE) as usize;

        for frame_idx in start_idx..end_idx {
            self.set_is_used(frame_idx, false);
        }

        self.next_hint = self.next_hint.min(start_idx);
    }
}

pub fn init_heap(mut mapper: OffsetPageTable<'static>, mut boot_frame_allocator : BootInfoFrameAllocator) -> Result<(), MapToError<Size2MiB>> {
    init_heap_mapping(&mut mapper, &mut boot_frame_allocator)?; 
    
    unsafe {
        ALLOCATOR.lock().init(KERNEL_HEAP_START as *mut u8, KERNEL_HEAP_SIZE);
    }
    
    let frame_allocator = init_bitmap_frame_allocator(&mut mapper, boot_frame_allocator);

    MEMORY_MANAGER.call_once(|| Mutex::new(MemoryManager {
        frame_allocator,
    }));

    Ok(())
}

fn map_page_inner(mapper : &mut OffsetPageTable<'_>, frame_allocator : &mut BitMapFrameAllocator, phys_frame : PhysFrame, virt_addr: VirtAddr, flags: PageTableFlags) -> Result<MapperFlush<Size4KiB>, MapToError<Size4KiB>> {
    let page = Page::containing_address(virt_addr);
    
    let flush = unsafe {
        mapper.map_to(page, phys_frame, flags, frame_allocator)?
    };
    Ok(flush)
}

pub fn get_page_flags_in(mapper : &mut OffsetPageTable<'_>, virt_addr: VirtAddr) -> Option<PageTableFlags> {
    match mapper.translate(virt_addr){
        TranslateResult::Mapped { frame: _, offset: _, flags } => Some(flags),
        TranslateResult::NotMapped | TranslateResult::InvalidFrameAddress(_) => None
    }
}

fn _map_page_phys_at_in(mem_manager_lock : &mut MemoryManager, page_table : PhysAddr, phys_frame : PhysFrame, virt_addr: VirtAddr, flags: PageTableFlags) -> Result<MapperFlush<Size4KiB>, MapToError<x86_64::structures::paging::Size4KiB>> {
    let phys_offset = PHYSICAL_MEMORY_OFFSET;
    let page_table_virt = phys_offset + page_table.as_u64();
    let page_table_ptr: *mut PageTable = page_table_virt.as_mut_ptr();
    let page_table = unsafe { &mut *page_table_ptr };
    let mut mapper = unsafe { OffsetPageTable::new(page_table, phys_offset) };

    map_page_inner(&mut mapper, &mut mem_manager_lock.frame_allocator, phys_frame, virt_addr, flags)
}

pub fn map_page_phys_at_in(page_table : PhysAddr, phys_frame : PhysFrame, virt_addr: VirtAddr, flags: PageTableFlags) -> Result<MapperFlush<Size4KiB>, MapToError<x86_64::structures::paging::Size4KiB>> {
    let mut mem_manager_lock = MEMORY_MANAGER.get().unwrap().lock();
    _map_page_phys_at_in(&mut mem_manager_lock, page_table, phys_frame, virt_addr, flags)
}

pub fn map_page_at_in(page_table : PhysAddr, virt_addr: VirtAddr, flags: PageTableFlags) -> Result<MapperFlush<Size4KiB>, MapToError<x86_64::structures::paging::Size4KiB>>{
    let mut mem_manager_lock = MEMORY_MANAGER.get().unwrap().lock();

    let phys_frame = mem_manager_lock.frame_allocator.allocate_frame().expect("no frame available");

    _map_page_phys_at_in(&mut mem_manager_lock, page_table, phys_frame, virt_addr, flags)
}


pub fn pml4_index(addr: u64) -> usize {
    ((addr >> 39) & 0x1ff) as usize
}

// TODO : better error handling ?
// TODO : handle page of other size than 4KiB
pub fn deallocate_virtual_page(page_table_frame : PhysFrame, page : Page){
    let phys_offset = PHYSICAL_MEMORY_OFFSET;
    let page_table_virt = phys_offset + page_table_frame.start_address().as_u64();
    let page_table_ptr: *mut PageTable = page_table_virt.as_mut_ptr();
    let page_table = unsafe { &mut *page_table_ptr };
    let mut mapper = unsafe { OffsetPageTable::new(page_table, phys_offset) };
    let (phys_frame, flush) = mapper.unmap(page).unwrap();
    flush.flush();
    
    serial_println!(
        "unmap virt={:#x} -> frame={:#x}",
        page.start_address().as_u64(),
        phys_frame.start_address().as_u64(),
    );

    unsafe {
        MEMORY_MANAGER.get().unwrap().lock().frame_allocator.deallocate_frame(phys_frame);
    }
}

pub fn allocate_userspace_level_4_table() -> PhysFrame {
    
    let current_page_table = unsafe { active_level_4_table() };
    let new_table_frame = MEMORY_MANAGER.get().unwrap().lock().frame_allocator.allocate_frame().unwrap();
    let new_table_phys = new_table_frame.start_address();
    let new_table_virt = PHYSICAL_MEMORY_OFFSET + new_table_phys.as_u64();
    let page_table_ptr: *mut PageTable = new_table_virt.as_mut_ptr();

    let page_table = unsafe { &mut *page_table_ptr };
    page_table.zero();
    
    for i in 256..512 {
        page_table[i] = current_page_table[i].clone();
    }

    new_table_frame
}

pub fn deallocate_userspace_level_4_table(phys_frame : PhysFrame){
    unsafe {
        let mut mem_manager_lock = MEMORY_MANAGER.get().unwrap().lock();
        mem_manager_lock.frame_allocator.deallocate_frame(phys_frame);
    }
}