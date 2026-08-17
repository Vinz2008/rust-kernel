#![no_std]
#![no_main]

use rt::{Args, println, syscall::{syscall_exec, syscall_wait_pid}};

#[unsafe(no_mangle)]
pub extern "Rust" fn main(_args : Args<'_>) -> i32 {
    println!("init start");

    let pid = syscall_exec("/bin/cli", &["/bin/cli"]).unwrap();
    syscall_wait_pid(pid);

    0
}