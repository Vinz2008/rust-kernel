use crate::{println, syscall::syscall_exit};

#[unsafe(no_mangle)]
pub static mut __stack_chk_guard: usize = 0;

#[unsafe(no_mangle)]
pub extern "C" fn __stack_chk_fail() -> ! {
    println!("stack chk fail");
    syscall_exit(127)
}