use alloc::{collections::vec_deque::VecDeque, vec::Vec};
use spin::Mutex;
use x86_64::registers::control::{Cr3, Cr3Flags};

use crate::{gdt::set_tss_privilege_stack, process::{Pid, Process}, serial_println, utils::Registers};

#[derive(PartialEq, Eq)]
pub enum SchedulerState {
    Loading,
    Ready,
    WaitPid(Pid),
    WaitKeyboard,
    Zombie(i32), // finished, but a process is still waiting on its pid
    Dead,
}

pub struct Scheduler {
    pub processes : Vec<Process>,
    pub runnable_processes : VecDeque<Pid>,
    pub current_process : Option<Pid>,
}

pub static SCHEDULER : Mutex<Scheduler> = {
    let scheduler = Scheduler {
        processes: Vec::new(),
        runnable_processes: VecDeque::new(),
        current_process: None,
    };
    Mutex::new(scheduler)
};

pub fn start_first_process(pid : Pid) -> ! {
    serial_println!("start first process");
    
    let (page_table_phys, kernel_stack_top, regs) = {
        let mut scheduler_lock = SCHEDULER.lock();
        scheduler_lock.current_process = Some(pid);

        let process = pid.get_process(&scheduler_lock.processes);
        set_tss_privilege_stack(process.kernel_stack_top);
        (process.page_table_phys, process.kernel_stack_top, process.saved_regs)
    };
    
    unsafe {
        Cr3::write(page_table_phys, Cr3Flags::empty());
        core::arch::asm!(
            "mov rsp, {kernel_rsp}",
            "push {ss}",
            "push {rsp}",
            "push {rflags}",
            "push {cs}",
            "push {rip}",
            "iretq",
            kernel_rsp = in(reg) kernel_stack_top.as_u64(),
            ss = in(reg) regs.ss,
            rsp = in(reg) regs.rsp,
            rflags = in(reg) regs.rflags,
            cs = in(reg) regs.cs,
            rip = in(reg) regs.rip,
            options(noreturn)
        )
    }
}

// call only if one process is already running
pub fn schedule(regs : &mut Registers){
    let mut scheduler_lock = SCHEDULER.lock();

    let current_pid = scheduler_lock.current_process.unwrap();
    current_pid.get_process_mut(&mut scheduler_lock.processes).saved_regs = *regs;
    
    if current_pid.get_process(&scheduler_lock.processes).state == SchedulerState::Ready {
        scheduler_lock.runnable_processes.push_back(current_pid);
    }

    let next = match scheduler_lock.runnable_processes.pop_front(){
        Some(next) => next,
        None => return, // continue with this process
    };

    scheduler_lock.current_process = Some(next);


    unsafe {
        Cr3::write(next.get_process(&scheduler_lock.processes).page_table_phys, Cr3Flags::empty());
    }
    set_tss_privilege_stack(next.get_process(&scheduler_lock.processes).kernel_stack_top);
    *regs = next.get_process(&scheduler_lock.processes).saved_regs;
}