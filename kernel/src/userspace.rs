use x86_64::{VirtAddr, structures::paging::{Page, Size4KiB}};

use crate::{allocator::map_page_at_in, elf::elf_to_page_permission, process::Process};

pub const USER_STACK_TOP: usize = 0x0000_7fff_ffff_f000;
const USER_STACK_SIZE: usize = 1024 * 1024; // 1MiB


pub fn map_userspace_stack(process : &Process, stack_flags : u32){
    // TODO : maybe replace the pattern like this with a range of page with a function mapping multiple page (like for example a start address and a number of pages or a len ?)
    let start = VirtAddr::new((USER_STACK_TOP - USER_STACK_SIZE) as u64);
    let end = VirtAddr::new((USER_STACK_TOP - 1) as u64);
    let start_page = Page::<Size4KiB>::containing_address(start);
    let end_page = Page::<Size4KiB>::containing_address(end);
    let page_table_flags = elf_to_page_permission(stack_flags);
    for page in Page::range_inclusive(start_page, end_page){
        map_page_at_in(process.page_table_phys.start_address(), page.start_address(),  page_table_flags).unwrap().ignore(); // TODO : should I really unwrap
    }
}