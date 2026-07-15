use core::{num::NonZero, ptr};

use alloc::{string::String, vec::Vec};
use shared_consts::{USER_HEAP_SIZE, USER_HEAP_START};
use x86_64::{PhysAddr, VirtAddr, instructions::interrupts, registers::{control::Cr3, rflags::RFlags}, structures::paging::{Page, PageTableFlags, PhysFrame, Size4KiB}};

use crate::{allocator::{allocate_userspace_level_4_table, map_page_at_in, map_page_phys_at_in}, gdt::GDT, paging::{PHYSICAL_MEMORY_OFFSET, translate_addr_in}, scheduler::{KernelContext, ReadyMode, SCHEDULER, SchedulerState, idle_main, with_scheduler_no_int}, userspace::USER_STACK_TOP, utils::Registers};

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
    pub cwd_path : String,
    pub fd_list : Vec<Option<OpenedFile>>,
    pub heap_start : VirtAddr,
    pub heap_break : VirtAddr,
    pub heap_max : VirtAddr,
}

// TODO : add in the first file desciptors stdout, stdin and stderr

pub struct OpenedFile {
    pub path : String, // TODO : have stable id like InodeId
    pub offset : usize,
    readable : bool,
    writable : bool,
}

impl OpenedFile {
    pub fn new(path : String, is_readable : bool, is_writable : bool) -> OpenedFile {
        OpenedFile { 
            path, 
            offset: 0, 
            readable: is_readable, 
            writable: is_writable, 
        }
    }
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

    pub fn empty_process(cwd_path : String) -> Pid {
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
                cwd_path: cwd_path,
                fd_list: Vec::new(),
                heap_start: VirtAddr::new(USER_HEAP_START as u64),
                heap_break: VirtAddr::new(USER_HEAP_START as u64),
                heap_max: VirtAddr::new((USER_HEAP_START + USER_HEAP_SIZE) as u64),
            });

            new_process_pid
        })
        
    }

    pub const IDLE_PROCESS_PID: Pid = unsafe { Pid::new_unchecked(1).unwrap() };

    pub const INIT_PROCESS_PID : Pid = unsafe { Pid::new_unchecked(2).unwrap() };

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
            cwd_path: String::new(),
            fd_list: Vec::new(),
            heap_start: VirtAddr::new(0),
            heap_break: VirtAddr::new(0),
            heap_max: VirtAddr::new(0),
        });
        
    }

    fn write_to_process_stack_bytes(page_table : PhysFrame<Size4KiB>, stack_ptr : &mut u64, bytes : &[u8]){
        unsafe {
            *stack_ptr -= bytes.len() as u64;
            let phys_ptr = translate_addr_in(page_table, VirtAddr::new(*stack_ptr)).unwrap(); // TODO : replace unwrap with real error handling ?
            let real_ptr_addr = PHYSICAL_MEMORY_OFFSET.get().unwrap().as_u64() + phys_ptr.as_u64();
            let real_ptr = real_ptr_addr as *mut u8;

            ptr::copy_nonoverlapping(bytes.as_ptr(), real_ptr, bytes.len());
        }
    }

    fn write_to_process_stack_u64(page_table : PhysFrame<Size4KiB>, stack_ptr : &mut u64, nb : u64){
        let bytes = &nb.to_ne_bytes();
        Self::write_to_process_stack_bytes(page_table, stack_ptr, bytes);
    }

    fn init_process_stack(&self, stack_top : usize, page_table : PhysFrame<Size4KiB>, args : &[&str]) -> usize {
        let mut current_stack_ptr = stack_top as u64;
        let mut args_ptr = Vec::with_capacity(args.len());
        for arg in args.iter() {
            Self::write_to_process_stack_bytes(page_table, &mut current_stack_ptr, arg.as_bytes());
            args_ptr.push((current_stack_ptr, arg.len()));
        }

        // TODO : env vars ?

        current_stack_ptr &= !0xf;

        Self::write_to_process_stack_u64(page_table, &mut current_stack_ptr, 0);

        for &(arg_ptr, arg_len) in args_ptr.iter().rev() {
            Self::write_to_process_stack_u64(page_table, &mut current_stack_ptr, arg_ptr as u64);
            Self::write_to_process_stack_u64(page_table, &mut current_stack_ptr, arg_len as u64);
        }


        Self::write_to_process_stack_u64(page_table, &mut current_stack_ptr, args.len() as u64);

        debug_assert_eq!(current_stack_ptr % 16, 0);
        current_stack_ptr as usize
    }

    pub fn init_process(&mut self, entrypoint : usize, args : &[&str]){

        let stack_segment = GDT.1.user_data_selector.0 as u64 | 3;
        let code_segment = GDT.1.user_code_selector.0 as u64 | 3;
        let rflags = RFlags::INTERRUPT_FLAG | RFlags::from_bits_truncate(0x2); // 0x2 is for the reserved bit that always need to be 1

        let stack_top = USER_STACK_TOP & !0xf; // 16 bytes align the stack, for syscall and iret
        let rsp = self.init_process_stack(stack_top, self.page_table_phys, args);
        //let rsp = stack_top;

        let saved_regs = Registers {
            rip: entrypoint as u64,
            rsp: rsp as u64,
            cs: code_segment,
            ss: stack_segment,
            rflags: rflags.bits(),
            ..Default::default()
        };

        self.saved_regs = saved_regs;
        self.state = SchedulerState::Ready(ReadyMode::User);
    }
}