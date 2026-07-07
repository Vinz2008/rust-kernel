#![no_std]
#![no_main]

use rt::{self as _, println, shared_consts::READABLE, syscall::{syscall_get_cwd, syscall_open}};

#[unsafe(no_mangle)]
pub extern "Rust" fn main() -> i32 {
    // TODO : inherit cwd for child process to help for ls
    let current_cwd = syscall_get_cwd().unwrap();
    let current_dir_fd = syscall_open(&current_cwd, READABLE).unwrap();
    println!("ls in {}", &current_cwd);
    0
}