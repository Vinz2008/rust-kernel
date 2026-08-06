#![no_std]
#![no_main]

use rt::{Args, fs::File, println, shared_consts::{READABLE}};

#[unsafe(no_mangle)]
pub extern "Rust" fn main(args : Args<'_>) -> i32 {
    let file_path = match args.get(1) {
        Some(file_path) => file_path,
        None => {
            println!("file arg missing");
            return -1;
        }
    };
    let _ = File::create(file_path, READABLE).expect("error when creating file");
    0
}