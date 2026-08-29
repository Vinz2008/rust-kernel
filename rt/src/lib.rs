#![no_std]

#![allow(internal_features)]

#![feature(naked_functions_rustic_abi)]
#![feature(fmt_internals)]

pub extern crate alloc;

use core::{arch::naked_asm, str};

use alloc::slice;
pub use shared_consts;
use shared_consts::{Arg, RNG_SEED_SIZE};

use crate::{random::init_rng_seed, stack_chk::__stack_chk_guard, syscall::syscall_exit};

pub use arrayvec;

mod panic;
mod stack_chk;
mod allocator;
pub mod syscall;
pub mod print;
pub mod input;
pub mod fs;
pub mod random;
pub mod power;

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

fn start_rt(initial_rsp: *const u8){
    let (random_seed, args) = unsafe {
        let rsp_after_stack_canary = initial_rsp.add(8) as *const u8;
        let random_seed = slice::from_raw_parts(rsp_after_stack_canary, RNG_SEED_SIZE);
        let rsp_after_random = rsp_after_stack_canary.add(RNG_SEED_SIZE) as *const usize;
        let argc = rsp_after_random.read();
        let argv_ptr = rsp_after_random.add(1) as *const Arg;
        let args = slice::from_raw_parts(argv_ptr, argc);
        (random_seed, args)
    };

    init_rng_seed(random_seed);

    // TODO : use the random seed

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
        "mov rax, [rsp]",
        "lea rcx, [rip + {stack_guard}]",
        "mov [rcx], rax",

        "mov rdi, rsp",
        "call {rust_start}",
        stack_guard = sym __stack_chk_guard,
        rust_start = sym start_rt,
    )
}

