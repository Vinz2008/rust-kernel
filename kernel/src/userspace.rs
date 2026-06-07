use core::arch::asm;

use x86_64::registers::rflags::RFlags;

use crate::gdt::GDT;

pub type EntryPointFun = extern "C" fn() -> i32;

// TODO : call this
pub fn switch_to_userspace(entry_point : EntryPointFun, stack_addr : usize){
    let stack_segment = GDT.1.user_data_selector.0 as usize;
    let code_segment = GDT.1.user_code_selector.0 as usize;
    let stack_addr = stack_addr & !0xf; // 16 bytes align the stack, for syscall and iret
    let rflags = RFlags::INTERRUPT_FLAG | RFlags::from_bits_truncate(0x2); // 0x2 is for the reserved bit that always need to be 1
    unsafe {
        asm!(
            "push {ss}",
            "push {rsp}",
            "push {rflags}",
            "push {cs}",
            "push {rip}",
            "iretq",
            ss = in(reg) stack_segment,
            rsp = in(reg) stack_addr,
            rflags = in(reg) (rflags).bits(),
            cs = in(reg) code_segment,
            rip = in(reg) entry_point as usize,
            options(noreturn),
        );
    }
}