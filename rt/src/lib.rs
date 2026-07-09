#![no_std]

pub extern crate alloc;

pub use shared_consts;

mod panic;
mod allocator;
pub mod syscall;
pub mod print;

// TODO : allocator

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

