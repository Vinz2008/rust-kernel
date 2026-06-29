use core::{mem::MaybeUninit, ptr};

pub struct RingBuf<T, const N: usize> {
    arr: [MaybeUninit<T>; N],
    start : usize,
    len : usize,
}

impl<T, const N: usize> RingBuf<T, N>{
    pub const fn new() -> RingBuf<T, N> {
        RingBuf {
            arr: [const { MaybeUninit::uninit() }; N],
            start: 0,
            len: 0,
        }
    }

    pub fn push(&mut self, val : T){
        if self.len == N {
            unsafe {
                ptr::drop_in_place(self.arr[self.start].as_mut_ptr());
            }
            self.arr[self.start].write(val);
            self.start = (self.start + 1) % N;
        } else {
            let end = (self.start + self.len) % N;
            self.arr[end].write(val);
            self.len += 1;
        }
    }
    
    pub fn pop(&mut self) -> Option<T> {
        if self.len == 0 {
            return None;
        }
        let popped_val = unsafe {
            self.arr[self.start].assume_init_read()
        };
        self.start = (self.start + 1) % N;
        self.len -= 1;
        Some(popped_val)
    }

    pub fn len(&self) -> usize {
        self.len
    }
}

impl<T, const N: usize> Drop for RingBuf<T, N> {
    fn drop(&mut self) {
        while self.pop().is_some() {}
    }
}