#![no_std]
#![no_main]

use rt::{self as _, Args, println, shared_consts::{DirChild, READABLE}, syscall::{syscall_get_cwd, syscall_get_dir_children, syscall_open}};

#[unsafe(no_mangle)]
pub extern "Rust" fn main(args : Args<'_>) -> i32 {
    let current_cwd = syscall_get_cwd().unwrap();
    let current_dir_fd: rt::shared_consts::Fd = syscall_open(&current_cwd, READABLE).unwrap();
    let mut children = [DirChild {
        kind: 0,
        name_len: 0,
        name: [0; rt::shared_consts::PATH_NAME_MAX],
    }; 16];
    loop {
        let n = syscall_get_dir_children(current_dir_fd, &mut children).unwrap();
        if n == 0 {
            break;
        }

        for child in &children[..n] {
            let name = str::from_utf8(&child.name[..child.name_len as usize]).unwrap_or("<invalid UTF-8>");
            println!("{}", name);
        }
    }
    0
}