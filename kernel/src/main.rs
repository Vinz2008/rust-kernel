#![no_std]
#![no_main]


#![feature(custom_test_frameworks)]
#![test_runner(crate::tests::test_main)]
#![reexport_test_harness_main = "test_main"]

#![feature(abi_x86_interrupt)]

#![allow(clippy::let_and_return)]

extern crate alloc;

use bootloader::{BootInfo, entry_point};

use crate::{acpi::init_acpi, apic::init_apic, gdt::init_tss, initrd::load_initrd_init, msr::enable_syscall, pcie::init_pcie, process::Process, random::init_kernel_rng, rtc::{BOOT_TIME, init_rtc}, security::enable_security_features, sse::init_fpu_template, utils::hlt_loop};


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

mod rtc;

mod sse;

mod mmio;
mod msr;
mod acpi;
mod apic;

mod pcie;
mod ahci;

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

mod ringbuf;

mod stack_chk;
mod security;

mod random;
mod power;


entry_point!(kernel_main);

// TODO : enable stack smashing protection for userspace exes

// TODO : for security reasons, make the kernel a PIE ? (and the userspace programs ?) and enable KASLR ? (also ALSR for userspace programs ?)

// TODO : add permissions for memory but also in userspace (not only just in page tables that only the kernel can modify), PKU feature

// TODO : compile the kernel with BIND_NOW (does the bootloader support it ?)

// TODO : add brk ASLR ?

// TODO : add libs/mmap ASLR

// TODO : stack ASLR ?

// TODO : use pointer encryption/obfuscation for most secure cases : https://udrepper.livejournal.com/13393.html

// TODO : harden memory allocators (poison, etc)

// TODO : TPM support for securing keys (other solutions ?)

// TODO : after adding mmap, have a minimum address for it (for ex 32K or 64K) to prevent NULL pointer attacks

// TODO : port doom (need to implement framebuffer)

// TODO : add limits for processes (of memory use, of syscalls, of filesystem access (kind of like chroot ?), etc) that can be set when doing exec (need a exec config struct)

// TODO : maybe in a very far future, support FRED (new technology to replace interrupts, syscalls, etc, faster, and because simpler is a little more secure) but disadvantage is as 2026, the cpus supporting have only been released a couple of months ago, and only intel cpus supports it for now

fn kernel_main(boot_info: &'static BootInfo) -> ! {

    #[cfg(test)]
    test_main();

    gdt::init();
    interrupts::init_idt();

    init_tss();

    init_fpu_template();

    init_kernel_rng();

    enable_security_features();

    let mapper = unsafe { paging::init() };
    let boot_frame_allocator = unsafe { paging::BootInfoFrameAllocator::init(&boot_info.memory_map) };

    allocator::init_heap(mapper, boot_frame_allocator).expect("heap initialization failed");

    init_acpi().unwrap();

    init_rtc();

    serial_println!("boot time : {}", BOOT_TIME.get().unwrap());

    init_apic().unwrap();

    x86_64::instructions::interrupts::enable();

    init_pcie();

    enable_syscall();

    Process::init_idle_process();
    
    load_initrd_init();

    hlt_loop();
}
