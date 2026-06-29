use bootloader::{bootinfo::{MemoryMap, MemoryRegionType}};
use spin::Once;
use x86_64::{PhysAddr, VirtAddr, registers::control::Cr3, structures::paging::{FrameAllocator, OffsetPageTable, PageTable, PhysFrame, Size4KiB, page_table::FrameError}};

use crate::allocator::pml4_index;


pub static PHYSICAL_MEMORY_OFFSET : Once<VirtAddr> = Once::new();

pub unsafe fn active_level_4_table() -> &'static mut PageTable {
    let (level_4_table_frame, _) = Cr3::read();
    let phys = level_4_table_frame.start_address();
    let virt = *PHYSICAL_MEMORY_OFFSET.get().unwrap() + phys.as_u64();
    let page_table_ptr: *mut PageTable = virt.as_mut_ptr();

    unsafe { &mut *page_table_ptr }
}


pub unsafe fn translate_addr(addr: VirtAddr) -> Option<PhysAddr> {
    let (level_4_table_frame, _) = Cr3::read();
    translate_addr_in(level_4_table_frame, addr)
}

pub unsafe fn translate_addr_in(page_table_frame : PhysFrame, addr: VirtAddr) -> Option<PhysAddr> {
    let table_indexes = [
        addr.p4_index(), addr.p3_index(), addr.p2_index(), addr.p1_index()
    ];
    let mut frame = page_table_frame;
    for &index in &table_indexes {
        let virt = *PHYSICAL_MEMORY_OFFSET.get().unwrap() + frame.start_address().as_u64();
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

pub unsafe fn init(physical_memory_offset: VirtAddr) -> OffsetPageTable<'static> {
    unsafe {
        PHYSICAL_MEMORY_OFFSET.call_once(|| physical_memory_offset);
        let level_4_table = active_level_4_table();
        OffsetPageTable::new(level_4_table, physical_memory_offset)
    }
}

// TODO huge pages (2 MiB)

pub struct BootInfoFrameAllocator {
    memory_map: &'static MemoryMap,
    next: usize,
}

impl BootInfoFrameAllocator {
    pub unsafe fn init(memory_map : &'static MemoryMap) -> BootInfoFrameAllocator {
        BootInfoFrameAllocator {
            memory_map,
            next: 0
        }
    }

    fn usable_frames(&self) -> impl Iterator<Item = PhysFrame> {
        let regions = self.memory_map.iter();
        let usable_regions = regions.filter(|r| r.region_type == MemoryRegionType::Usable);
        let addr_ranges = usable_regions.map(|r| r.range.start_addr()..r.range.end_addr());
        const PAGE_SIZE : usize = 4096; // TODO : change this when using huge pages (pass through args ?)
        let frame_addresses = addr_ranges.flat_map(|r| r.step_by(PAGE_SIZE));
        frame_addresses.map(|addr| PhysFrame::containing_address(PhysAddr::new(addr)))
    }

    pub fn get_memory_map_pml4_index(&self) -> usize {
        pml4_index(self.memory_map as *const _ as u64)
    }
}

unsafe impl FrameAllocator<Size4KiB> for BootInfoFrameAllocator {
    fn allocate_frame(&mut self) -> Option<PhysFrame> {
        let frame = self.usable_frames().nth(self.next); // TODO : cache the usable frames iter (according to tutorial phil-opp.cpp not possible because of lack of named existential types), or I could rework the api to not return an iter ?
        self.next += 1;
        frame
    }
}