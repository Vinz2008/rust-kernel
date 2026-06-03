#![no_std]
#![no_main]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

#[allow(deref_nullptr)]

#[unsafe(no_mangle)]
pub fn _start() {
    core::hint::black_box( unsafe { *(0x0 as *const u64) });
    loop {}
}
