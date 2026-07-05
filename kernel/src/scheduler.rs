use core::arch::naked_asm;

use alloc::{collections::vec_deque::VecDeque, vec::Vec};
use spin::Mutex;
use x86_64::{instructions::interrupts::{self, without_interrupts}, registers::control::{Cr3, Cr3Flags}};

use crate::{gdt::set_tss_privilege_stack, process::{Pid, Process}, serial_println, utils::Registers};

#[derive(PartialEq, Eq, Clone, Copy)]
pub enum SchedulerState {
    Loading,
    Ready(ReadyMode),
    WaitPid(Pid),
    WaitKeyboard,
    Zombie(i32), // finished, but a process is still waiting on its pid
    Dead,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ReadyMode {
    Kernel,
    User,
}

pub struct Scheduler {
    pub processes : Vec<Process>,
    pub runnable_processes : VecDeque<Pid>,
    pub current_process : Option<Pid>,
    pub processes_waiting_keyboard : VecDeque<Pid>,
}

impl Scheduler {
    pub fn new_char(&mut self){
        if let Some(pid) = self.processes_waiting_keyboard.pop_front() {
            self.make_runnable_inner(pid, ReadyMode::Kernel);
        }
    }

    fn make_runnable_inner(&mut self, pid : Pid, ready_mode : ReadyMode){
        pid.get_process_mut(&mut self.processes).state = SchedulerState::Ready(ready_mode);
        // TODO : remove the contains ? is O(n)
   
        if self.current_process != Some(pid) && !self.runnable_processes.contains(&pid){
            self.runnable_processes.push_back(pid);
        }
    }

    pub fn make_runnable(&mut self, pid : Pid){
        self.make_runnable_inner(pid, ReadyMode::User);
    }

