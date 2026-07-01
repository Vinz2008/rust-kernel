
use core::panic::PanicInfo;


// TODO : improve this
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}