use core::num::NonZero;

use alloc::{collections::vec_deque::VecDeque, vec::Vec};
use x86_64::{PhysAddr, VirtAddr, registers::rflags::RFlags, structures::paging::{Page, PageTableFlags, PhysFrame, Size4KiB}};

use crate::{allocator::{allocate_userspace_level_4_table, map_page_at_in, map_page_phys_at_in}, gdt::GDT, scheduler::{SCHEDULER, SchedulerState}, userspace::USER_STACK_TOP, utils::Registers};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pid(pub NonZero<usize>);

impl Pid {
    pub unsafe fn new_unchecked(pid_nb : usize) -> Option<Pid> {
        Some(Pid(NonZero::new(pid_nb)?))
    }

    pub fn get_process(self, processes : &[Process]) -> &Process {
        processes.get(self.0.get()-1).unwrap()
    }
    
    pub fn get_process_mut(self, processes : &mut [Process]) -> &mut Process {
        processes.get_mut(self.0.get()-1).unwrap()
    }
}

pub struct Process {
    pub pid : Pid,
    pub parent : Option<Pid>,
    pub children: Vec<Pid>,
    pub kernel_stack_top : VirtAddr,
    pub page_table_phys : PhysFrame,
    pub state : SchedulerState,
    pub saved_regs : Registers,
}

const KERNEL_PROC_STACK_BASE: u64 = 0xffff_8000_0000_0000;

const KERNEL_PROC_STACK_SIZE: u64 = 32 * 1024; // 8 pages
// TODO : add stack guard for kernel stack

const KERNEL_PROC_STACK_GUARD_SIZE: u64 = 4096; // 1 page

const SLOT_SIZE : u64 = KERNEL_PROC_STACK_GUARD_SIZE + KERNEL_PROC_STACK_SIZE;


impl Process {
    pub fn empty_process() -> Pid {
        // TODO : use the dead process pid
        let mut scheduler_lock = SCHEDULER.lock();
        let new_process_idx = scheduler_lock.processes.len();
        let new_process_pid = new_process_idx + 1;
        let new_process_pid = Pid(NonZero::new(new_process_pid).unwrap());
        let page_table_phys = allocate_userspace_level_4_table();
        
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
            map_page_at_in(page_table_phys.start_address(), page.start_address(), PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::NO_EXECUTE);
        }

        let current_process = scheduler_lock.current_process;

        map_page_phys_at_in(page_table_phys.start_address(), PhysFrame::containing_address(PhysAddr::new(0xb8000)), VirtAddr::new(0xb8000), PageTableFlags::PRESENT | PageTableFlags::WRITABLE);
        scheduler_lock.processes.push(Process { 
            pid: new_process_pid, 
            children: Vec::new(),
            parent: current_process,
            kernel_stack_top: VirtAddr::new(stack_end), 
            page_table_phys,
            state: SchedulerState::Loading,
            saved_regs: Registers::default(),
        });

        new_process_pid
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
            ..Registers::default()
        };

        self.saved_regs = saved_regs;
        self.state = SchedulerState::Ready;
    }
}