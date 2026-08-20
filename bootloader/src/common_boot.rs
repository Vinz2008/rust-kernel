use crate::bootinfo::{BootInfo, FrameRange, MemoryMap};
use core::arch::asm;
use core::{convert::TryInto, panic::PanicInfo};
use core::mem;
use fixedvec::alloc_stack;
use x86_64::instructions::tlb;
use x86_64::structures::paging::PageSize;
use x86_64::structures::paging::{
    frame::PhysFrameRange, page_table::PageTableEntry, Mapper, Page, PageTable, PageTableFlags,
    PageTableIndex, PhysFrame, RecursivePageTable, Size2MiB, Size4KiB,
};
use x86_64::{PhysAddr, VirtAddr};
use crate::stack_chk::stack_chk_random;
use crate::firmware_regions::{cleanup_firmware_regions, reserve_firmware_boot_regions};
use crate::printer;

// The bootloader_config.rs file contains some configuration constants set by the build script:
// PHYSICAL_MEMORY_OFFSET: The offset into the virtual address space where the physical memory
// is mapped if the `map_physical_memory` feature is activated.
//
// KERNEL_STACK_ADDRESS: The virtual address of the kernel stack.
//
// KERNEL_STACK_SIZE: The number of pages in the kernel stack.
include!(concat!(env!("OUT_DIR"), "/bootloader_config.rs"));


unsafe fn context_switch(boot_info: VirtAddr, entry_point: VirtAddr, stack_pointer: VirtAddr) -> ! {
    asm!("mov rsp, {1}; call {0}; 2: jmp 2b",
         in(reg) entry_point.as_u64(), in(reg) stack_pointer.as_u64(), in("rdi") boot_info.as_u64());
    ::core::hint::unreachable_unchecked()
}

