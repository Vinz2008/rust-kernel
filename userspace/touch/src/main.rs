#![no_std]
#![no_main]

use rt::{Args, println, shared_consts::{CREATE_FILE, READABLE}, syscall::syscall_open};

#[unsafe(no_mangle)]
pub extern "Rust" fn main(args : Args<'_>) -> i32 {
    let file_path = match args.get(1) {
        Some(file_path) => file_path,
        None => {
            println!("file arg missing");
            return -1;
        }
    };
    let _ = syscall_open(file_path, READABLE | CREATE_FILE).expect("error when creating file");
    0
}