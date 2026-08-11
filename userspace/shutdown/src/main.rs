#![no_std]
#![no_main]

use rt::{self as _, Args, power::{ShutdownResult, shutdown}};

#[unsafe(no_mangle)]
pub extern "Rust" fn main(args : Args<'_>) -> i32 {
    // TODO : add help (also add it in the other crates)
    // TODO : maybe even create a crate to help add flags, which could autogenerate help ?
    let mut res = ShutdownResult::Success;
    if let Some(arg) = args.get(1) {
        match arg {
            "--failure" => {
                res = ShutdownResult::Failure;
            },
            "--success" => {
                res = ShutdownResult::Success;
            }
            _ => {}
        }
    }
    shutdown(res)
}