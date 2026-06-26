use core::num::NonZero;

use alloc::vec::Vec;
use spin::Mutex;

#[derive(Clone, Copy, Debug)]
pub struct Pid(pub NonZero<u32>);

pub struct Process {
    pub pid : Pid,
}

impl Process {
    pub fn new_process() -> Pid {
        let new_process_pid = (PROCESSES.lock().len() + 1).try_into().unwrap();
        Pid(NonZero::new(new_process_pid).unwrap())
    }
}

static PROCESSES : Mutex<Vec<Process>> = Mutex::new(Vec::new());

pub static CURRENT_PROCESS : Mutex<Option<Pid>> = Mutex::new(None);