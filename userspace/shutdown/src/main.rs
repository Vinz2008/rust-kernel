#![no_std]
#![no_main]

use rt::{self as _, Args, shared_consts::SHUTDOWN_SUCCESS, syscall::syscall_shutdown};

#[unsafe(no_mangle)]
pub extern "Rust" fn main(args : Args<'_>) -> i32 {
    // TODO : add flags handling to know if there should be success of failure
    syscall_shutdown(SHUTDOWN_SUCCESS)
}