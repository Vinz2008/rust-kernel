use core::{cell::UnsafeCell, ptr::{read_volatile, write_volatile}};

use spin::Mutex;
use x86_64::{VirtAddr, structures::paging::{PageSize, Size4KiB}};

#[repr(transparent)]
pub struct MmioRegister<T> {
    val : UnsafeCell<T>,
}

impl<T : Copy> MmioRegister<T> {
    #[inline]
    pub fn read(&self) -> T {
        unsafe { read_volatile(self.val.get()) }
    }

    // after calling this, read the id to ensure the write
    #[inline]
    pub fn write(&self, val : T){
        unsafe { write_volatile(self.val.get(), val); }
    }

    #[inline]
    pub fn update<F>(&self, f : F)
    where F : FnOnce(T) -> T 
    {
        let r = self.read();
        let updated = f(r);
        self.write(updated);
    }
}

// used for reserved types, can't read or write it
#[repr(transparent)]
pub struct Reserved<T>(T);

pub const MMIO_START: VirtAddr = VirtAddr::new(0xffff_a000_0000_0000);
pub const MMIO_END : VirtAddr = VirtAddr::new(0xffff_a100_0000_0000);

static NEXT_MMIO_ADDR : Mutex<VirtAddr> = Mutex::new(MMIO_START);

pub fn alloc_virtual_pages(pages_count : u64) -> Option<VirtAddr> {
    let pages_size = pages_count.checked_mul(Size4KiB::SIZE)?;
    let mut next_mmio_addr_lock = NEXT_MMIO_ADDR.lock();
    let next_mmio_addr = *next_mmio_addr_lock;
    let new_next_mmio_addr = next_mmio_addr + pages_size;
    if new_next_mmio_addr > MMIO_END {
        return None;
    }

    *next_mmio_addr_lock = new_next_mmio_addr;
    Some(next_mmio_addr)
}