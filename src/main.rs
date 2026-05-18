#![no_std]
#![no_main]


#![feature(custom_test_frameworks)]
#![test_runner(crate::tests::test_main)]
#![reexport_test_harness_main = "test_main"]

#![feature(abi_x86_interrupt)]

use bootloader::{BootInfo, entry_point};
use x86_64::{VirtAddr, structures::paging::Translate};

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

    let mapper = unsafe { paging::init(phys_mem_offset) };
    
    let addresses = [
        // the identity-mapped vga buffer page
        0xb8000,
        // some code page
        0x201008,
        // some stack page
        0x0100_0020_1a10,
        // virtual address mapped to physical address 0
        boot_info.physical_memory_offset,
    ];

    for &address in &addresses {
        let virt = VirtAddr::new(address);
        // new: use the `mapper.translate_addr` method
        let phys = mapper.translate_addr(virt);
        println!("{:?} -> {:?}", virt, phys);
    }


    cli::init_cli();

    serial_println!("test");

    hlt_loop();
}
