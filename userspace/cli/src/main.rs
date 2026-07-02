#![no_std]
#![no_main]

use rt::{self as _, syscall::{syscall_get_char, syscall_print}};


#[allow(deref_nullptr)]
#[unsafe(no_mangle)]
pub extern "Rust" fn main() -> i32 {
    syscall_print(">");

    /*let c = syscall_get_char();
    let mut dst = [0; 4];
    syscall_print(c.encode_utf8(&mut dst));*/

    loop {}
}