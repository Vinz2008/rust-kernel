#![no_std]

#![feature(naked_functions_rustic_abi)]

pub extern crate alloc;

use core::{arch::naked_asm, str};

use alloc::slice;
pub use shared_consts;
use shared_consts::Arg;

use crate::syscall::syscall_exit;

mod panic;
mod allocator;
pub mod syscall;
pub mod print;

// TODO : allocator

unsafe extern "Rust" {
    fn main(args : Args<'_>) -> i32;
}

pub struct Args<'a> {
    args : &'a [Arg],
}

fn get_str_from_arg(arg : &Arg) -> &str {
    unsafe {
        let slice = slice::from_raw_parts(arg.ptr, arg.len);
        str::from_utf8_unchecked(slice) // TODO : should I check (for now args are forced to be utf8, should I change it ?)
    }
}

impl<'a> Args<'a> {
    pub fn get(&self, idx : usize) -> Option<&'a str> {
        let arg = self.args.get(idx)?;
        Some(get_str_from_arg(arg))
    }

    pub fn iter(&self) -> impl Iterator<Item = &'a str> + 'a {
        self.args.iter().map(get_str_from_arg)
    }
}

fn start_rt(initial_rsp: *const usize){
    let args = unsafe {
        let argc = initial_rsp.read();
        let argv_ptr = initial_rsp.add(1) as *const Arg;
        slice::from_raw_parts(argv_ptr, argc)
    };

    let args = Args {
        args,
    };

    let exit = unsafe { main(args) };
    syscall_exit(exit)
}

#[unsafe(no_mangle)]
#[unsafe(naked)]
pub fn _start() -> ! {
    naked_asm!(
        "mov rdi, rsp",
        "call {rust_start}",
        rust_start = sym start_rt,
    )
}

