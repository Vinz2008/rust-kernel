use core::ptr::NonNull;

use x86_64::{PhysAddr, registers::control::Cr3};
use x86_64::structures::paging::{PageTable, PhysFrame, Size4KiB};

pub fn allocate_level4_page_table(new_p4_addr : NonNull<u8>) -> PhysAddr {
    let new_p4_phys = PhysAddr::new(new_p4_addr.as_ptr() as u64);

    let (firmware_p4_frame, cr3_flags) = Cr3::read();

    let firmware_p4_phys = firmware_p4_frame.start_address();
    let firmware_p4 = unsafe { &*(firmware_p4_phys.as_u64() as *const PageTable) };

    let new_p4 = unsafe { &mut *(new_p4_phys.as_u64() as *mut PageTable) };

    for i in 0..512 {
        new_p4[i] = firmware_p4[i].clone();
    }

    let new_p4_frame = PhysFrame::<Size4KiB>::containing_address(new_p4_phys);

    unsafe {
        Cr3::write(new_p4_frame, cr3_flags);
    }
    new_p4_phys
}