
use core::panic::PanicInfo;

use crate::{println, syscall::syscall_exit};


// TODO : improve this
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    println!("{}", info);
    syscall_exit(-1)
}