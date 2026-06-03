use core::fmt::{self, Display};

// how it is represented in memory in the stack, the last rbp value, which is the previous stack frame pointer, and the return address
#[derive(Clone, Copy)]
#[repr(C)]
pub struct StackFrame {
    previous_rbp : *const StackFrame,
    return_address : usize,
}

pub struct Backtrace {
    rbp : *const StackFrame,
}

pub struct BacktraceIter {
    current_rbp : *const StackFrame,
}


// TODO : embed a symbol table in the exe, so it can print backtrace symbol names (enable it with a feature flag to not have too much bloat by default ?)
impl Iterator for BacktraceIter {
    type Item = usize;

    fn next(&mut self) -> Option<Self::Item> {
        if self.current_rbp.is_null(){
            None
        } else {
            let current_frame = unsafe { *self.current_rbp };
            self.current_rbp = current_frame.previous_rbp;
            Some(current_frame.return_address)
        }
        
    }
}

impl Backtrace {
    pub fn new() -> Backtrace {
        let rbp;
        unsafe {
            core::arch::asm!("mov {}, rbp", out(reg) rbp);
        }
        Backtrace { 
            rbp,
        }
    }

    fn iter(&self) -> BacktraceIter {
        BacktraceIter { 
            current_rbp: self.rbp 
        }
    }
}

impl Display for Backtrace {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        for fun in self.iter() {
            writeln!(f, "0x{:x} ", fun)?;
        }
        fmt::Result::Ok(())
    }
}
