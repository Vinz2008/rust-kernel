#![no_std]
#![no_main]


#![feature(custom_test_frameworks)]
#![test_runner(crate::tests::test_main)]
#![reexport_test_harness_main = "test_main"]

#![feature(abi_x86_interrupt)]

extern crate alloc;

//use alloc::{boxed::Box, rc::Rc, vec::Vec, vec};
use bootloader::{BootInfo, entry_point};
use x86_64::VirtAddr;

use crate::{initrd::load_initrd_init, utils::hlt_loop};


mod tests;

mod utils;

mod panic;

mod vga;

mod qemu;

mod serial;

mod backtrace;

mod interrupts;
mod pic;

mod gdt;

mod paging;
mod allocator;

mod elf;

mod syscall;

mod process;

mod userspace;

mod initrd;

mod cli;

entry_point!(kernel_main);


fn kernel_main(boot_info: &'static BootInfo) -> ! {

    #[cfg(test)]
    test_main();

    gdt::init();
    interrupts::init_idt();

    unsafe { pic::PICS.lock().initialize() };

    x86_64::instructions::interrupts::enable();

    let phys_mem_offset = VirtAddr::new(boot_info.physical_memory_offset);

    let mapper = unsafe { paging::init(phys_mem_offset) };
    let frame_allocator = unsafe { paging::BootInfoFrameAllocator::init(&boot_info.memory_map) };

    allocator::init_heap(mapper, frame_allocator).expect("heap initialization failed");

    
    load_initrd_init();
    
    /*cli::init_cli();

    serial_println!("test");*/

    hlt_loop();
}
