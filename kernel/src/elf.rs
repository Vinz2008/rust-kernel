use core::ptr;

use elf::{ElfBytes, endian::AnyEndian};
use x86_64::{VirtAddr, structures::paging::{Page, PageTableFlags, Size4KiB}};

use crate::{allocator::map_page_at, serial_println, userspace::EntryPointFun};


pub const USER_STACK_TOP: usize = 0x0000_7fff_ffff_f000;
const USER_STACK_SIZE: usize = 64 * 1024; // 64 KiB

fn elf_to_page_permission(elf_flags : u32) -> PageTableFlags {
    let mut flags = PageTableFlags::PRESENT | PageTableFlags::USER_ACCESSIBLE;
    if elf_flags & elf::abi::PF_W != 0 {
        flags |= PageTableFlags::WRITABLE;
    }
    if elf_flags & elf::abi::PF_X == 0 {
        flags |= PageTableFlags::NO_EXECUTE;
    }
    flags
}

pub fn load_elf<'a>(content : &'a [u8]) -> ElfBytes<'a, AnyEndian>{
    let file = ElfBytes::<AnyEndian>::minimal_parse(content).expect("Error when parsing init elf");

    //let text_section_header = file.section_header_by_name(".text").unwrap().unwrap();
    //let text_section_content = file.section_data(&text_section_header).unwrap().0;
    //serial_println!("text section content : {:?}", text_section_content);

    let mut stack_flags = elf::abi::PF_R | elf::abi::PF_W;

    for prog_header in file.segments().unwrap() {
        serial_println!("type={} offset={:#x} vaddr={:#x} filesz={:#x} memsz={:#x} flags={:#x}", prog_header.p_type, prog_header.p_offset, prog_header.p_vaddr, prog_header.p_filesz, prog_header.p_memsz, prog_header.p_flags);
        match prog_header.p_type {
            elf::abi::PT_LOAD => {
                let virtual_addr = prog_header.p_vaddr;
                let start = VirtAddr::new(virtual_addr);
                let end = VirtAddr::new(virtual_addr + prog_header.p_memsz as u64 - 1);

                let start_page = Page::<Size4KiB>::containing_address(start);
                let end_page = Page::<Size4KiB>::containing_address(end);
                for page in Page::range_inclusive(start_page, end_page){
                    map_page_at(page.start_address(), PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE);
                }
                
                let segment_off = prog_header.p_offset as usize;
                let bytes = &content[segment_off..(segment_off + prog_header.p_filesz as usize)];
                let virtual_addr_ptr = virtual_addr as usize as *mut u8;
                unsafe {
                    ptr::copy_nonoverlapping(bytes.as_ptr(), virtual_addr_ptr, prog_header.p_filesz as usize);
                }
                if prog_header.p_filesz < prog_header.p_memsz {
                    // zero the rest
                    unsafe {
                        ptr::write_bytes(virtual_addr_ptr.add(prog_header.p_filesz as usize), 0, (prog_header.p_memsz - prog_header.p_filesz) as usize);
                    }
                }
            },
            elf::abi::PT_GNU_STACK => {
                stack_flags = prog_header.p_flags;
            }
            elf::abi::PT_GNU_RELRO | elf::abi::PT_PHDR  => {},
            p_type => serial_println!("unknown p_type : {}", p_type),
        }
    }


    // map stack
    // TODO : maybe replace the pattern like this with a range of page with a function mapping multiple page (like for example a start address and a number of pages or a len ?)

    let start = VirtAddr::new((USER_STACK_TOP - USER_STACK_SIZE) as u64);
    let end = VirtAddr::new((USER_STACK_TOP - 1) as u64);
    let start_page = Page::<Size4KiB>::containing_address(start);
    let end_page = Page::<Size4KiB>::containing_address(end);
    let page_table_flags = elf_to_page_permission(stack_flags);
    for page in Page::range_inclusive(start_page, end_page){
        map_page_at(page.start_address(),  page_table_flags);
    }


    file
}

pub fn get_elf_entrypoint(elf : &ElfBytes<'_, AnyEndian>) -> EntryPointFun {
    let entrypoint_virt_address = elf.ehdr.e_entry as usize;
    let entrypoint_fun : EntryPointFun = unsafe { core::mem::transmute(entrypoint_virt_address) };
    entrypoint_fun
}