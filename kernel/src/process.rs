use core::num::NonZero;

use alloc::vec::Vec;
use spin::Mutex;
use x86_64::{PhysAddr, VirtAddr, structures::paging::{Page, PageTableFlags, PhysFrame, Size4KiB}};

use crate::{allocator::{allocate_userspace_level_4_table, map_page_at_in, map_page_phys_at_in}};

#[derive(Clone, Copy, Debug)]
pub struct Pid(pub NonZero<usize>);

impl Pid {
    pub fn get_process(self, processes : &[Process]) -> &Process {
        processes.get(self.0.get()-1).unwrap()
    }
}

pub struct Process {
    pub pid : Pid,
    pub kernel_stack_top : VirtAddr,
    pub page_table_phys : PhysFrame,
}

const KERNEL_PROC_STACK_BASE: u64 = 0xffff_8000_0000_0000;

const KERNEL_PROC_STACK_SIZE: u64 = 32 * 1024; // 8 pages
// TODO : add stack guard for kernel stack

const KERNEL_PROC_STACK_GUARD_SIZE: u64 = 4096; // 1 page

const SLOT_SIZE : u64 = KERNEL_PROC_STACK_GUARD_SIZE + KERNEL_PROC_STACK_SIZE;

pub static PROCESSES : Mutex<Vec<Process>> = Mutex::new(Vec::new());

pub static CURRENT_PROCESS : Mutex<Option<Pid>> = Mutex::new(None);

impl Process {
    pub fn new_process() -> Pid {
        let mut processes_lock = PROCESSES.lock();
        let new_process_idx = processes_lock.len();
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

        map_page_phys_at_in(page_table_phys.start_address(), PhysFrame::containing_address(PhysAddr::new(0xb8000)), VirtAddr::new(0xb8000), PageTableFlags::PRESENT | PageTableFlags::WRITABLE);

        processes_lock.push(Process { 
            pid: new_process_pid, 
            kernel_stack_top: VirtAddr::new(stack_end), 
            page_table_phys 
        });
        new_process_pid
    }
}