#![no_std]
#![no_main]

use rt::{self as _, Args, print, println, shared_consts::{DIRENT_DIR, DirChild, Fd, READABLE}, syscall::{syscall_get_dir_children, syscall_open}};

#[unsafe(no_mangle)]
pub extern "Rust" fn main(args : Args<'_>) -> i32 {
    let dir = match args.get(1) {
        Some(dir) => dir,
        None => ".",
    };

    
    let current_dir_fd: Fd = syscall_open(dir, READABLE).unwrap();
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
            print!("{}", name);
            if child.kind == DIRENT_DIR {
                print!("/");
            }
            println!();
        }
    }
    0
}