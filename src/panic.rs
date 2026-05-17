use core::panic::PanicInfo;


#[cfg(not(test))]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    use crate::{println, utils::hlt_loop, vga::WRITER};
    if let Some(mut writer_lock) = WRITER.try_lock(){
        writer_lock.reset();
    }
    println!("{}", info);
    hlt_loop()
}

#[cfg(test)]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    use crate::serial_println;
    use crate::qemu::{QemuExitCode, exit_qemu};
    use crate::utils::hlt_loop;

    serial_println!("[failed]\n");
    serial_println!("Error: {}\n", info);
    exit_qemu(QemuExitCode::Failed);
    hlt_loop()
}
