#![no_std]
#![no_main]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}


// TODO : add a rt crate to do syscall on the userspace side, can use it after for the std ?

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

    
    loop {
        let message = "test";
        unsafe {
            core::arch::asm!(
                "int 0x80", 
                in("rax") 1,
                in("rdi") message.as_ptr() as usize,
                in("rsi") message.len(),
            );
        }
    }
}


static mut TEST_BSS: [u8; 4096] = [0; 4096];

#[unsafe(no_mangle)]
pub fn _start() -> ! {
    main();
    unsafe {
        core::arch::asm!(
            "int 0x80",
            in("rax") 0,
        );
    }
    loop {}
}
