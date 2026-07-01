#![no_std]

mod panic;
pub mod syscall;

// TODO : allocator
// TODO : add syscalls in syscall.rs

unsafe extern "Rust" {
    fn main() -> i32;
}

#[unsafe(no_mangle)]
pub fn _start() -> ! {
    let exit = unsafe { main() };
    unsafe {
        core::arch::asm!(
            "int 0x80",
            in("rax") exit as usize,
            options(noreturn),
        );
    }
}

