use rsdp::handler::{AcpiHandler, PhysicalMapping};
use core::ptr::NonNull;

#[derive(Clone, Copy)]
pub struct RsdpHandler {
    pub phys_offset : u64,
}

impl AcpiHandler for RsdpHandler {
    unsafe fn map_physical_region<T>(&self, physical_address: usize, size: usize) -> rsdp::handler::PhysicalMapping<Self, T> {
        let virt_addr = self.phys_offset + physical_address as u64;
        let virtual_start = unsafe {
            NonNull::new_unchecked(virt_addr as *mut T)
        };

        unsafe {
            PhysicalMapping::new(
                physical_address,
                virtual_start,
                size, // requested region length
                size, // actual mapped length
                *self,
            )
        }
    }

    fn unmap_physical_region<T>(region: &rsdp::handler::PhysicalMapping<Self, T>) {
        // nothing
    }
}