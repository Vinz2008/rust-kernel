#![no_std]
#![no_main]

use rt::{self as _, Args, fs::File, print, println, shared_consts::{DIRENT_DIR, DirChild, READABLE}, syscall::syscall_get_dir_children};

#[unsafe(no_mangle)]
pub extern "Rust" fn main(args : Args<'_>) -> i32 {
    let dir = match args.get(1) {
        Some(dir) => dir,
        None => ".",
    };
    
    let current_dir = File::open(dir, READABLE).unwrap();
    let mut children = [DirChild::zeroed(); 16];
    loop {
        let n = syscall_get_dir_children(unsafe { current_dir.get_fd() }, &mut children).unwrap();
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
    //println!("return ls");
    0
}