    pub fn make_runnable_kernel(&mut self, pid : Pid){
        self.make_runnable_inner(pid, ReadyMode::Kernel);
    }
}

pub static SCHEDULER : Mutex<Scheduler> = {
    let scheduler = Scheduler {
        processes: Vec::new(),
        runnable_processes: VecDeque::new(),
        processes_waiting_keyboard: VecDeque::new(),
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

    x86_64::instructions::interrupts::enable();
    
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

#[repr(C)]
#[derive(Default)]
pub struct KernelContext {
    pub rsp: u64, // offset 0x00
    pub rbp: u64, // offset 0x08
    pub rbx: u64, // offset 0x10
    pub r12: u64, // offset 0x18
    pub r13: u64, // offset 0x20
    pub r14: u64, // offset 0x28
    pub r15: u64, // offset 0x30
}

#[unsafe(naked)]
pub unsafe extern "C" fn switch_kernel_context(_old: *mut KernelContext, _new: *const KernelContext) {
    naked_asm!(
        "
        // rdi = old
        // rsi = new

        // save old context
        mov [rdi + 0x00], rsp
        mov [rdi + 0x08], rbp
        mov [rdi + 0x10], rbx
        mov [rdi + 0x18], r12
        mov [rdi + 0x20], r13
        mov [rdi + 0x28], r14
        mov [rdi + 0x30], r15

        // load new context
        mov rsp, [rsi + 0x00]
        mov rbp, [rsi + 0x08]
        mov rbx, [rsi + 0x10]
        mov r12, [rsi + 0x18]
        mov r13, [rsi + 0x20]
        mov r14, [rsi + 0x28]
        mov r15, [rsi + 0x30]

        ret
        "
    );
}

enum SwitchTarget {
    ContinueCurrent,
    SwitchUser(Registers),
    SwitchKernelToKernel {
        old_ctx : *mut KernelContext,
        new_ctx : *const KernelContext,
    },
    SwitchKernelToUser {
        old_ctx : *mut KernelContext,
        regs : Registers,
    }
}

// only call this with no interrupts
fn schedule_get_switch_target(scheduler : &mut Scheduler, current_pid : Pid, next_pid : Pid, regs : Option<&mut Registers>) -> SwitchTarget {
    serial_println!("scheduling to pid {}", next_pid.0.get());
    scheduler.current_process = Some(next_pid);

    let next_process = next_pid.get_process(&scheduler.processes);

    unsafe {
        Cr3::write(next_process.page_table_phys, Cr3Flags::empty());
    }

    set_tss_privilege_stack(next_process.kernel_stack_top);

    let current_state = current_pid.get_process(&scheduler.processes).state;

    let current_is_in_kernel = matches!(current_state, SchedulerState::WaitPid(_) | SchedulerState::WaitKeyboard);

    match next_process.state {
        SchedulerState::Ready(ReadyMode::User) => {
            let next_regs = next_process.saved_regs;
            match regs {
                Some(regs) => {
                    if current_is_in_kernel {
                        let old_ctx = &mut current_pid.get_process_mut(&mut scheduler.processes).kernel_context as *mut KernelContext;
                        SwitchTarget::SwitchKernelToUser { 
                            old_ctx, 
                            regs: next_regs, 
                        }
                    } else {
                        *regs = next_regs;
                        SwitchTarget::ContinueCurrent
                    }
                }
                None => {
                    SwitchTarget::SwitchUser(next_regs)
                }
            }
        }
        SchedulerState::Ready(ReadyMode::Kernel) => {
            // TODO : make it safer (the vec could be reallocated between creating the ptr and switching the context)
            let old_ctx = &mut current_pid.get_process_mut(&mut scheduler.processes).kernel_context as *mut KernelContext;
            let new_ctx = &next_pid.get_process_mut(&mut scheduler.processes).kernel_context as *const KernelContext;
            SwitchTarget::SwitchKernelToKernel { old_ctx, new_ctx }
        }
        _ => panic!("scheduled non-ready process {:?}", next_pid),
    }
}

fn schedule_switch(target : SwitchTarget){
    match target {
        SwitchTarget::ContinueCurrent => {}
        SwitchTarget::SwitchKernelToKernel { old_ctx, new_ctx } => unsafe {
            switch_kernel_context(old_ctx, new_ctx);
        }
        SwitchTarget::SwitchKernelToUser { old_ctx, regs } => unsafe {
            switch_kernel_to_user(old_ctx, &regs as *const Registers);
        }
        SwitchTarget::SwitchUser(regs) => unsafe {
            switch_user_from_kernel(&regs as *const Registers)
        },
    }
}

// from segfault, exit, etc
fn schedule_switch_never_return(target : SwitchTarget) -> ! {
    match target {
        SwitchTarget::SwitchUser(regs) => unsafe {
            switch_user_from_kernel(&regs as *const Registers)
        }
        SwitchTarget::SwitchKernelToKernel { old_ctx, new_ctx } => unsafe {
            switch_kernel_context(old_ctx, new_ctx);
            unreachable!("returned to dead process");
        }
        SwitchTarget::SwitchKernelToUser { old_ctx, regs } => unsafe {
            switch_kernel_to_user(old_ctx, &regs as *const Registers);
            unreachable!("returned to dead process");
        }
        SwitchTarget::ContinueCurrent => unreachable!(),
    }
}

// call only if one process is already running
pub fn schedule(regs : &mut Registers){
    let target = with_scheduler_no_int(|scheduler|{

        if let Some(current) = scheduler.current_process {
            serial_println!("current process before assert : {:?}", current);
            debug_assert!(!scheduler.runnable_processes.contains(&current), "current process is also in runnable queue");
        }

        serial_println!("runnable processes at start of schedule : {:?}", &scheduler.runnable_processes);

        let current_pid = scheduler.current_process.unwrap();
        current_pid.get_process_mut(&mut scheduler.processes).saved_regs = *regs;
        
        let current_is_ready = matches!(current_pid.get_process(&scheduler.processes).state, SchedulerState::Ready(_));

        let next = match scheduler.runnable_processes.pop_front(){
            Some(next) => {
                if current_is_ready && current_pid != next {
                    serial_println!("adding to ready process pid {}", current_pid.0.get());
                    scheduler.make_runnable(current_pid);
                }
                next
            },
            None => {
                if current_is_ready {
                    return SwitchTarget::ContinueCurrent; // continue with this process
                } else {
                    Process::IDLE_PROCESS_PID
                }
                
            },
        };
        
        schedule_get_switch_target(scheduler, current_pid, next, Some(regs))

    });
    schedule_switch(target);
}

// used by segfault
pub fn kill_current_and_schedule(exit_code : i32) -> ! {
    let target = with_scheduler_no_int(|scheduler|{
        let current_pid = scheduler.current_process.unwrap();
        serial_println!("current_process_pid : {:?}", current_pid);
        if current_pid == Process::INIT_PROCESS_PID {
            panic!("tried to exit init");
        }
        let current = current_pid.get_process_mut(&mut scheduler.processes);
        current.state = SchedulerState::Zombie(exit_code);
        
        let parent_pid = current_pid.get_process(&scheduler.processes).parent;
    

        if let Some(parent_pid) = parent_pid {
            let parent = parent_pid.get_process_mut(&mut scheduler.processes);
            if parent.state == SchedulerState::WaitPid(current_pid) {
                //parent.state = SchedulerState::Ready(ReadyMode::Kernel);
                scheduler.make_runnable_kernel(parent_pid);
            }
        }

        let next_pid = scheduler.runnable_processes.pop_front().unwrap_or(Process::IDLE_PROCESS_PID);
        schedule_get_switch_target(scheduler, current_pid, next_pid, None)
    });
    schedule_switch_never_return(target)
}

pub extern "C" fn idle_main(){
    loop {
        interrupts::enable_and_hlt();
        idle_schedule(); // schedule after an waiting for an interrupt because no process available
    }
}

// TODO : make this more elegant, like linux, not have to have to enter_user_from_kernel

fn idle_schedule(){
    let target = with_scheduler_no_int(|scheduler|{
        let next_pid = match scheduler.runnable_processes.pop_front() {
            Some(next_pid) => next_pid,
            None => return SwitchTarget::ContinueCurrent,
        };
            

        let current_pid = scheduler.current_process.unwrap();
        debug_assert_eq!(current_pid, Process::IDLE_PROCESS_PID);

        schedule_get_switch_target(scheduler, current_pid, next_pid, None)
    });
    schedule_switch(target);
}

#[unsafe(naked)]
pub unsafe extern "C" fn switch_user_from_kernel(_regs: *const Registers) -> ! {
    naked_asm!(
        "
        // rdi = *const Registers
        mov rax, rdi

        // Build iretq frame on the current kernel stack.
        // iretq will pop: rip, cs, rflags, rsp, ss
        push qword ptr [rax + 0x98]  // ss
        push qword ptr [rax + 0x90]  // user rsp
        push qword ptr [rax + 0x88]  // rflags
        push qword ptr [rax + 0x80]  // cs
        push qword ptr [rax + 0x78]  // rip

        // Restore general-purpose registers.
        // Keep rax as the pointer until the end.
        mov r15, [rax + 0x00]
        mov r14, [rax + 0x08]
        mov r13, [rax + 0x10]
        mov r12, [rax + 0x18]
        mov r11, [rax + 0x20]
        mov r10, [rax + 0x28]
        mov r9,  [rax + 0x30]
        mov r8,  [rax + 0x38]
        mov rbp, [rax + 0x40]
        mov rdi, [rax + 0x48]
        mov rsi, [rax + 0x50]
        mov rdx, [rax + 0x58]
        mov rcx, [rax + 0x60]
        mov rbx, [rax + 0x68]

        // Restore user rax last, because rax was our pointer.
        mov rax, [rax + 0x70]

        iretq
        "
    );
}

#[unsafe(naked)]
pub unsafe extern "C" fn switch_kernel_to_user(_old: *mut KernelContext, _regs: *const Registers,) {
    naked_asm!("
        // rdi = old ctx
        // rsi = regs

        mov [rdi + 0x00], rsp
        mov [rdi + 0x08], rbp
        mov [rdi + 0x10], rbx
        mov [rdi + 0x18], r12
        mov [rdi + 0x20], r13
        mov [rdi + 0x28], r14
        mov [rdi + 0x30], r15

        mov rax, rsi

        push qword ptr [rax + 0x98]
        push qword ptr [rax + 0x90]
        push qword ptr [rax + 0x88]
        push qword ptr [rax + 0x80]
        push qword ptr [rax + 0x78]

        mov r15, [rax + 0x00]
        mov r14, [rax + 0x08]
        mov r13, [rax + 0x10]
        mov r12, [rax + 0x18]
        mov r11, [rax + 0x20]
        mov r10, [rax + 0x28]
        mov r9,  [rax + 0x30]
        mov r8,  [rax + 0x38]
        mov rbp, [rax + 0x40]
        mov rdi, [rax + 0x48]
        mov rsi, [rax + 0x50]
        mov rdx, [rax + 0x58]
        mov rcx, [rax + 0x60]
        mov rbx, [rax + 0x68]
        mov rax, [rax + 0x70]

        iretq
        "
    );
}

pub fn with_scheduler_no_int<R>(f : impl FnOnce(&mut Scheduler) -> R) -> R {
    without_interrupts(||{
        let mut scheduler_lock = SCHEDULER.lock();
        f(&mut scheduler_lock)
    })
}