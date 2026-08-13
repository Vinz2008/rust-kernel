use acpi::{address::AddressSpace, sdt::fadt::Fadt};
use shared_consts::SHUTDOWN_SUCCESS;
use x86_64::instructions::{interrupts, port::{Port, PortWriteOnly}};

use crate::{acpi::ACPI_PLATFORM, paging::PHYSICAL_MEMORY_OFFSET, qemu::{self, QemuExitCode}};

pub fn shutdown(flags : u64) -> ! {
    interrupts::disable();
    let is_success = (flags & SHUTDOWN_SUCCESS) != 0;
    
    // TODO : acpi shutdown

    let status = if is_success {
        QemuExitCode::Success
    } else {
        QemuExitCode::Failed
    };
    qemu::exit_qemu(status)
}

pub fn reboot() -> ! {

    interrupts::disable();

    let fadt = ACPI_PLATFORM.get().unwrap().tables.find_table::<Fadt>().unwrap();

    const FADT_RESET_VALUE_END: u32 = 129;

    if fadt.header.length >= FADT_RESET_VALUE_END {
        let reset_reg = fadt.reset_register().unwrap();
        let value = fadt.reset_value;
        
        // TODO : maybe make this a dedicated write_gas_u8 (gas for generic address structure) if this type of code is used elsewhere
        match reset_reg.address_space {
            AddressSpace::SystemIo => unsafe {
                PortWriteOnly::<u8>::new(reset_reg.address as u16).write(value);
            },
            AddressSpace::SystemMemory => unsafe {
                let ptr = (PHYSICAL_MEMORY_OFFSET + reset_reg.address).as_mut_ptr::<u8>();
                ptr.write_volatile(value);
            },
            _ => unimplemented!(), // TODO : support others especially like pci (https://wiki.osdev.org/Reboot), etc
        }
    }

    // use the keyboard controller command port to reboot (it is 8042 reset, used only if the other one doesn't work)
    let mut good = 0x02;
    let mut keyboard_port = Port::<u8>::new(0x64);
    while (good & 0x02) != 0 {
        good = unsafe { keyboard_port.read() };
    }

    unsafe {
        keyboard_port.write(0xFE);
    }

    // TODO : to be even more sure, do a PCI/ICH reset (port 0xCF9, but verify if supported ?)

    // TODO : to be even be more sure, do triple fault ?

    panic!("couldn't reboot computer")
}