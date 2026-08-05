
use core::panic::PanicInfo;

use crate::{print::_print, syscall::syscall_exit};


// TODO : improve this
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    let _ = _print(format_args!("panic : {}\n", info));
    syscall_exit(-1)
}