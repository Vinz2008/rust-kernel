#![no_std]
#![no_main]


#![feature(custom_test_frameworks)]
#![test_runner(crate::tests::test_main)]
#![reexport_test_harness_main = "test_main"]

#![feature(abi_x86_interrupt)]

extern crate alloc;

//use alloc::{boxed::Box, rc::Rc, vec::Vec, vec};
use bootloader::{BootInfo, entry_point};
use elf::{ElfBytes, endian::AnyEndian};
use x86_64::VirtAddr;

use crate::utils::hlt_loop;


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

mod initrd;

mod cli;

entry_point!(kernel_main);

const INITRD_BYTES : &[u8] = include_bytes!("../initrd.tar");


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

    /*let heap_value = Box::new(41);
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
    println!("reference count is {} now", Rc::strong_count(&cloned_reference));*/

    // TODO : move this initrd.rs (??)
    let tar_initrd = initrd::TarInitrd::new(INITRD_BYTES).expect("invalid tar");
    for (idx, &file) in tar_initrd.headers.iter().enumerate() {
        serial_println!("file {} {} {}", idx, file.get_filename().unwrap(), file.size().unwrap());
    }

    let init_file_header = *tar_initrd.headers.iter().find(|e| e.get_filename().unwrap() == "./init").unwrap();
    let init_content = init_file_header.content().unwrap();
    let file = ElfBytes::<AnyEndian>::minimal_parse(init_content).expect("Error when parsing init elf");

    let text_section_header = file.section_header_by_name(".text").unwrap().unwrap();
    serial_println!("text section content : {:?}", file.section_data(&text_section_header).unwrap().0);

    cli::init_cli();

    serial_println!("test");

    hlt_loop();
}
