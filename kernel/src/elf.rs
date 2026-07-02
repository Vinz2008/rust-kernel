use core::ptr;

use elf::{ElfBytes, endian::AnyEndian, segment::ProgramHeader};
use x86_64::{VirtAddr, structures::paging::{Page, PageTableFlags, Size4KiB, mapper::MapToError}};

use crate::{allocator::map_page_at_in, paging::{PHYSICAL_MEMORY_OFFSET, translate_addr_in}, process::Process, serial_println, userspace::{EntryPointFun, map_userspace_stack}};


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

fn load_segment(content: &[u8], process : &Process, prog_header : &ProgramHeader){
    let virt_addr = prog_header.p_vaddr;
    let memory_size = prog_header.p_memsz as usize;
    let file_size = prog_header.p_filesz as usize;

    if memory_size == 0 {
        return;
    }

    let start = VirtAddr::new(virt_addr);
    let end = VirtAddr::new(virt_addr + memory_size as u64 - 1);

    let start_page = Page::<Size4KiB>::containing_address(start);
    let end_page = Page::<Size4KiB>::containing_address(end);

    let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE;
    
    for page in Page::range_inclusive(start_page, end_page){
        match map_page_at_in(process.page_table_phys.start_address(), page.start_address(), flags){
            Ok(_) => {}
            Err(MapToError::PageAlreadyMapped(_)) => {}
            Err(e) => panic!("error when mapping elf page : {:?}", e), // TODO : should I really panic here ?
        }
    }

    let segment_off = prog_header.p_offset as usize;

    // TODO : this is byte by byte copying, optimize it by copying page by page, because one page can't be non contiguous, but page by page it can't be
    for i in 0..memory_size {
        let dst_virt_addr = VirtAddr::new((virt_addr as usize + i) as u64);
        let dst_phys = unsafe { translate_addr_in(process.page_table_phys, dst_virt_addr) }.unwrap(); // TODO : replace this unwrap with proper error handling ? (can it even fail because I just mapped the pages isn't it ?)
        let virt_ptr_of_phys = (PHYSICAL_MEMORY_OFFSET.get().unwrap().as_u64() + dst_phys.as_u64()) as *mut u8;
        unsafe {
            if i < file_size {
                *virt_ptr_of_phys = content[segment_off + i];
            } else {
                *virt_ptr_of_phys = 0;
            }
        }
    }
                
    
    /*let bytes = &content[segment_off..(segment_off + file_size)];
    let page_table_frame = process.page_table_phys;
    let phys_frame = unsafe { translate_addr_in(page_table_frame, start) }.unwrap(); // TODO : replace this unwrap with proper error handling ? (can it even fail because I just mapped the pages isn't it ?)
    let virtual_addr_ptr_of_phys = (PHYSICAL_MEMORY_OFFSET.get().unwrap().as_u64() + phys_frame.as_u64()) as *mut u8;
    unsafe {
        ptr::copy_nonoverlapping(bytes.as_ptr(), virtual_addr_ptr_of_phys, file_size);
    }
    if file_size < memory_size {
        // zero the rest
        unsafe {
            ptr::write_bytes(virtual_addr_ptr_of_phys.add(file_size), 0, memory_size - file_size);
        }
    }*/
}

pub fn load_elf<'a>(content : &'a [u8], process : &Process) -> ElfBytes<'a, AnyEndian>{
    let file = ElfBytes::<AnyEndian>::minimal_parse(content).expect("Error when parsing init elf");

    //let text_section_header = file.section_header_by_name(".text").unwrap().unwrap();
    //let text_section_content = file.section_data(&text_section_header).unwrap().0;
    //serial_println!("text section content : {:?}", text_section_content);

    let mut stack_flags = elf::abi::PF_R | elf::abi::PF_W;

    for prog_header in file.segments().unwrap() {
        serial_println!("type={} offset={:#x} vaddr={:#x} filesz={:#x} memsz={:#x} flags={:#x}", prog_header.p_type, prog_header.p_offset, prog_header.p_vaddr, prog_header.p_filesz, prog_header.p_memsz, prog_header.p_flags);
        match prog_header.p_type {
            elf::abi::PT_LOAD => load_segment(content, process, &prog_header),
            elf::abi::PT_GNU_STACK => {
                stack_flags = prog_header.p_flags;
            }
            elf::abi::PT_GNU_RELRO | elf::abi::PT_PHDR  => {},
            p_type => serial_println!("unknown p_type : {}", p_type),
        }
    }

    map_userspace_stack(process, stack_flags);
    
    file
}