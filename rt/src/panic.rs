
use core::panic::PanicInfo;

use crate::println;


// TODO : improve this
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    println!("{}", info);
    loop {}
}