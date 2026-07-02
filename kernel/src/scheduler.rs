use core::arch::naked_asm;

use alloc::{collections::vec_deque::VecDeque, vec::Vec};
use spin::Mutex;
use x86_64::{instructions::interrupts, registers::control::{Cr3, Cr3Flags}};

use crate::{gdt::set_tss_privilege_stack, process::{Pid, Process, ProcessKind}, serial_println, utils::Registers};

#[derive(PartialEq, Eq)]
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
pub unsafe extern "C" fn switch_context(_old: *mut KernelContext, _new: *const KernelContext) {
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

// call only if one process is already running
pub fn schedule(regs : &mut Registers){
    let mut scheduler_lock = SCHEDULER.lock();

    if let Some(current) = scheduler_lock.current_process {
        serial_println!("current process before assert : {:?}", current);
        assert!(!scheduler_lock.runnable_processes.contains(&current), "current process is also in runnable queue");
    }

    serial_println!("runnable processes at start of schedule : {:?}", &scheduler_lock.runnable_processes);

    let current_pid = scheduler_lock.current_process.unwrap();
    current_pid.get_process_mut(&mut scheduler_lock.processes).saved_regs = *regs;
    
    let current_is_ready = matches!(current_pid.get_process(&scheduler_lock.processes).state, SchedulerState::Ready(_));

    let next = match scheduler_lock.runnable_processes.pop_front(){
        Some(next) => {
            if current_is_ready && current_pid != next {
                serial_println!("adding to ready process pid {}", current_pid.0.get());
                scheduler_lock.make_runnable(current_pid);
            }
            next
        },
        None => {
            if current_is_ready {
                return; // continue with this process
            } else {
                Process::IDLE_PROCESS_PID
            }
            
        },
    };
    serial_println!("scheduling to pid {}", next.0.get());

    scheduler_lock.current_process = Some(next);


    unsafe {
        Cr3::write(next.get_process(&scheduler_lock.processes).page_table_phys, Cr3Flags::empty());
    }
    set_tss_privilege_stack(next.get_process(&scheduler_lock.processes).kernel_stack_top);
    
    match next.get_process(&scheduler_lock.processes).state {
        SchedulerState::Ready(ReadyMode::User) => {
            *regs = next.get_process(&scheduler_lock.processes).saved_regs;
        },
        SchedulerState::Ready(ReadyMode::Kernel) => {
            // TODO : make it safer (the vec could be reallocated between creating the ptr and switching the context)
            let old_ctx = &mut current_pid.get_process_mut(&mut scheduler_lock.processes).kernel_context as *mut KernelContext;
            let new_ctx = &next.get_process_mut(&mut scheduler_lock.processes).kernel_context as *const KernelContext;
            drop(scheduler_lock);
            unsafe {
                switch_context(old_ctx, new_ctx);
            }
        },
        _ => panic!("scheduled non-ready process {:?}", next),
    }
    
}

pub extern "C" fn idle_main(){
    loop {
        interrupts::enable_and_hlt();
        idle_schedule(); // schedule after an waiting for an interrupt because no process available
    }
}

// TODO : make this more elegant, like linux, not have to have to enter_user_from_kernel

fn idle_schedule(){
    let mut scheduler_lock = SCHEDULER.lock();

    let next_pid = match scheduler_lock.runnable_processes.pop_front() {
        Some(next_pid) => next_pid,
        None => return,
    };
        

    let current_pid = scheduler_lock.current_process.unwrap();

    scheduler_lock.current_process = Some(next_pid);
    
    unsafe {
        Cr3::write(next_pid.get_process(&scheduler_lock.processes).page_table_phys, Cr3Flags::empty());
    }
    set_tss_privilege_stack(next_pid.get_process(&scheduler_lock.processes).kernel_stack_top);

    match next_pid.get_process(&scheduler_lock.processes).state {
        SchedulerState::Ready(ReadyMode::User) => {
            let next_regs = next_pid.get_process(&scheduler_lock.processes).saved_regs;
            drop(scheduler_lock);
            unsafe {
                enter_user_from_kernel(&next_regs as *const Registers)
            }
        }
        SchedulerState::Ready(ReadyMode::Kernel) => {
            let old_ctx = &mut current_pid.get_process_mut(&mut scheduler_lock.processes).kernel_context as *mut KernelContext;
            let new_ctx = &next_pid.get_process(&scheduler_lock.processes).kernel_context as *const KernelContext;
            drop(scheduler_lock);
            unsafe  {
                switch_context(old_ctx, new_ctx);
            }
        }
        _ => panic!("idle scheduled non-ready process {:?}", next_pid),
    }

    
}

#[unsafe(naked)]
pub unsafe extern "C" fn enter_user_from_kernel(_regs: *const Registers) -> ! {
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