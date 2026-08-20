use core::{slice, cell::UnsafeCell};

use bootloader::bootinfo::{E820MemoryRegion, MemoryMap, MemoryRegion, MemoryRegionType};
use usize_conversions::usize_from;
use x86_64::VirtAddr;

pub struct MemoryMapStorage(pub UnsafeCell<MemoryMap>);

unsafe impl Sync for MemoryMapStorage {}

pub(crate) fn init_from(memory_map_addr: VirtAddr, entry_count: u64, memory_map_output : *mut MemoryMap) {
    let memory_map_output = unsafe { &mut *memory_map_output };
    let memory_map_start_ptr = usize_from(memory_map_addr.as_u64()) as *const E820MemoryRegion;
    let e820_memory_map =
        unsafe { slice::from_raw_parts(memory_map_start_ptr, usize_from(entry_count)) };

    for region in e820_memory_map {
        memory_map_output.add_region(MemoryRegion::from(*region));
    }

    memory_map_output.sort();

    let mut iter = memory_map_output.iter_mut().peekable();
    while let Some(region) = iter.next() {
        if let Some(next) = iter.peek() {
            if region.range.end_frame_number > next.range.start_frame_number
                && region.region_type == MemoryRegionType::Usable
            {
                region.range.end_frame_number = next.range.start_frame_number;
            }
        }
    }
}
