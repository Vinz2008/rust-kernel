#![no_std]
#![no_main]

use rt::{Args, fs::{File, IoWrite}, print, println, shared_consts::{READABLE, WRITABLE}, syscall::syscall_read};

#[unsafe(no_mangle)]
pub extern "Rust" fn main(args : Args<'_>) -> i32 {
    let file_path = match args.get(1) {
        Some(file_path) => file_path,
        None => {
            println!("file arg missing");
            return -1;
        }
    };

    let file = File::open(file_path, READABLE).expect("file not found");
    let mut stdout = File::open("/dev/stdout", WRITABLE).expect("couldn't open stdout");

    let mut buf = [0 as u8; 4096];

    loop {
        let count = syscall_read(unsafe { file.get_fd() }, &mut buf).expect("error in read");
        if count == 0 {
            break;
        }
        stdout.write(&buf[..count]).expect("write failed to stdout");
    }
    stdout.write(&[b'\n']).expect("write failed to stdout");

    0
}