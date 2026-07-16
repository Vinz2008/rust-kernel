use core::{cmp::min, ptr};

use elf::{ElfBytes, ParseError, endian::AnyEndian, segment::ProgramHeader};
use x86_64::{VirtAddr, structures::paging::{Page, PageSize, PageTableFlags, Size4KiB, mapper::MapToError}};

use crate::{allocator::map_page_at_in, paging::{PHYSICAL_MEMORY_OFFSET, translate_addr_in}, process::Process, serial_println, userspace::map_userspace_stack};


#[derive(Debug)]
pub enum ElfError {
    ElfParsingErr(ParseError),
    UnsupportedArch,
    UnsupportedElfType,
    MapPagingErr(MapToError<Size4KiB>),
    SegmentTableNotFound,
    TranslatePhysErr,
    InvalidElf,
}

impl From<ParseError> for ElfError {
    fn from(parse_error: ParseError) -> ElfError {
        ElfError::ElfParsingErr(parse_error)
    }
}

pub fn elf_to_page_permission(elf_flags : u32) -> PageTableFlags {
    let mut flags = PageTableFlags::PRESENT | PageTableFlags::USER_ACCESSIBLE;
    if elf_flags & elf::abi::PF_W != 0 {
        flags |= PageTableFlags::WRITABLE;
    }
    if elf_flags & elf::abi::PF_X == 0 {
        flags |= PageTableFlags::NO_EXECUTE;
    }
    flags
}

fn load_segment(content: &[u8], process : &Process, prog_header : &ProgramHeader) -> Result<(), ElfError> {
    let virt_addr = prog_header.p_vaddr;
    let memory_size = prog_header.p_memsz as usize;
    let file_size = prog_header.p_filesz as usize;

    if file_size > memory_size {
        return Err(ElfError::InvalidElf);
    }

    if memory_size == 0 {
        return Ok(());
    }

    let start = VirtAddr::new(virt_addr);
    let end = VirtAddr::new(virt_addr + memory_size as u64 - 1);

    let start_page = Page::<Size4KiB>::containing_address(start);
    let end_page = Page::<Size4KiB>::containing_address(end);

    let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE;
    
    let phys_offset = PHYSICAL_MEMORY_OFFSET.get().unwrap().as_u64();

    const PAGE_SIZE : usize = Size4KiB::SIZE as usize; // TODO : change this when adding big pages

    for page in Page::range_inclusive(start_page, end_page){
        match map_page_at_in(process.page_table_phys.start_address(), page.start_address(), flags){
            Ok(_) => {
                let page_phys = unsafe {
                    translate_addr_in(process.page_table_phys, page.start_address())
                }.ok_or(ElfError::TranslatePhysErr)?;
                let dst_ptr = (phys_offset + page_phys.as_u64()) as *mut u8;
                unsafe {
                    ptr::write_bytes(dst_ptr, 0, PAGE_SIZE);
                }
            }
            Err(MapToError::PageAlreadyMapped(_)) => {} // TODO : only not zero if the page is already written here (it could be already mapped, but has been used)
            Err(e) => return Err(ElfError::MapPagingErr(e)),
        }
    }

    let segment_off = prog_header.p_offset as usize;

    let mut written = 0;

    while written < file_size {
        let dst_virt = start + written as u64;
        let dst_phys = unsafe { 
            translate_addr_in(process.page_table_phys, dst_virt) 
        }.ok_or(ElfError::TranslatePhysErr)?;
        let offset_in_page = dst_virt.as_u64() as usize & (PAGE_SIZE - 1);
        let bytes_left_in_page = PAGE_SIZE - offset_in_page;
        let chunk_len = min(bytes_left_in_page, file_size-written);

        let dst_ptr = (phys_offset + dst_phys.as_u64()) as *mut u8;

        unsafe {
            let src_ptr = content.as_ptr().add(segment_off + written);
            ptr::copy_nonoverlapping(src_ptr, dst_ptr, chunk_len);
        }
        
        written += chunk_len;
    }

    Ok(())
}

// TODO : special permissions for each pages (ex, const data and code is read only, data is no exe)

pub fn load_elf<'a>(content : &'a [u8], process : &Process) -> Result<ElfBytes<'a, AnyEndian>, ElfError> {
    let file = ElfBytes::<AnyEndian>::minimal_parse(content)?;

    if file.ehdr.e_machine != elf::abi::EM_X86_64 {
        return Err(ElfError::UnsupportedArch);
    }

    // TODO : support ET_DYN (PIE/relocations)
    serial_println!("file.ehdr.e_machine : {}", file.ehdr.e_machine);

    if file.ehdr.e_type != elf::abi::ET_EXEC {
        return Err(ElfError::UnsupportedElfType);
    }

    //let text_section_header = file.section_header_by_name(".text").unwrap().unwrap();
    //let text_section_content = file.section_data(&text_section_header).unwrap().0;
    //serial_println!("text section content : {:?}", text_section_content);

    let mut stack_flags = elf::abi::PF_R | elf::abi::PF_W;

    for prog_header in file.segments().ok_or(ElfError::SegmentTableNotFound)? {
        serial_println!("type={} offset={:#x} vaddr={:#x} filesz={:#x} memsz={:#x} flags={:#x}", prog_header.p_type, prog_header.p_offset, prog_header.p_vaddr, prog_header.p_filesz, prog_header.p_memsz, prog_header.p_flags);
        match prog_header.p_type {
            elf::abi::PT_LOAD => load_segment(content, process, &prog_header)?,
            elf::abi::PT_GNU_STACK => {
                stack_flags = prog_header.p_flags;
            }
            // TODO : really support PT_GNU_RELRO, which says which segment should become read only after writing its content 
            elf::abi::PT_GNU_RELRO | elf::abi::PT_PHDR  => {},
            elf::abi::PT_INTERP => return Err(ElfError::UnsupportedElfType), // dynamic exe unsupported
            p_type => serial_println!("unknown p_type : {}", p_type), // TODO : reject unsupported sections
        }
    }

    map_userspace_stack(process, stack_flags);

    // TODO : validate that e_entry is in a mapped region of memory
    
    Ok(file)
}