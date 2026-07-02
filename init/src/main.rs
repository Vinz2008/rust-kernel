#![no_std]
#![no_main]

use rt::syscall::{syscall_exec, syscall_print};

// for now, not special function, just normal function (need to make the _start function when porting the std, TODO)
#[unsafe(no_mangle)]
pub extern "Rust" fn main() -> i32 {
    syscall_print("init start\n");

    let pid = syscall_exec("/cli");
    syscall_wait_pid(pid);
    
    0
}


static mut TEST_BSS: [u8; 4096] = [0; 4096];