#![no_std]
#![no_main]

use rt::{self as _, Args, power::reboot};

#[unsafe(no_mangle)]
pub extern "Rust" fn main(_args : Args<'_>) -> i32 {
    reboot()
}