// TODO : refactor this interface ?
pub fn bootloader_main(
    kernel : &'static [u8],
    memory_map: &mut MemoryMap,
    page_table_start: Option<PhysAddr>, // only for BIOS
    page_table_end: Option<PhysAddr>, // only for BIOS
    bootloader_start: PhysAddr,
    bootloader_end: PhysAddr,
    p4_physical: PhysAddr,
) -> ! {
    use crate::bootinfo::{MemoryRegion, MemoryRegionType};
    use fixedvec::FixedVec;
    use xmas_elf::program::{ProgramHeader, ProgramHeader64};

    printer::Printer.clear_screen();

    let max_phys_addr = memory_map
        .iter()
        .map(|r| r.range.end_addr())
        .max()
        .expect("no physical memory regions found");

    // Extract required information from the ELF file.
    // TODO : not use the fixedVec to reduce stack usage ?
    let mut preallocated_space = alloc_stack!([ProgramHeader64; 32]);
    let mut segments = FixedVec::new(&mut preallocated_space);
    let entry_point;
    {
        let elf_file = xmas_elf::ElfFile::new(kernel).unwrap();
        xmas_elf::header::sanity_check(&elf_file).unwrap();

        entry_point = elf_file.header.pt2.entry_point();

        for program_header in elf_file.program_iter() {
            match program_header {
                ProgramHeader::Ph64(header) => segments
                    .push(*header)
                    .expect("does not support more than 32 program segments"),
                ProgramHeader::Ph32(_) => panic!("does not support 32 bit elf files"),
            }
        }
    }

    // Mark used virtual addresses
    let mut level4_entries = crate::level4_entries::UsedLevel4Entries::new(&segments);

    // Enable support for the no-execute bit in page tables.
    enable_nxe_bit();

    // Create a recursive page table entry
    let recursive_index =
        PageTableIndex::new(level4_entries.get_free_entries(1).try_into().unwrap());
    let mut entry = PageTableEntry::new();
    entry.set_addr(
        p4_physical,
        PageTableFlags::PRESENT | PageTableFlags::WRITABLE,
    );

    // Write the recursive entry into the page table
    let page_table = unsafe { &mut *(p4_physical.as_u64() as *mut PageTable) };
    page_table[recursive_index] = entry;
    tlb::flush_all();

    let recursive_page_table_addr = Page::from_page_table_indices(
        recursive_index,
        recursive_index,
        recursive_index,
        recursive_index,
    )
    .start_address();
    let page_table = unsafe { &mut *(recursive_page_table_addr.as_mut_ptr()) };
    let mut rec_page_table =
        RecursivePageTable::new(page_table).expect("recursive page table creation failed");

    // Create a frame allocator, which marks allocated frames as used in the memory map.
    let mut frame_allocator = crate::frame_allocator::FrameAllocator {
        memory_map: memory_map,
    };

    // Mark already used memory areas in frame allocator.
    {
        let zero_frame: PhysFrame = PhysFrame::from_start_address(PhysAddr::new(0)).unwrap();
        frame_allocator.mark_allocated_region(MemoryRegion {
            range: frame_range(PhysFrame::range(zero_frame, zero_frame + 1)),
            region_type: MemoryRegionType::FrameZero,
        });
        let bootloader_start_frame = PhysFrame::containing_address(bootloader_start);
        let bootloader_end_frame = PhysFrame::containing_address(bootloader_end - 1u64);
        let bootloader_memory_area =
            PhysFrame::range(bootloader_start_frame, bootloader_end_frame + 1);
        frame_allocator.mark_allocated_region(MemoryRegion {
            range: frame_range(bootloader_memory_area),
            region_type: MemoryRegionType::Bootloader,
        });
        
    
        reserve_firmware_boot_regions(&mut frame_allocator, kernel, page_table_start, page_table_end);
    }

    // Map a page for the boot info structure
    let boot_info_start_page = {
        let page: Page = match BOOT_INFO_ADDRESS {
            Some(addr) => Page::containing_address(VirtAddr::new(addr)),
            None => Page::from_page_table_indices(
                level4_entries.get_free_entries(1),
                PageTableIndex::new(0),
                PageTableIndex::new(0),
                PageTableIndex::new(0),
            ),
        };
        
        page
    };

    let boot_info_page_count = size_of::<BootInfo>().div_ceil(Size4KiB::SIZE as usize);
    for i in 0..boot_info_page_count {
        let frame = frame_allocator
            .allocate_frame(MemoryRegionType::BootInfo)
            .expect("frame allocation failed");
        let page = boot_info_start_page + i as u64;
        let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;
        unsafe {
            crate::page_table::map_page(
                page,
                frame,
                flags,
                &mut rec_page_table,
                &mut frame_allocator,
            )
        }
        .expect("Mapping of bootinfo page failed")
        .flush();
    }

    // If no kernel stack address is provided, map the kernel stack after the boot info page
    let kernel_stack_address = match KERNEL_STACK_ADDRESS {
        Some(addr) => Page::containing_address(VirtAddr::new(addr)),
        None => boot_info_start_page + 1,
    };

    // Map kernel segments.
    let kernel_memory_info = crate::page_table::map_kernel(
        kernel,
        kernel_stack_address,
        KERNEL_STACK_SIZE,
        &segments,
        &mut rec_page_table,
        &mut frame_allocator,
    )
    .expect("kernel mapping failed");

    cleanup_firmware_regions(&mut rec_page_table, kernel);

    let physical_memory_offset = if cfg!(feature = "map_physical_memory") {
        let physical_memory_offset = PHYSICAL_MEMORY_OFFSET.unwrap_or_else(|| {
            const LEVEL_4_SIZE: u64 = 4096 * 512 * 512 * 512;
            let level_4_entries = (max_phys_addr + (LEVEL_4_SIZE - 1)) / LEVEL_4_SIZE;
            Page::from_page_table_indices_1gib(
                level4_entries.get_free_entries(level_4_entries),
                PageTableIndex::new(0),
            )
            .start_address()
            .as_u64()
        });

        let virt_for_phys =
            |phys: PhysAddr| -> VirtAddr { VirtAddr::new(phys.as_u64() + physical_memory_offset) };

        let start_frame = PhysFrame::<Size2MiB>::containing_address(PhysAddr::new(0));
        let end_frame = PhysFrame::<Size2MiB>::containing_address(PhysAddr::new(max_phys_addr));

        for frame in PhysFrame::range_inclusive(start_frame, end_frame) {
            let page = Page::containing_address(virt_for_phys(frame.start_address()));
            let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::GLOBAL;
            unsafe {
                crate::page_table::map_page(
                    page,
                    frame,
                    flags,
                    &mut rec_page_table,
                    &mut frame_allocator,
                )
            }
            .expect("Mapping of bootinfo page failed")
            .flush();
        }

        physical_memory_offset
    } else {
        0 // Value is unused by BootInfo::new, so this doesn't matter
    };

    let guard = stack_chk_random().expect("no entropy available for stack canary") & 0xffff_ffff_ffff_ff00;

    let memory_map = frame_allocator.memory_map;

    // Construct boot info structure.
    let mut boot_info = BootInfo::new(
        memory_map,
        kernel_memory_info.tls_segment,
        recursive_page_table_addr.as_u64(),
        physical_memory_offset,
        guard
    );
    boot_info.memory_map.sort();

    // Write boot info to boot info page.
    let boot_info_addr = boot_info_start_page.start_address();
    unsafe { boot_info_addr.as_mut_ptr::<BootInfo>().write(boot_info) };

    // Make sure that the kernel respects the write-protection bits, even when in ring 0.
    // TODO : remove the cfg ?
    #[cfg(not(feature = "uefi"))]
    enable_write_protect_bit();

    enable_global_bit();

    if cfg!(not(feature = "recursive_page_table")) {
        // unmap recursive entry
        rec_page_table
            .unmap(Page::<Size4KiB>::containing_address(
                recursive_page_table_addr,
            ))
            .expect("error deallocating recursive entry")
            .1
            .flush();
        mem::drop(rec_page_table);
    }

    #[cfg(feature = "sse")]
    crate::sse::enable_sse();

    let entry_point = VirtAddr::new(entry_point);
    unsafe { context_switch(boot_info_addr, entry_point, kernel_memory_info.stack_end) };
}

