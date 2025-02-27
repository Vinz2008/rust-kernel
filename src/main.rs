#![no_std]
#![no_main]


#![feature(custom_test_frameworks)]
#![test_runner(crate::tests::test_main)]
#![reexport_test_harness_main = "test_main"]

#![feature(abi_x86_interrupt)]


mod tests;


mod panic;

mod vga;

mod qemu;

mod serial;

mod interrupts;

mod gdt;


#[no_mangle]
pub extern "C" fn _start() -> ! {
    println!("Hello World{}", "!");

    #[cfg(test)]
    test_main();

    gdt::init();
    interrupts::init_idt();

    loop {}
}
