use core::panic::PanicInfo;



#[cfg(not(test))]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    use crate::{utils::hlt_loop, vga::WRITER, serial::SERIAL1, backtrace::Backtrace};
    use core::fmt::Write;
    use x86_64::instructions::interrupts;
    
    if let Some(mut writer_lock) = WRITER.try_lock(){
        writer_lock.clear_screen();
        let _ = writeln!(writer_lock, "{}", info);
    }

    
    if let Some(mut serial_lock) = SERIAL1.try_lock() {
        let backtrace = Backtrace::new();
        interrupts::without_interrupts(|| serial_lock.write_fmt(format_args!("backtrace {}", backtrace)).unwrap());
    }
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
