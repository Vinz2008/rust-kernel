use core::hint::unreachable_unchecked;

use syscall_nbs::{SYSCALL_EXEC, SYSCALL_EXIT, SYSCALL_GET_CHAR, SYSCALL_PRINT, SYSCALL_WAIT_PID};

pub unsafe fn syscall0(syscall_nb : u64) -> u64 {
    let ret : u64;
    unsafe {
        core::arch::asm!(
            "int 0x80",
            inlateout("rax") syscall_nb => ret,
            options(nostack)
        );
    }
    ret
}

pub unsafe fn syscall1(syscall_nb : u64, arg1 : u64) -> u64 {
    let ret : u64;
    unsafe {
        core::arch::asm!(
            "int 0x80",
            inlateout("rax") syscall_nb => ret,
            in("rdi") arg1,
            options(nostack)
        );
    }
    ret
}

pub unsafe fn syscall2(syscall_nb : u64, arg1 : u64, arg2 : u64) -> u64 {
    let ret : u64;
    unsafe {
        core::arch::asm!(
            "int 0x80",
            inlateout("rax") syscall_nb => ret,
            in("rdi") arg1,
            in("rsi") arg2,
            options(nostack)
        );
    }
    ret
}

pub fn syscall_exit(status : i32) -> ! {
    unsafe { 
        syscall1(SYSCALL_EXIT, status as u64);
        unreachable_unchecked()
    }
}

fn str_to_ptr_and_len(s : &str) -> (u64, u64) {
    (s.as_ptr() as u64, s.len() as u64)
}

pub fn syscall_print(message : &str){
    let (message_ptr, message_len) = str_to_ptr_and_len(message);
    unsafe {
        syscall2(SYSCALL_PRINT, message_ptr, message_len);
    }
}

pub fn syscall_exec(path : &str) -> u64 {
    let (path_ptr, path_len) = str_to_ptr_and_len(path);
    unsafe {
        syscall2(SYSCALL_EXEC, path_ptr, path_len)
    }
}

pub fn syscall_get_char() -> char {
    unsafe {
        let c_u32 = syscall0(SYSCALL_GET_CHAR).try_into().unwrap();
        char::from_u32_unchecked(c_u32) 
    }
}

pub fn syscall_wait_pid(pid : u64){
    unsafe {
        syscall1(SYSCALL_WAIT_PID, pid); 
    }
}