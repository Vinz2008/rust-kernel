#![no_std]
#![no_main]

use rt::{self as _, syscall::syscall_print};


#[allow(deref_nullptr)]
#[unsafe(no_mangle)]
pub extern "Rust" fn main() -> i32 {
    syscall_print("test");

    loop {}
}