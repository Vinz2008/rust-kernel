use core::num::NonZero;

use alloc::vec::Vec;
use spin::Mutex;
use x86_64::{VirtAddr, structures::paging::{Page, PageTableFlags, PhysFrame, Size4KiB}};

use crate::allocator::{allocate_userspace_level_4_table, map_page_at};

#[derive(Clone, Copy, Debug)]
pub struct Pid(pub NonZero<usize>);

pub struct Process {
    pub pid : Pid,
    pub kernel_stack_top : VirtAddr,
    pub page_table_phys : PhysFrame,
}

const KERNEL_PROC_STACK_SIZE: u64 = 32 * 1024; // 8 pages
// TODO : add stack guard for kernel stack


const KERNEL_PROC_STACK_BASE: u64 = 0xffff_8000_0000_0000;

impl Process {
    pub fn new_process() -> Pid {
        let mut processes_lock = PROCESSES.lock();
        let new_process_idx = processes_lock.len();
        let new_process_pid = (new_process_idx + 1).try_into().unwrap();
        let new_process_pid = Pid(NonZero::new(new_process_pid).unwrap());
        let page_table_phys = allocate_userspace_level_4_table();

        let kernel_stack_start = VirtAddr::new(KERNEL_PROC_STACK_BASE);
        let kernel_stack_end = VirtAddr::new(KERNEL_PROC_STACK_BASE + KERNEL_PROC_STACK_SIZE - 1);
        let kernel_stack_start_page = Page::<Size4KiB>::containing_address(kernel_stack_start);
        let kernel_stack_end_page = Page::containing_address(kernel_stack_end);
        let page_range = Page::range_inclusive(kernel_stack_start_page, kernel_stack_end_page);
        for page in page_range {
            map_page_at(page.start_address(), PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::NO_EXECUTE);
        }

        processes_lock.push(Process { 
            pid: new_process_pid, 
            kernel_stack_top: VirtAddr::new(KERNEL_PROC_STACK_BASE), 
            page_table_phys 
        });
        new_process_pid
    }
}

pub static PROCESSES : Mutex<Vec<Process>> = Mutex::new(Vec::new());

pub static CURRENT_PROCESS : Mutex<Option<Pid>> = Mutex::new(None);