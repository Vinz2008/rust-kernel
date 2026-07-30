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

use crate::{acpi::init_acpi, apic::init_apic, gdt::init_tss, initrd::load_initrd_init, msr::enable_syscall, process::Process, utils::hlt_loop};


mod tests;

mod utils;

mod panic;

mod vga;

mod qemu;

mod serial;

mod backtrace;
mod symbols;

mod interrupts;
mod pic;

mod gdt;

mod msr;
mod acpi;
mod apic;

mod paging;
mod allocator;

mod elf;

mod syscall;

mod process;
mod scheduler;

mod userspace;

mod fs;
mod device;
mod initrd;

mod cli;

mod ringbuf;

entry_point!(kernel_main);


// TODO : port doom (need to implement framebuffer)

fn kernel_main(boot_info: &'static BootInfo) -> ! {

    #[cfg(test)]
    test_main();

    gdt::init();
    interrupts::init_idt();

    init_tss();

    enable_syscall();

    // TODO : remove pic initialization in the future (just disable it using the .lock().disable(), is small enough that I can just implement it without deps)
    unsafe { pic::PICS.lock().initialize() };

    // TODO : should I really use the physical memory offset in the bootinfo ? why not just use the constant one in the kernel Cargo.toml, it would prevent to put PHYSICAL_MEMORY_OFFSET in a Once (remove atomic loads)
    let phys_mem_offset = VirtAddr::new(boot_info.physical_memory_offset);

    let mapper = unsafe { paging::init(phys_mem_offset) };
    let frame_allocator = unsafe { paging::BootInfoFrameAllocator::init(&boot_info.memory_map) };

    allocator::init_heap(mapper, frame_allocator).expect("heap initialization failed");

    let acpi_tables = init_acpi().unwrap();
    
    init_apic(acpi_tables).unwrap();


    Process::init_idle_process();
    
    load_initrd_init();

    hlt_loop();
}