fn enable_nxe_bit() {
    use x86_64::registers::control::{Efer, EferFlags};
    unsafe { Efer::update(|efer| *efer |= EferFlags::NO_EXECUTE_ENABLE) }
}

fn enable_write_protect_bit() {
    use x86_64::registers::control::{Cr0, Cr0Flags};
    unsafe { Cr0::update(|cr0| *cr0 |= Cr0Flags::WRITE_PROTECT) };
}

fn enable_global_bit(){
    use x86_64::registers::control::{Cr4, Cr4Flags};
    unsafe { Cr4::update(|flags| flags.insert(Cr4Flags::PAGE_GLOBAL)) };
}

#[panic_handler]
pub fn panic(info: &PanicInfo) -> ! {
    use core::fmt::Write;
    write!(printer::Printer, "{}", info).unwrap();
    loop {}
}

#[no_mangle]
pub extern "C" fn _Unwind_Resume() {
    loop {}
}

pub fn phys_frame_range(range: FrameRange) -> PhysFrameRange {
    PhysFrameRange {
        start: PhysFrame::from_start_address(PhysAddr::new(range.start_addr())).unwrap(),
        end: PhysFrame::from_start_address(PhysAddr::new(range.end_addr())).unwrap(),
    }
}

pub fn frame_range(range: PhysFrameRange) -> FrameRange {
    FrameRange::new(
        range.start.start_address().as_u64(),
        range.end.start_address().as_u64(),
    )
}
