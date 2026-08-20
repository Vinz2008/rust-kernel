#![no_std]
#![no_main]

use bootloader::{bootinfo::MemoryMap, common_boot::bootloader_main};
use crate::memory_map::MemoryMapStorage;
use core::cell::UnsafeCell;

#[cfg(not(target_os = "none"))]
compile_error!("The bootloader crate must be compiled for the `x86_64-bootloader.json` target");

use core::{arch::{asm, global_asm}, slice};
use x86_64::{PhysAddr, VirtAddr};

global_asm!(include_str!("stage_1.s"));

global_asm!(include_str!("stage_2.s"));

global_asm!(include_str!("e820.s"));

global_asm!(include_str!("stage_3.s"));

#[cfg(feature = "vga_320x200")]
global_asm!(include_str!("video_mode/vga_320x200.s"));
#[cfg(not(feature = "vga_320x200"))]
global_asm!(include_str!("video_mode/vga_text_80x25.s"));

// Symbols defined in `linker.ld`
extern "C" {
    static mmap_ent: usize;
    static _memory_map: usize;
    static _kernel_start_addr: usize;
    static _kernel_end_addr: usize;
    static _kernel_size: usize;
    static __page_table_start: usize;
    static __page_table_end: usize;
    static __bootloader_end: usize;
    static __bootloader_start: usize;
    static _p4: usize;
}

mod memory_map;

static MEMORY_MAP_STORAGE: MemoryMapStorage = MemoryMapStorage(UnsafeCell::new(MemoryMap::const_new()));

#[no_mangle]
pub unsafe extern "C" fn stage_4() -> ! {
    // Set stack segment
    asm!(
        "push rbx
          mov bx, 0x0
          mov ss, bx
          pop rbx"
    );

    let kernel_start = 0x400000;
    let kernel_size = &_kernel_size as *const _ as u64;
    let memory_map_addr = &_memory_map as *const _ as u64;
    let memory_map_entry_count = (mmap_ent & 0xff) as u64; // Extract lower 8 bits
    let page_table_start = &__page_table_start as *const _ as u64;
    let page_table_end = &__page_table_end as *const _ as u64;
    let bootloader_start = &__bootloader_start as *const _ as u64;
    let bootloader_end = &__bootloader_end as *const _ as u64;
    let p4_physical = &_p4 as *const _ as u64;

    let kernel = slice::from_raw_parts(kernel_start as *const u8, kernel_size as usize);

    let memory_map_ptr = MEMORY_MAP_STORAGE.0.get();
    let memory_map = unsafe { &mut *memory_map_ptr };
    

    crate::memory_map::init_from(VirtAddr::new(memory_map_addr), memory_map_entry_count, memory_map);

    bootloader_main(
        kernel,
        memory_map,
        Some(PhysAddr::new(page_table_start)),
        Some(PhysAddr::new(page_table_end)),
        PhysAddr::new(bootloader_start),
        PhysAddr::new(bootloader_end),
        PhysAddr::new(p4_physical),
    )
}