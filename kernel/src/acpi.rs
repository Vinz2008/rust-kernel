use core::ptr::NonNull;

use acpi::{AcpiError, AcpiTables, Handler, platform::AcpiPlatform};
use spin::Once;

use crate::{paging::PHYSICAL_MEMORY_OFFSET};

#[derive(Clone, Copy)]
pub struct MapHandler;

impl Handler for MapHandler {
    unsafe fn map_physical_region<T>(&self, physical_address: usize, size: usize) -> acpi::PhysicalMapping<Self, T> {
        let phys_offset = PHYSICAL_MEMORY_OFFSET.as_u64() as usize;
        let virt_of_phys = phys_offset + physical_address;
        let virtual_start = NonNull::new(virt_of_phys as *mut T).unwrap();

        acpi::PhysicalMapping {
            physical_start: physical_address,
            virtual_start,
            region_length: size,
            mapped_length: size,
            handler: *self,
        }
    }

    fn unmap_physical_region<T>(_region: &acpi::PhysicalMapping<Self, T>) {
        // nothing needed
    }

    fn read_u8(&self, _address: usize) -> u8 {
        todo!()
    }

    fn read_u16(&self, _address: usize) -> u16 {
        todo!()
    }

    fn read_u32(&self, _address: usize) -> u32 {
        todo!()
    }

    fn read_u64(&self, _address: usize) -> u64 {
        todo!()
    }

    fn write_u8(&self, _address: usize, _value: u8) {
        todo!()
    }

    fn write_u16(&self, _address: usize, _value: u16) {
        todo!()
    }

    fn write_u32(&self, _address: usize, _value: u32) {
        todo!()
    }

    fn write_u64(&self, _address: usize, _value: u64) {
        todo!()
    }

    fn read_io_u8(&self, _port: u16) -> u8 {
        todo!()
    }

    fn read_io_u16(&self, _port: u16) -> u16 {
        todo!()
    }

    fn read_io_u32(&self, _port: u16) -> u32 {
        todo!()
    }

    fn write_io_u8(&self, _port: u16, _value: u8) {
        todo!()
    }

    fn write_io_u16(&self, _port: u16, _value: u16) {
        todo!()
    }

    fn write_io_u32(&self, _port: u16, _value: u32) {
        todo!()
    }

    fn read_pci_u8(&self, _address: acpi::PciAddress, _offset: u16) -> u8 {
        todo!()
    }

    fn read_pci_u16(&self, _address: acpi::PciAddress, _offset: u16) -> u16 {
        todo!()
    }

    fn read_pci_u32(&self, _address: acpi::PciAddress, _offset: u16) -> u32 {
        todo!()
    }

    fn write_pci_u8(&self, _address: acpi::PciAddress, _offset: u16, _value: u8) {
        todo!()
    }

    fn write_pci_u16(&self, _address: acpi::PciAddress, _offset: u16, _value: u16) {
        todo!()
    }

    fn write_pci_u32(&self, _address: acpi::PciAddress, _offset: u16, _value: u32) {
        todo!()
    }

    fn nanos_since_boot(&self) -> u64 {
        todo!()
    }

    fn stall(&self, _microseconds: u64) {
        todo!()
    }

    fn sleep(&self, _milliseconds: u64) {
        todo!()
    }

    fn create_mutex(&self) -> acpi::Handle {
        todo!()
    }

    fn acquire(&self, _mutex: acpi::Handle, _timeout: u16) -> Result<(), acpi::aml::AmlError> {
        todo!()
    }

    fn release(&self, _mutex: acpi::Handle) {
        todo!()
    }
}

pub static ACPI_PLATFORM : Once<AcpiPlatform<MapHandler>> = Once::new();

pub fn init_acpi(rsdp_addr : u64) -> Result<(), AcpiError> {
    let handler = MapHandler;
    let acpi_tables = unsafe { AcpiTables::from_rsdp(handler, rsdp_addr as usize)? };
    let acpi_platform = AcpiPlatform::new(acpi_tables, handler)?;

    ACPI_PLATFORM.call_once(|| acpi_platform);

    Ok(())
}