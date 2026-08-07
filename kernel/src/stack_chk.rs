use crate::serial_println;

#[unsafe(no_mangle)]
pub static mut __stack_chk_guard: usize = 0;

#[unsafe(no_mangle)]
pub extern "C" fn __stack_chk_fail() -> ! {
    //panic!("kernel stack smashing detected");
    serial_println!("*** STACK CHK FAIL ***");
    loop {
        core::hint::spin_loop();
    }
}