use core::{cmp::min, ptr};

use elf::{ElfBytes, ParseError, abi::PF_X, endian::AnyEndian, segment::ProgramHeader};
use x86_64::{VirtAddr, align_down, structures::paging::{OffsetPageTable, Page, PageSize, PageTable, PageTableFlags, Size4KiB, mapper::MapToError}};

use crate::{paging::{PHYSICAL_MEMORY_OFFSET, get_page_flags_in, map_page_at_in, set_page_flags_in, translate_addr_in}, process::{ElfMemRegion, Process}, serial_println, userspace::map_userspace_stack};


#[derive(Debug)]
pub enum ElfError {
    ElfParsingErr(ParseError),
    UnsupportedArch,
    UnsupportedElfType,
    MapPagingErr(MapToError<Size4KiB>),
    SegmentTableNotFound,
    TranslatePhysErr,
    InvalidElf,
    RelroErr,
    ExecutableStackUnsupported,
    WriteExecutableSection,
    InvalidElfVirtAddr(VirtAddr),
}

impl From<ParseError> for ElfError {
    fn from(parse_error: ParseError) -> ElfError {
        ElfError::ElfParsingErr(parse_error)
    }
}

pub fn elf_to_page_permission(elf_flags : u32) -> Option<PageTableFlags> {
    let mut flags = PageTableFlags::PRESENT | PageTableFlags::USER_ACCESSIBLE;
    if elf_flags & elf::abi::PF_W != 0 {
        flags |= PageTableFlags::WRITABLE;
    }
    if elf_flags & elf::abi::PF_X == 0 {
        flags |= PageTableFlags::NO_EXECUTE;
    }

    if flags.contains(PageTableFlags::WRITABLE) && !flags.contains(PageTableFlags::NO_EXECUTE) {
        return None;
    }

    Some(flags)
}

const ELF_MEM_REGION_START : u64 = 0x0000_0000_0020_0000;
const ELF_MEM_REGION_END : u64 = 0x0000_0000_4000_0000;

fn validate_elf_virt_addr(virt_addr : VirtAddr) -> Result<(), ElfError>{
    if !(ELF_MEM_REGION_START..ELF_MEM_REGION_END).contains(&virt_addr.as_u64()){
        return Err(ElfError::InvalidElfVirtAddr(virt_addr));
    }
    Ok(())
}

// TODO : add a validate_elf_virt_addr_is_mapped to check if the address is mapped (obviously need to check this after mapping, that's why it needs to be a separate function)

fn load_segment(content: &[u8], process : &mut Process, prog_header : &ProgramHeader) -> Result<(), ElfError> {
    let virt_addr = prog_header.p_vaddr;
    let memory_size = prog_header.p_memsz as usize;
    let file_size = prog_header.p_filesz as usize;
    let elf_mem_flags = prog_header.p_flags;

    if file_size > memory_size {
        return Err(ElfError::InvalidElf);
    }

    if memory_size == 0 {
        return Ok(());
    }

    let start = VirtAddr::new(virt_addr);
    let end = VirtAddr::new(virt_addr + memory_size as u64 - 1);

    validate_elf_virt_addr(start)?;
    validate_elf_virt_addr(end)?;

    let start_page = Page::<Size4KiB>::containing_address(start);
    let end_page = Page::<Size4KiB>::containing_address(end);

    let flags = elf_to_page_permission(elf_mem_flags).ok_or(ElfError::WriteExecutableSection)?;
    
    let phys_offset = PHYSICAL_MEMORY_OFFSET.as_u64();

    const PAGE_SIZE : usize = Size4KiB::SIZE as usize; // TODO : change this when adding big pages

    for page in Page::range_inclusive(start_page, end_page){
        match map_page_at_in(process.page_table_phys.start_address(), page.start_address(), flags){
            Ok(flush) => {
                flush.ignore();
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

    process.elf_regions.push(ElfMemRegion {
        start: start_page.start_address(),
        end: end_page.start_address() + end_page.size(),
    });

    Ok(())
}

fn apply_relro(process: &mut Process, addr : u64, size : u64) -> Result<(), ElfError>{
    let end = addr.checked_add(size).ok_or(ElfError::RelroErr)?;
    let protect_start = align_down(addr, 4096);
    let protect_end   = align_down(end, 4096);

    if protect_start == protect_end {
        return Ok(());
    }
    let start_page = Page::<Size4KiB>::from_start_address(VirtAddr::new(protect_start)).map_err(|_| ElfError::RelroErr)?;
    let end_page = Page::<Size4KiB>::from_start_address(VirtAddr::new(protect_end)).map_err(|_| ElfError::RelroErr)?;

    let phys_offset = PHYSICAL_MEMORY_OFFSET;
    let page_table_phys = process.page_table_phys.start_address();
    let virt = phys_offset + page_table_phys.as_u64();
    let page_table_ptr: *mut PageTable = virt.as_mut_ptr();
    let page_table = unsafe { &mut *page_table_ptr };
    let mut mapper = unsafe { OffsetPageTable::new(page_table, phys_offset) };

    for page in Page::range(start_page, end_page) {
        let mut flags= get_page_flags_in(&mut mapper, page.start_address()).ok_or(ElfError::RelroErr)?;
        flags.remove(PageTableFlags::WRITABLE);
        set_page_flags_in(&mut mapper, page.start_address(), flags).ignore();
    }

    Ok(())
}

pub fn load_elf<'a>(content : &'a [u8], process : &mut Process) -> Result<ElfBytes<'a, AnyEndian>, ElfError> {
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

    for prog_header in file.segments().ok_or(ElfError::SegmentTableNotFound)? {
        serial_println!("type={} offset={:#x} vaddr={:#x} filesz={:#x} memsz={:#x} flags={:#x}", prog_header.p_type, prog_header.p_offset, prog_header.p_vaddr, prog_header.p_filesz, prog_header.p_memsz, prog_header.p_flags);
        match prog_header.p_type {
            elf::abi::PT_LOAD => load_segment(content, process, &prog_header)?,
            // TODO : support PT_TLS for thread local storage (use also fs/gs for it)
            elf::abi::PT_GNU_STACK => {
                if (prog_header.p_flags & PF_X) != 0 {
                    return Err(ElfError::ExecutableStackUnsupported);
                }
            }
            elf::abi::PT_GNU_RELRO | elf::abi::PT_PHDR => {},
            elf::abi::PT_INTERP => return Err(ElfError::UnsupportedElfType), // dynamic exe unsupported
            p_type => serial_println!("unknown p_type : {}", p_type), // TODO : reject unsupported sections
        }
    }

    // add here the functionnalities that override things, for relocations, dynamic loading, that then the permissions will be fixed by apply_relro

    for prog_header in file.segments().ok_or(ElfError::SegmentTableNotFound)? {
        if prog_header.p_type == elf::abi::PT_GNU_RELRO {
            validate_elf_virt_addr(VirtAddr::new(prog_header.p_vaddr))?;
            validate_elf_virt_addr(VirtAddr::new(prog_header.p_vaddr + prog_header.p_memsz - 1))?;
            apply_relro(process, prog_header.p_vaddr, prog_header.p_memsz)?;
        }
    }

    let stack_flags = elf::abi::PF_R | elf::abi::PF_W;

    map_userspace_stack(process, stack_flags);

    validate_elf_virt_addr(VirtAddr::new(file.ehdr.e_entry))?;
    
    Ok(file)
}