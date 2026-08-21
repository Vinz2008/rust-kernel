use x86_64::PhysAddr;
use x86_64::structures::paging::RecursivePageTable;


use crate::frame_allocator::FrameAllocator;

#[cfg(feature = "uefi")]
pub fn reserve_firmware_boot_regions(_frame_allocator : &mut FrameAllocator<'_>, _kernel : &'static [u8], _page_table_start : Option<PhysAddr>, _page_table_end : Option<PhysAddr>, bootloader_start : PhysAddr, bootloader_end : PhysAddr){
    // nothing
}

#[cfg(not(feature = "uefi"))]
pub fn reserve_firmware_boot_regions(frame_allocator : &mut FrameAllocator<'_>, kernel : &'static [u8], page_table_start : Option<PhysAddr>, page_table_end : Option<PhysAddr>, bootloader_start : PhysAddr, bootloader_end : PhysAddr){
    use x86_64::PhysAddr;
    use x86_64::structures::paging::PhysFrame;
    use crate::common_boot::frame_range;
    use crate::bootinfo::{MemoryRegion, MemoryRegionType};

    // is it identity mapped
    let kernel_start_phys = PhysAddr::new(kernel.as_ptr() as u64);
    let kernel_size = kernel.len();
    let kernel_start_frame = PhysFrame::containing_address(kernel_start_phys);
    let kernel_end_frame = PhysFrame::containing_address(kernel_start_phys + kernel_size - 1u64);
    let kernel_memory_area = PhysFrame::range(kernel_start_frame, kernel_end_frame + 1);
    frame_allocator.mark_allocated_region(MemoryRegion {
        range: frame_range(kernel_memory_area),
        region_type: MemoryRegionType::Kernel,
    });

    
    let page_table_start = page_table_start.unwrap();
    let page_table_end = page_table_end.unwrap();
    let page_table_start_frame = PhysFrame::containing_address(page_table_start);
    let page_table_end_frame = PhysFrame::containing_address(page_table_end - 1u64);
    let page_table_memory_area = PhysFrame::range(page_table_start_frame, page_table_end_frame + 1);
    frame_allocator.mark_allocated_region(MemoryRegion {
        range: frame_range(page_table_memory_area),
        region_type: MemoryRegionType::PageTable,
    });

    let bootloader_start_frame = PhysFrame::containing_address(bootloader_start);
    let bootloader_end_frame = PhysFrame::containing_address(bootloader_end - 1u64);
    let bootloader_memory_area = PhysFrame::range(bootloader_start_frame, bootloader_end_frame + 1);
    frame_allocator.mark_allocated_region(MemoryRegion {
        range: frame_range(bootloader_memory_area),
        region_type: MemoryRegionType::Bootloader,
    });

    let zero_frame: PhysFrame = PhysFrame::from_start_address(PhysAddr::new(0)).unwrap();
    frame_allocator.mark_allocated_region(MemoryRegion {
        range: frame_range(PhysFrame::range(zero_frame, zero_frame + 1)),
        region_type: MemoryRegionType::FrameZero,
    });
}

#[cfg(feature = "uefi")]
pub fn cleanup_firmware_regions(_rec_page_table: &mut RecursivePageTable, _kernel : &'static [u8]){
    // nothing
}

#[cfg(not(feature = "uefi"))]
pub fn cleanup_firmware_regions(rec_page_table: &mut RecursivePageTable, kernel : &'static [u8]){
    use x86_64::{structures::paging::{Page, Size2MiB, Mapper}, VirtAddr};

    let kernel_start_virt = VirtAddr::new(kernel.as_ptr() as u64);
    let kernel_size = kernel.len() as u64;

    let start: Page<Size2MiB> = Page::containing_address(kernel_start_virt);
    let end: Page<Size2MiB> = Page::containing_address(kernel_start_virt + kernel_size - 1 as u64);
     for page in Page::range_inclusive(start, end) {
        rec_page_table
            .unmap(page)
            .expect("dealloc error")
            .1
            .flush();
    }
}