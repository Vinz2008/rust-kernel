use core::arch::naked_asm;

use alloc::{slice, str};
use x86_64::{VirtAddr, structures::paging::{Page, PageTableFlags, Size4KiB}};

use crate::{allocator::MEMORY_MANAGER, println, process::CURRENT_PROCESS, serial_println};

#[unsafe(naked)]
pub unsafe extern "C" fn syscall_interrupt_stub() -> ! {
    naked_asm!(
        "
        push rax
        push rbx
        push rcx
        push rdx
        push rsi
        push rdi
        push rbp
        push r8
        push r9
        push r10
        push r11
        push r12
        push r13
        push r14
        push r15

        mov rdi, rsp # put in rdi the stack pointer to have as arg the reg struct
        call {handler}

        pop r15
        pop r14
        pop r13
        pop r12
        pop r11
        pop r10
        pop r9
        pop r8
        pop rbp
        pop rdi
        pop rsi
        pop rdx
        pop rcx
        pop rbx
        pop rax

        iretq
        ",
        handler = sym syscall_interrupt_handler,
    )
}

#[repr(C)]
pub struct SyscallRegs {
    pub r15: u64,
    pub r14: u64,
    pub r13: u64,
    pub r12: u64,
    pub r11: u64,
    pub r10: u64,
    pub r9: u64,
    pub r8: u64,
    pub rbp: u64,
    pub rdi: u64,
    pub rsi: u64,
    pub rdx: u64,
    pub rcx: u64,
    pub rbx: u64,
    pub rax: u64,
}

impl SyscallRegs {

    fn get_arg(&self, nb : u8) -> u64 {
        match nb {
            1 => self.rdi,
            2 => self.rsi,
            3 => self.rdx,
            4 => self.r10,
            5 => self.r8,
            6 => self.r9,
            _ => unreachable!(), // coding error
        }
    }
}

fn syscall_interrupt_handler(regs : &mut SyscallRegs){
    let sycall_nb = regs.rax;
    serial_println!("syscall rax number : {}", sycall_nb);
    let ret = match sycall_nb {
        0 => syscall_exit(regs),
        1 => syscall_print(regs), // TODO : change these syscalls ?
        _ => u64::MAX,
    };
    regs.rax = ret;
}

fn syscall_exit(regs : &mut SyscallRegs) -> u64 {
    let current_process_pid = *CURRENT_PROCESS.lock();
    serial_println!("current_process_pid : {:?}", current_process_pid);
    if current_process_pid.is_some_and(|pid| pid.0.get() == 1){
        panic!("tried to exit init");
    }
    todo!()
}


// TODO : look at all the memory regions, and also a check to have kernel memory forbidden (for ex memory > 0xXXXXX)
fn check_ptr(ptr : usize, len : usize, is_write : bool) -> bool {
    let end = match ptr.checked_add(len){
        Some(end) => end,
        None => return false,
    };
    let start_page = Page::<Size4KiB>::containing_address(VirtAddr::new(ptr as u64));
    let end_page = Page::<Size4KiB>::containing_address(VirtAddr::new((end-1) as u64));
    let memory_manager_lock = MEMORY_MANAGER.get().unwrap().lock();
    for page in Page::range_inclusive(start_page, end_page){
        let flags = memory_manager_lock.get_page_flags(page.start_address());
        match flags {
            Some(flags) => {
                if !flags.contains(PageTableFlags::USER_ACCESSIBLE){
                    return false;
                }
                if is_write && !flags.contains(PageTableFlags::WRITABLE){
                    return false;
                }
            },
            None => return false,
        }
    }
    true
}

fn syscall_print(regs : &mut SyscallRegs) -> u64 {
    let message_ptr = regs.get_arg(1) as *const u8;
    serial_println!("message_ptr : {:?}", message_ptr);
    
    let message_len = regs.get_arg(2);
    if !check_ptr(message_ptr as usize, message_len as usize, false) {
        return u64::MAX;
    }
    let slice = unsafe { slice::from_raw_parts(message_ptr, message_len as usize) };
    let s = match str::from_utf8(slice){
        Ok(s) => s,
        Err(_) => return u64::MAX,
    };
    println!("{}", s);
    0
}