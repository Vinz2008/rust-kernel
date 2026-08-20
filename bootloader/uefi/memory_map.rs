use bootloader::bootinfo::MemoryRegion;
use uefi::mem::memory_map::{MemoryMapOwned, MemoryMap};

pub fn create_memory_map(uefi_mem_map : MemoryMapOwned, output: *mut bootloader::bootinfo::MemoryMap) -> &'static mut bootloader::bootinfo::MemoryMap {
    unsafe {
        bootloader::bootinfo::MemoryMap::init_at(output);
        
        let memory_map = &mut *output;
        for region in uefi_mem_map.entries() {
            memory_map.add_region(
                MemoryRegion::from(*region)
            );
        }

        memory_map.sort();

        memory_map
    }    
}