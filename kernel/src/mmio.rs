use core::{cell::UnsafeCell, ptr::{read_volatile, write_volatile}};

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
}
