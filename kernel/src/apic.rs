use acpi::{AcpiError, AcpiTables, platform::{AcpiPlatform, InterruptModel}};
use x86_64::{instructions::interrupts::without_interrupts, registers::model_specific::{ApicBase, ApicBaseFlags}};

use crate::{acpi::MapHandler, serial_println};

// TODO : finish this

fn _init_apic(acpi_tables : AcpiTables<MapHandler>) -> Result<(), AcpiError> {
    

    let handler = MapHandler;
    let platform = AcpiPlatform::new(acpi_tables, handler)?;

    let apic = match platform.interrupt_model {
        InterruptModel::Apic(apic) => apic,
        InterruptModel::Unknown => panic!("unknown interrupt model"),
        _ => unreachable!(),
    };

    let (frame, mut flags) = ApicBase::read();
    flags.insert(ApicBaseFlags::LAPIC_ENABLE);
    flags.remove(ApicBaseFlags::X2APIC_ENABLE); // TODO : support for X2APIC

    unsafe {
        ApicBase::write(frame, flags);
    }

    let lapic_physical_addr = frame.start_address();

    for io_apic in &apic.io_apics {
        serial_println!("apic io : id = {}, address = 0x{:x}, global_system_interrupt_base = {}", io_apic.id, io_apic.address, io_apic.global_system_interrupt_base);
    }

    Ok(())
}

pub fn init_apic(acpi_tables : AcpiTables<MapHandler>) -> Result<(), AcpiError> {
    without_interrupts(|| _init_apic(acpi_tables))
}