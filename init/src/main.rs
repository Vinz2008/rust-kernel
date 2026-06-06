#![no_std]
#![no_main]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}


// for now, not special function, just normal function (need to make the _start function when porting the std, TODO)
#[allow(deref_nullptr)]
//#[allow(static_mut_refs)]
#[unsafe(no_mangle)]
fn main() -> i32 {
    /*unsafe {
        for (i, x) in TEST_BSS.iter().enumerate() {
            assert_eq!(*x, 0, "bss byte {} is {}", i, x);
        }
    }*/
    // core::hint::black_box( unsafe { *(0x0 as *const u64) });
    /*unsafe {
        core::arch::asm!("ud2");
    }*/
    0
}


static mut TEST_BSS: [u8; 4096] = [0; 4096];

#[unsafe(no_mangle)]
pub fn _start() {
    main();
    loop {} // TODO : after having syscalls, add the exit syscall here
}
