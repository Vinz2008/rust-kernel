use core::{fmt::{self, Display}, ops::Range};

use crate::{process::{KERNEL_PROC_STACK_BASE, KERNEL_PROC_STACK_GUARD_SIZE, KERNEL_PROC_STACK_SIZE, KERNEL_PROC_STACK_SLOT_SIZE}, symbols};

// how it is represented in memory in the stack, the last rbp value, which is the previous stack frame pointer, and the return address
#[derive(Clone, Copy)]
#[repr(C)]
pub struct StackFrame {
    previous_rbp : *const StackFrame,
    return_address : usize,
}

pub struct Backtrace {
    rbp : *const StackFrame,
    stack_bounds : Option<Range<usize>>,
}

pub struct BacktraceIter {
    current_rbp : *const StackFrame,
    stack_bounds : Option<Range<usize>>,
}

fn is_rbp_invalid(backtrace_iter : &BacktraceIter) -> bool {
    let address = backtrace_iter.current_rbp as usize;
    if backtrace_iter.current_rbp.is_null() || !address.is_multiple_of(core::mem::align_of::<StackFrame>()){
        return true;
    }

    if let Some(stack_bounds) = backtrace_iter.stack_bounds.clone() {
        return !stack_bounds.contains(&address);
    }

    false
}

impl Iterator for BacktraceIter {
    type Item = usize;

    fn next(&mut self) -> Option<Self::Item> {
        if is_rbp_invalid(self) {
            None
        } else {
            let current_frame = unsafe { *self.current_rbp };
            self.current_rbp = current_frame.previous_rbp;
            Some(current_frame.return_address)
        }
        
    }
}

fn current_rsp() -> usize {
    let rsp: usize;

    unsafe {
        core::arch::asm!(
            "mov {}, rsp",
            out(reg) rsp,
            options(nomem, nostack, preserves_flags),
        );
    }

    rsp
}

fn current_process_stack_bounds() -> Option<Range<usize>> {
    let rsp = current_rsp();
    let base = KERNEL_PROC_STACK_BASE as usize;
    if rsp < base {
        return None;
    }

    let slot_idx = (rsp - base) / KERNEL_PROC_STACK_SLOT_SIZE as usize;
    let slot_start = base + slot_idx * KERNEL_PROC_STACK_SLOT_SIZE as usize;
    let stack_start = slot_start + KERNEL_PROC_STACK_GUARD_SIZE as usize;
    let stack_end = stack_start + KERNEL_PROC_STACK_SIZE as usize;

    if rsp < stack_start || rsp > stack_end {
        return None;
    }

    Some(stack_start..stack_end)
}

impl Backtrace {
    pub fn new() -> Backtrace {
        let rbp;
        unsafe {
            core::arch::asm!("mov {}, rbp", out(reg) rbp);
        }
        let stack_bounds = current_process_stack_bounds();
        Backtrace { 
            rbp,
            stack_bounds,
        }
    }

    fn iter(&self) -> BacktraceIter {
        BacktraceIter { 
            current_rbp: self.rbp,
            stack_bounds: self.stack_bounds.clone(),
        }
    }
}

impl Display for Backtrace {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        for fun in self.iter() {
            if let Some((name, offset)) = symbols::lookup_symbol(fun){
                writeln!(f, "0x{:x}  {}+0x{:x}", fun, name, offset)?;
            } else {
                writeln!(f, "0x{:x} ", fun)?;
            }
            
        }
        fmt::Result::Ok(())
    }
}
