#![no_std]
#![no_main]


#![feature(custom_test_frameworks)]
#![test_runner(crate::tests::test_main)]
#![reexport_test_harness_main = "test_main"]

#![feature(abi_x86_interrupt)]

extern crate alloc;

use alloc::{boxed::Box, rc::Rc, vec::Vec, vec};
use bootloader::{BootInfo, entry_point};
use x86_64::{VirtAddr, structures::paging::{Page, Translate}};

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

mod paging;
mod allocator;

mod cli;

entry_point!(kernel_main);


fn kernel_main(boot_info: &'static BootInfo) -> ! {
    //println!("Hello World{}", "!");

    #[cfg(test)]
    test_main();

    gdt::init();
    interrupts::init_idt();

    unsafe { pic::PICS.lock().initialize() };

    x86_64::instructions::interrupts::enable();

    let phys_mem_offset = VirtAddr::new(boot_info.physical_memory_offset);

    let mut mapper = unsafe { paging::init(phys_mem_offset) };
    let mut frame_allocator = unsafe { paging::BootInfoFrameAllocator::init(&boot_info.memory_map) };

    allocator::init_heap(&mut mapper, &mut frame_allocator).expect("heap initialization failed");

    let heap_value = Box::new(41);
    println!("heap_value at {:p}", heap_value);

    let mut vec = Vec::new();
    for i in 0..500 {
        vec.push(i);
    }
    println!("vec at {:p}", vec.as_slice());

    let reference_counted = Rc::new(vec![1, 2, 3]);
    let cloned_reference = reference_counted.clone();
    println!("current reference count is {}", Rc::strong_count(&cloned_reference));
    core::mem::drop(reference_counted);
    println!("reference count is {} now", Rc::strong_count(&cloned_reference));

    cli::init_cli();

    serial_println!("test");

    hlt_loop();
}
