use bootloader::{bootinfo::{MemoryMap, MemoryRegionType}};
use x86_64::{PhysAddr, VirtAddr, align_up, registers::control::Cr3, structures::paging::{FrameAllocator, Mapper, OffsetPageTable, Page, PageSize, PageTable, PageTableFlags, PhysFrame, Size4KiB, Translate, mapper::{MapToError, MapperFlush, TranslateResult}, page_table::FrameError}};

use crate::allocator::{BitMapFrameAllocator, MEMORY_MANAGER, MemoryManager};


pub const PHYSICAL_MEMORY_OFFSET : VirtAddr = VirtAddr::new(0xFFFFFE0000000000);

pub unsafe fn active_level_4_table() -> &'static mut PageTable {
    let (level_4_table_frame, _) = Cr3::read();
    let phys = level_4_table_frame.start_address();
    let virt = PHYSICAL_MEMORY_OFFSET + phys.as_u64();
    let page_table_ptr: *mut PageTable = virt.as_mut_ptr();

    unsafe { &mut *page_table_ptr }
}

pub fn reload_cr3(){
    let (active_p4, cr3_flags) = Cr3::read();

    unsafe {
        Cr3::write(active_p4, cr3_flags);
    }
}


pub unsafe fn translate_addr(addr: VirtAddr) -> Option<PhysAddr> {
    let (level_4_table_frame, _) = Cr3::read();
    unsafe {
        translate_addr_in(level_4_table_frame, addr)
    }
}

pub unsafe fn translate_addr_in(page_table_frame : PhysFrame, addr: VirtAddr) -> Option<PhysAddr> {
    let table_indexes = [
        addr.p4_index(), addr.p3_index(), addr.p2_index(), addr.p1_index()
    ];
    let mut frame = page_table_frame;
    let phys_off = PHYSICAL_MEMORY_OFFSET;
    for &index in &table_indexes {
        let virt = phys_off + frame.start_address().as_u64();
        let table_ptr: *const PageTable = virt.as_ptr();
        let table = unsafe {&*table_ptr};
        let entry = &table[index];
        frame = match entry.frame() {
            Ok(frame) => frame,
            Err(FrameError::FrameNotPresent) => return None,
            Err(FrameError::HugeFrame) => panic!("huge pages not supported"),
        };
    }
    Some(frame.start_address() + u64::from(addr.page_offset()))
}

fn map_page_inner(mapper : &mut OffsetPageTable<'_>, frame_allocator : &mut BitMapFrameAllocator, phys_frame : PhysFrame, virt_addr: VirtAddr, flags: PageTableFlags) -> Result<MapperFlush<Size4KiB>, MapToError<Size4KiB>> {
    let page = Page::containing_address(virt_addr);
    
    let flush = unsafe {
        mapper.map_to(page, phys_frame, flags, frame_allocator)?
    };
    Ok(flush)
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

pub fn get_page_flags_in(mapper : &mut OffsetPageTable<'_>, virt_addr: VirtAddr) -> Option<PageTableFlags> {
    match mapper.translate(virt_addr){
        TranslateResult::Mapped { frame: _, offset: _, flags } => Some(flags),
        TranslateResult::NotMapped | TranslateResult::InvalidFrameAddress(_) => None
    }
}

// TODO : better error handling
pub fn set_page_flags_in(mapper : &mut OffsetPageTable<'_>, virt_addr: VirtAddr, flags : PageTableFlags) -> MapperFlush<Size4KiB> {
    let page = Page::<Size4KiB>::containing_address(virt_addr);
    unsafe {
        mapper.update_flags(page, flags).unwrap()
    }
}

pub unsafe fn init() -> OffsetPageTable<'static> {
    unsafe {
        let level_4_table = active_level_4_table();
        OffsetPageTable::new(level_4_table, PHYSICAL_MEMORY_OFFSET)
    }
}

// TODO huge pages (2 MiB)

pub struct BootInfoFrameAllocator {
    pub memory_map: &'static MemoryMap,
    pub region_idx : usize,
    pub next_addr: usize,
}

impl BootInfoFrameAllocator {
    pub unsafe fn init(memory_map : &'static MemoryMap) -> BootInfoFrameAllocator {
        BootInfoFrameAllocator {
            memory_map,
            region_idx: 0,
            next_addr: 0,
        }
    }
}


unsafe impl<S : PageSize> FrameAllocator<S> for BootInfoFrameAllocator {
    fn allocate_frame(&mut self) -> Option<PhysFrame<S>> {
        let size = S::SIZE;
        loop {
            let region = *self.memory_map.get(self.region_idx)?;
            if region.region_type != MemoryRegionType::Usable {
                self.region_idx += 1;
                self.next_addr = 0;
                continue;
            }
            let candidate = if self.next_addr == 0 {
                align_up(region.range.start_addr(), size)
            } else {
                align_up(self.next_addr as u64, size)
            };
            
            let frame_end = candidate.checked_add(size)?;


            let region_end = region.range.end_addr();
            if frame_end <= region_end {
                self.next_addr = frame_end as usize;
                return PhysFrame::<S>::from_start_address(PhysAddr::new(candidate)).ok();
            }
            
            self.region_idx += 1;
            self.next_addr = 0;
        }
    }
}