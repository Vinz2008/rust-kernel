use bootloader::{bootinfo::{MemoryMap, MemoryRegionType}};
use spin::Once;
use x86_64::{PhysAddr, VirtAddr, align_up, registers::control::Cr3, structures::paging::{FrameAllocator, OffsetPageTable, PageSize, PageTable, PhysFrame, page_table::FrameError}};


pub static PHYSICAL_MEMORY_OFFSET : Once<VirtAddr> = Once::new();

pub unsafe fn active_level_4_table() -> &'static mut PageTable {
    let (level_4_table_frame, _) = Cr3::read();
    let phys = level_4_table_frame.start_address();
    let virt = *PHYSICAL_MEMORY_OFFSET.get().unwrap() + phys.as_u64();
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
    let phys_off = *PHYSICAL_MEMORY_OFFSET.get().unwrap();
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

pub unsafe fn init(physical_memory_offset: VirtAddr) -> OffsetPageTable<'static> {
    unsafe {
        PHYSICAL_MEMORY_OFFSET.call_once(|| physical_memory_offset);
        let level_4_table = active_level_4_table();
        OffsetPageTable::new(level_4_table, physical_memory_offset)
    }
}

// TODO : only use it the current page strategy to allocate the bitmap for better frame allocation which support frame deallocation
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

    /*fn usable_frames(&self) -> impl Iterator<Item = PhysFrame> {
        let regions = self.memory_map.iter();
        let usable_regions = regions.filter(|r| r.region_type == MemoryRegionType::Usable);
        let addr_ranges = usable_regions.map(|r| r.range.start_addr()..r.range.end_addr());
        
        let frame_addresses = addr_ranges.flat_map(|r| r.step_by(PAGE_SIZE));
        frame_addresses.map(|addr| PhysFrame::containing_address(PhysAddr::new(addr)))
    }*/

    // TODO : remove this
    /*pub fn get_memory_map_pml4_index(&self) -> usize {
        pml4_index(self.memory_map as *const _ as u64)
    }*/
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