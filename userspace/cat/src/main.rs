#![no_std]
#![no_main]

use rt::{Args, println, shared_consts::READABLE, syscall::{syscall_open, syscall_print, syscall_read}};

#[unsafe(no_mangle)]
pub extern "Rust" fn main(args : Args<'_>) -> i32 {
    let file_path = match args.get(1) {
        Some(file_path) => file_path,
        None => {
            println!("file arg missing");
            return -1;
        }
    };

    let fd = syscall_open(file_path, READABLE).expect("file not found");

    let mut buf = [0 as u8; 4096];

    loop {
        let count = syscall_read(fd, &mut buf).expect("error in read");
        if count == 0 {
            break;
        }
        // TODO : for now can't print only a str, TODO : just have printing be writing to a files, which is just bytes
        let str = str::from_utf8(&buf[..count]).expect("can't print file");
        syscall_print(str).expect("failed print");
    }
    println!();

    0
}