#![no_std]
#![no_main]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}


#[allow(deref_nullptr)]
#[unsafe(no_mangle)]
fn main() -> i32 {
    let message = "test";
    unsafe {
        core::arch::asm!(
            "int 0x80", 
            in("rax") 1,
            in("rdi") message.as_ptr() as usize,
            in("rsi") message.len(),
            lateout("rax") _,
            clobber_abi("C"),
            options(nostack),
        );
    }
    loop {}
}

#[unsafe(no_mangle)]
pub fn _start() -> ! {
    main();
    unsafe {
        core::arch::asm!(
            "int 0x80",
            in("rax") 0,
            options(noreturn),
        );
    }
}
