#[cfg(test)]
use crate::qemu::{QemuExitCode, exit_qemu};

use crate::{serial_print, serial_println};

#[cfg(test)]
pub fn test_main(tests: &[&dyn Testable]) {
    use crate::qemu::{QemuExitCode, exit_qemu};
    use crate::serial_println;

    serial_println!("Running {} tests", tests.len());
    for test in tests {
        test.run();
    }
    exit_qemu(QemuExitCode::Success);
}

pub trait Testable {
    fn run(&self) -> ();
}

impl<T> Testable for T
where
    T: Fn(),
{
    fn run(&self) {
        serial_print!("{}...\t", core::any::type_name::<T>());
        self();
        serial_println!("[ok]");
    }
}



#[test_case]
fn trivial_assertion() {
    assert_eq!(1, 1);
}