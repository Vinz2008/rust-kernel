use crate::{serial_println, symbols};

#[unsafe(no_mangle)]
pub static mut __stack_chk_guard: usize = 0;

#[unsafe(naked)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __stack_chk_fail() -> ! {
    core::arch::naked_asm!(
        "mov rdi, [rsp]",   // return address = instruction after call
        "mov rsi, rbp",     // failing function's rbp
        "jmp {handler}",
        handler = sym stack_chk_fail_handler,
    );
}

extern "C" fn stack_chk_fail_handler(return_address: usize, rbp: usize) -> ! {
    //panic!("kernel stack smashing detected");
    let saved_guard = unsafe {
        *((rbp - 8) as *const usize)
    };

    let expected_guard = unsafe {
        __stack_chk_guard
    };
    if let Some((name, offset)) = symbols::lookup_symbol(return_address){
        serial_println!("*** STACK CHK FAIL *** at {}+{:#x} ({:#x})", name, offset, return_address);
    } else {
        serial_println!("*** STACK CHK FAIL *** at {:#x}", return_address);
    }

    serial_println!(
        "rbp={:#x}, canary_addr={:#x}",
        rbp,
        rbp - 8
    );

    serial_println!(
        "saved={:#x}, expected={:#x}",
        saved_guard,
        expected_guard
    );
    
    loop {
        core::hint::spin_loop();
    }
}