//! This library part of the bootloader allows kernels to retrieve information from the bootloader.
//!
//! To combine your kernel with the bootloader crate you need a tool such
//! as [`bootimage`](https://github.com/rust-osdev/bootimage). See the
//! [_Writing an OS in Rust_](https://os.phil-opp.com/minimal-rust-kernel/#creating-a-bootimage)
//! blog for an explanation.

#![no_std]
#![warn(missing_docs)]

pub use crate::bootinfo::BootInfo;

pub mod bootinfo;

#[cfg(target_arch = "x86")]
compile_error!(
    "This crate currently does not support 32-bit protected mode. \
         See https://github.com/rust-osdev/bootloader/issues/70 for more information."
);

#[cfg(not(any(target_arch = "x86_64", target_arch = "x86")))]
compile_error!("This crate only supports the x86_64 architecture.");

/// Defines the entry point function.
///
/// The function must have the signature `fn(&'static BootInfo) -> !`.
///
/// This macro just creates a function named `_start`, which the linker will use as the entry
/// point. The advantage of using this macro instead of providing an own `_start` function is
/// that the macro ensures that the function and argument types are correct.
#[macro_export]
macro_rules! entry_point {
    ($path:path) => {
        unsafe extern "C" {
            static mut __stack_chk_guard: usize;
        }
        #[export_name = "_start"]
        #[unsafe(naked)]
        pub extern "C" fn __real_start(_boot_info: &'static $crate::bootinfo::BootInfo) -> ! {
            // TODO : setup stack_chk, call __impl_start
            core::arch::naked_asm!(
                "mov rax, [rdi + {guard_offset}]",
                "mov [rip + {stack_guard}], rax",
                "jmp {start}",
                guard_offset = const core::mem::offset_of!($crate::bootinfo::BootInfo, rand_stack_guard),
                stack_guard = sym __stack_chk_guard,
                start = sym __impl_start
            )
        }

        fn __impl_start(boot_info: &'static $crate::bootinfo::BootInfo) -> ! {
            // validate the signature of the program entry point
            let f: fn(&'static $crate::bootinfo::BootInfo) -> ! = $path;

            f(boot_info)
        }
    };
}
