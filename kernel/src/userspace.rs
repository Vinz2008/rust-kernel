use x86_64::{VirtAddr, structures::paging::{Page, Size4KiB}};

use crate::{allocator::map_page_at_in, elf::elf_to_page_permission, process::Process};

//pub type EntryPointFun = extern "C" fn() -> !;


/*pub fn switch_to_userspace(entry_point : EntryPointFun, stack_addr : usize, kernel_stack_top : VirtAddr, user_page_table : PhysFrame) -> ! {
    let stack_segment = GDT.1.user_data_selector.0 as usize | 3;
    let code_segment = GDT.1.user_code_selector.0 as usize | 3;
    let stack_addr = stack_addr & !0xf; // 16 bytes align the stack, for syscall and iret
    let rflags = RFlags::INTERRUPT_FLAG | RFlags::from_bits_truncate(0x2); // 0x2 is for the reserved bit that always need to be 1

    set_tss_privilege_stack(kernel_stack_top);

    serial_println!("exec: B");

    unsafe {
        asm!(
            "mov rsp, {kernel_rsp}",
            "mov cr3, {cr3}",
            "push {ss}",
            "push {rsp}",
            "push {rflags}",
            "push {cs}",
            "push {rip}",
            "iretq",
            kernel_rsp = in(reg) kernel_stack_top.as_u64(),
            cr3 = in(reg) user_page_table.start_address().as_u64(),
            ss = in(reg) stack_segment,
            rsp = in(reg) stack_addr,
            rflags = in(reg) (rflags).bits(),
            cs = in(reg) code_segment,
            rip = in(reg) entry_point as usize,
            options(noreturn),
        )
    }
}*/

pub const USER_STACK_TOP: usize = 0x0000_7fff_ffff_f000;
const USER_STACK_SIZE: usize = 64 * 1024; // 64 KiB

pub fn map_userspace_stack(process : &Process, stack_flags : u32){
    // TODO : maybe replace the pattern like this with a range of page with a function mapping multiple page (like for example a start address and a number of pages or a len ?)
    let start = VirtAddr::new((USER_STACK_TOP - USER_STACK_SIZE) as u64);
    let end = VirtAddr::new((USER_STACK_TOP - 1) as u64);
    let start_page = Page::<Size4KiB>::containing_address(start);
    let end_page = Page::<Size4KiB>::containing_address(end);
    let page_table_flags = elf_to_page_permission(stack_flags);
    for page in Page::range_inclusive(start_page, end_page){
        map_page_at_in(process.page_table_phys.start_address(), page.start_address(),  page_table_flags).unwrap(); // TODO : should I really unwrap
    }
}