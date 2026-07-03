use core::num::NonZero;

use alloc::vec::Vec;
use x86_64::{PhysAddr, VirtAddr, instructions::interrupts, registers::{control::Cr3, rflags::RFlags}, structures::paging::{Page, PageTableFlags, PhysFrame, Size4KiB}};

use crate::{allocator::{allocate_userspace_level_4_table, map_page_at_in, map_page_phys_at_in}, gdt::GDT, scheduler::{KernelContext, ReadyMode, SCHEDULER, SchedulerState, idle_main, with_scheduler_no_int}, userspace::USER_STACK_TOP, utils::Registers};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pid(pub NonZero<usize>);

impl Pid {
    pub const unsafe fn new_unchecked(pid_nb : usize) -> Option<Pid> {
        match NonZero::new(pid_nb) {
            Some(pid) => Some(Pid(pid)),
            None => None,
        }
    }

    pub fn get_process(self, processes : &[Process]) -> &Process {
        processes.get(self.0.get()-1).unwrap()
    }
    
    pub fn get_process_mut(self, processes : &mut [Process]) -> &mut Process {
        processes.get_mut(self.0.get()-1).unwrap()
    }
}

#[derive(PartialEq, Eq)]
pub enum ProcessKind {
    User,
    Kernel,
}

pub struct Process {
    pub pid : Pid,
    pub parent : Option<Pid>,
    pub children: Vec<Pid>,
    pub kernel_stack_top : VirtAddr,
    pub page_table_phys : PhysFrame,
    pub state : SchedulerState,
    pub process_kind : ProcessKind,
    pub saved_regs : Registers,
    pub kernel_context : KernelContext,
}

const KERNEL_PROC_STACK_BASE: u64 = 0xffff_8000_0000_0000;

const KERNEL_PROC_STACK_SIZE: u64 = 32 * 1024; // 8 pages
// TODO : add stack guard for kernel stack

const KERNEL_PROC_STACK_GUARD_SIZE: u64 = 4096; // 1 page

const SLOT_SIZE : u64 = KERNEL_PROC_STACK_GUARD_SIZE + KERNEL_PROC_STACK_SIZE;

impl Process {

    fn allocate_kernel_stack(new_process_idx : usize, page_table_phys : PhysFrame) -> u64 {
        // stack starts at the end
        let stack_slot_start = KERNEL_PROC_STACK_BASE + new_process_idx as u64 * SLOT_SIZE;
        let stack_start = stack_slot_start + KERNEL_PROC_STACK_GUARD_SIZE;
        let stack_end = stack_start + KERNEL_PROC_STACK_SIZE;
        let virt_stack_start = VirtAddr::new(stack_start);
        let virt_stack_end = VirtAddr::new(stack_end-1);
        let kernel_stack_start_page = Page::<Size4KiB>::containing_address(virt_stack_start);
        let kernel_stack_end_page = Page::containing_address(virt_stack_end);
        let page_range = Page::range_inclusive(kernel_stack_start_page, kernel_stack_end_page);
        for page in page_range {
            map_page_at_in(page_table_phys.start_address(), page.start_address(), PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::NO_EXECUTE).unwrap(); // TODO : should I realy unwrap ?
        }
        stack_end
    }

    pub fn empty_process() -> Pid {
        // TODO : use the dead process pid
        with_scheduler_no_int(|scheduler|{
            let new_process_idx = scheduler.processes.len();
            let new_process_pid = new_process_idx + 1;
            let new_process_pid = Pid(NonZero::new(new_process_pid).unwrap());
            let page_table_phys = allocate_userspace_level_4_table();
            
            let stack_end = Process::allocate_kernel_stack(new_process_idx, page_table_phys);

            let parent_pid = scheduler.current_process;
            if let Some(parent_pid) = parent_pid {
                parent_pid.get_process_mut(&mut scheduler.processes).children.push(new_process_pid);
            }

            map_page_phys_at_in(page_table_phys.start_address(), PhysFrame::containing_address(PhysAddr::new(0xb8000)), VirtAddr::new(0xb8000), PageTableFlags::PRESENT | PageTableFlags::WRITABLE).unwrap(); // TODO : should I realy unwrap ?
            scheduler.processes.push(Process { 
                pid: new_process_pid, 
                children: Vec::new(),
                parent: parent_pid,
                kernel_stack_top: VirtAddr::new(stack_end), 
                page_table_phys,
                state: SchedulerState::Loading,
                process_kind: ProcessKind::User,
                saved_regs: Registers::default(),
                kernel_context: KernelContext::default(),
            });

            new_process_pid
        })
        
    }

    pub const IDLE_PROCESS_PID: Pid = unsafe { Pid::new_unchecked(1).unwrap() };

    pub fn init_idle_process(){
        debug_assert!(!interrupts::are_enabled());
        let mut scheduler_lock = SCHEDULER.lock();
        let new_process_idx = scheduler_lock.processes.len();
        let new_process_pid = new_process_idx + 1;
        debug_assert_eq!(new_process_pid, Process::IDLE_PROCESS_PID.0.get());
        let new_process_pid = Pid(NonZero::new(new_process_pid).unwrap());

        let (kernel_page_table, _) = Cr3::read();

        let kernel_stack_end = Process::allocate_kernel_stack(new_process_idx, kernel_page_table);

        let entrypoint = idle_main as *const () as usize;


        let saved_regs = Registers::default();

        let rsp = kernel_stack_end - 8;
        let ret_adr = rsp as *mut usize;
        unsafe {
            *ret_adr = entrypoint;
        }
            

        let kernel_context = KernelContext {
            rsp,
            ..Default::default()
        };

        scheduler_lock.processes.push(Process { 
            pid: new_process_pid, 
            children: Vec::new(),
            parent: None,
            kernel_stack_top: VirtAddr::new(kernel_stack_end), 
            page_table_phys: kernel_page_table,
            state: SchedulerState::Ready(ReadyMode::Kernel),
            process_kind: ProcessKind::Kernel,
            saved_regs,
            kernel_context,
        });
        
    }

    pub fn init_process(&mut self, entrypoint : usize){
        let stack_segment = GDT.1.user_data_selector.0 as u64 | 3;
        let code_segment = GDT.1.user_code_selector.0 as u64 | 3;
        let rflags = RFlags::INTERRUPT_FLAG | RFlags::from_bits_truncate(0x2); // 0x2 is for the reserved bit that always need to be 1

        let stack_addr = USER_STACK_TOP & !0xf; // 16 bytes align the stack, for syscall and iret

        let saved_regs = Registers {
            rip: entrypoint as u64,
            rsp: stack_addr as u64,
            cs: code_segment,
            ss: stack_segment,
            rflags: rflags.bits(),
            ..Default::default()
        };

        self.saved_regs = saved_regs;
        self.state = SchedulerState::Ready(ReadyMode::User);
    }
}