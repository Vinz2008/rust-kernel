#![no_std]
#![no_main]


#![feature(custom_test_frameworks)]
#![test_runner(crate::tests::test_main)]
#![reexport_test_harness_main = "test_main"]

#![feature(abi_x86_interrupt)]

use crate::utils::hlt_loop;


mod tests;

mod utils;

mod panic;

mod vga;

mod qemu;

mod serial;

mod interrupts;
mod pic;

mod gdt;

mod cli;

#[no_mangle]
pub extern "C" fn _start() -> ! {
    //println!("Hello World{}", "!");


    #[cfg(test)]
    test_main();

    gdt::init();
    interrupts::init_idt();

    unsafe { pic::PICS.lock().initialize() };

    x86_64::instructions::interrupts::enable();

    cli::init_cli();

    serial_println!("test");

    hlt_loop();
}
