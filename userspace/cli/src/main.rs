#![no_std]
#![no_main]

use arrayvec::ArrayString;
use rt::{self as _, syscall::{syscall_get_char, syscall_print}};


#[unsafe(no_mangle)]
pub extern "Rust" fn main() -> i32 {
    let mut cli : ArrayString<10000> = ArrayString::new();
    syscall_print("> ");

    loop {
        let c = syscall_get_char();
        match c {
            '\n' => {
                syscall_print("\nentered : ");
                syscall_print(&cli);
                syscall_print("\n");
                syscall_print("> ");
                cli.clear();
            },
            _ => {
                if let Ok(_) = cli.try_push(c) {
                    let mut dst = [0; 4];
                    //syscall_print("\ntest get_char");
                    syscall_print(c.encode_utf8(&mut dst));
                    //syscall_print("\n");
                }
            }
        }
        
    }
}