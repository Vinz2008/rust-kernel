
use core::panic::PanicInfo;

use crate::{print::{self, _print}, syscall::syscall_exit};

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    let _ = _print(format_args!("panic : {}\n", info));
    let _ = print::flush_stdout();
    syscall_exit(-1)
}