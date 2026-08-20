use crate::frame_allocator::FrameAllocator;
use crate::bootinfo::MemoryRegionType;
use crate::bootinfo::TlsTemplate;
use fixedvec::FixedVec;
use x86_64::structures::paging::mapper::{MapToError, MapperFlush};
use x86_64::structures::paging::{
    self, Mapper, Page, PageSize, PageTableFlags, PhysFrame, RecursivePageTable, Size4KiB,
};
use x86_64::VirtAddr;
use xmas_elf::program::{self, ProgramHeader64};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemoryInfo {
    pub stack_end: VirtAddr,
    pub tls_segment: Option<TlsTemplate>,
}

#[derive(Debug)]
pub enum MapKernelError {
    Mapping(
        /// This field is never read, but still printed as part of the Debug output on error.
        #[allow(dead_code)]
        MapToError<Size4KiB>,
    ),
    MultipleTlsSegments,
}

impl From<MapToError<Size4KiB>> for MapKernelError {
    fn from(e: MapToError<Size4KiB>) -> Self {
        MapKernelError::Mapping(e)
    }
}

pub(crate) fn map_kernel(
    kernel : &[u8],
    stack_start: Page,
    stack_size: u64,
    segments: &FixedVec<ProgramHeader64>,
    page_table: &mut RecursivePageTable,
    frame_allocator: &mut FrameAllocator,
) -> Result<MemoryInfo, MapKernelError> {
    let mut tls_segment = None;
    for segment in segments {
        let tls = map_segment(segment, kernel, page_table, frame_allocator)?;
        if let Some(tls) = tls {
            if tls_segment.replace(tls).is_some() {
                return Err(MapKernelError::MultipleTlsSegments);
            }
        }
    }

    // Create a stack
    let stack_start = stack_start + 1; // Leave the first page unmapped as a 'guard page'
    let stack_end = stack_start + stack_size; // stack_size is in pages

    let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;
    let region_type = MemoryRegionType::KernelStack;

    for page in Page::range(stack_start, stack_end) {
        let frame = frame_allocator
            .allocate_frame(region_type)
            .ok_or(MapToError::FrameAllocationFailed)?;
        unsafe { map_page(page, frame, flags, page_table, frame_allocator)? }.flush();
    }

    Ok(MemoryInfo {
        stack_end: stack_end.start_address(),
        tls_segment,
    })
}

// TODO : use elf instead of xmas-elf ?
pub(crate) fn map_segment(
    segment: &ProgramHeader64,
    kernel : &[u8],
    page_table: &mut RecursivePageTable,
    frame_allocator: &mut FrameAllocator,
) -> Result<Option<TlsTemplate>, MapToError<Size4KiB>> {
    let typ = segment.get_type().unwrap();
    match typ {
        program::Type::Load => {
            let mem_size = segment.mem_size;
            let file_size = segment.file_size;
            let file_offset = segment.offset;
            if mem_size == 0 {
                return Ok(None);
            }

            assert!(file_size <= mem_size, "ELF PT_LOAD has file_size > mem_size");

            let virt_start_addr = VirtAddr::new(segment.virtual_addr);

            let start_page: Page<Size4KiB> = Page::containing_address(virt_start_addr);


            let end_addr = virt_start_addr + mem_size - 1 as u64;
            let end_page: Page<Size4KiB> = Page::containing_address(end_addr);
            for page in Page::range_inclusive(start_page, end_page) {
                let frame = frame_allocator.allocate_frame(MemoryRegionType::Kernel).ok_or(MapToError::FrameAllocationFailed)?;
                let temporary_flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::NO_EXECUTE | PageTableFlags::GLOBAL;
                unsafe {
                    map_page(
                        page,
                        frame,
                        temporary_flags,
                        page_table,
                        frame_allocator,
                    )?.flush();
                    // zero out memory
                    core::ptr::write_bytes(
                        page.start_address().as_mut_ptr::<u8>(),
                        0,
                        Size4KiB::SIZE as usize,
                    );
                }
            }

            if file_size != 0 {
                let file_end = file_offset + file_size;
                let src = &kernel[file_offset as usize ..file_end as usize];
                unsafe {
                    core::ptr::copy_nonoverlapping(
                        src.as_ptr(),
                        virt_start_addr.as_mut_ptr::<u8>(),
                        src.len(),
                    );
                }
            }

            let flags = segment.flags;
            let mut page_table_flags = PageTableFlags::PRESENT | PageTableFlags::GLOBAL;
            if !flags.is_execute() {
                page_table_flags |= PageTableFlags::NO_EXECUTE
            };
            if flags.is_write() {
                page_table_flags |= PageTableFlags::WRITABLE
            };

            for page in Page::range_inclusive(start_page, end_page) {
                unsafe {
                    page_table.update_flags(page, page_table_flags).unwrap()
                }.flush();
            }

            Ok(None)
        }
        program::Type::Tls => Ok(Some(TlsTemplate {
            start_addr: segment.virtual_addr,
            mem_size: segment.mem_size,
            file_size: segment.file_size,
        })),
        _ => Ok(None),
    }
}

pub(crate) unsafe fn map_page<'a, S>(
    page: Page<S>,
    phys_frame: PhysFrame<S>,
    flags: PageTableFlags,
    page_table: &mut RecursivePageTable<'a>,
    frame_allocator: &mut FrameAllocator,
) -> Result<MapperFlush<S>, MapToError<S>>
where
    S: PageSize,
    RecursivePageTable<'a>: Mapper<S>,
{
    struct PageTableAllocator<'a, 'b: 'a>(&'a mut FrameAllocator<'b>);

    unsafe impl<'a, 'b> paging::FrameAllocator<Size4KiB> for PageTableAllocator<'a, 'b> {
        fn allocate_frame(&mut self) -> Option<PhysFrame<Size4KiB>> {
            self.0.allocate_frame(MemoryRegionType::PageTable)
        }
    }

    page_table.map_to(
        page,
        phys_frame,
        flags,
        &mut PageTableAllocator(frame_allocator),
    )
}
