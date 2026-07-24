use core::ptr::NonNull;

use acpi::{AcpiError, AcpiTables, Handler, rsdp::Rsdp};

use crate::{paging::PHYSICAL_MEMORY_OFFSET};

#[derive(Clone, Copy)]
pub struct MapHandler;

impl Handler for MapHandler {
    unsafe fn map_physical_region<T>(&self, physical_address: usize, size: usize) -> acpi::PhysicalMapping<Self, T> {
        let phys_offset = PHYSICAL_MEMORY_OFFSET.get().unwrap().as_u64() as usize;
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

    fn unmap_physical_region<T>(region: &acpi::PhysicalMapping<Self, T>) {
        // nothing needed
    }

    fn read_u8(&self, address: usize) -> u8 {
        todo!()
    }

    fn read_u16(&self, address: usize) -> u16 {
        todo!()
    }

    fn read_u32(&self, address: usize) -> u32 {
        todo!()
    }

    fn read_u64(&self, address: usize) -> u64 {
        todo!()
    }

    fn write_u8(&self, address: usize, value: u8) {
        todo!()
    }

    fn write_u16(&self, address: usize, value: u16) {
        todo!()
    }

    fn write_u32(&self, address: usize, value: u32) {
        todo!()
    }

    fn write_u64(&self, address: usize, value: u64) {
        todo!()
    }

    fn read_io_u8(&self, port: u16) -> u8 {
        todo!()
    }

    fn read_io_u16(&self, port: u16) -> u16 {
        todo!()
    }

    fn read_io_u32(&self, port: u16) -> u32 {
        todo!()
    }

    fn write_io_u8(&self, port: u16, value: u8) {
        todo!()
    }

    fn write_io_u16(&self, port: u16, value: u16) {
        todo!()
    }

    fn write_io_u32(&self, port: u16, value: u32) {
        todo!()
    }

    fn read_pci_u8(&self, address: acpi::PciAddress, offset: u16) -> u8 {
        todo!()
    }

    fn read_pci_u16(&self, address: acpi::PciAddress, offset: u16) -> u16 {
        todo!()
    }

    fn read_pci_u32(&self, address: acpi::PciAddress, offset: u16) -> u32 {
        todo!()
    }

    fn write_pci_u8(&self, address: acpi::PciAddress, offset: u16, value: u8) {
        todo!()
    }

    fn write_pci_u16(&self, address: acpi::PciAddress, offset: u16, value: u16) {
        todo!()
    }

    fn write_pci_u32(&self, address: acpi::PciAddress, offset: u16, value: u32) {
        todo!()
    }

    fn nanos_since_boot(&self) -> u64 {
        todo!()
    }

    fn stall(&self, microseconds: u64) {
        todo!()
    }

    fn sleep(&self, milliseconds: u64) {
        todo!()
    }

    fn create_mutex(&self) -> acpi::Handle {
        todo!()
    }

    fn acquire(&self, mutex: acpi::Handle, timeout: u16) -> Result<(), acpi::aml::AmlError> {
        todo!()
    }

    fn release(&self, mutex: acpi::Handle) {
        todo!()
    }
}

pub fn init_acpi() -> Result<AcpiTables<MapHandler>, AcpiError> {
    let handler = MapHandler;
    let rdsp = unsafe { Rsdp::search_for_on_bios(handler)? };
    let acpi_tables = unsafe { AcpiTables::from_rsdp(handler, rdsp.physical_start)? };
    
    Ok(acpi_tables)
}