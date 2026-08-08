use core::{num::NonZero, ptr};

use alloc::{boxed::Box, string::String, sync::Arc, vec::Vec};
use shared_consts::{Fd, USER_HEAP_SIZE, USER_HEAP_START};
use spin::Mutex;
use x86_64::{PhysAddr, VirtAddr, instructions::interrupts, registers::{control::Cr3, rflags::RFlags}, structures::paging::{Page, PageTableFlags, PhysFrame, Size4KiB}};

use crate::{allocator::{allocate_userspace_level_4_table, deallocate_userspace_page_tables, deallocate_virtual_page}, fs::{FileError, Inode, add_inode, get_inode}, gdt::GDT, paging::{PHYSICAL_MEMORY_OFFSET, map_page_at_in, map_page_phys_at_in, translate_addr_in}, scheduler::{KernelContext, ReadyMode, SCHEDULER, SchedulerState, idle_main, with_scheduler_no_int}, serial_println, sse::{DEFAULT_FXSTATE, FxState}, userspace::{USER_STACK_SIZE, USER_STACK_TOP}, utils::Registers};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pid(pub NonZero<usize>);

impl Pid {
    pub const unsafe fn new_unchecked(pid_nb : usize) -> Option<Pid> {
        match NonZero::new(pid_nb) {
            Some(pid) => Some(Pid(pid)),
            None => None,
        }
    }

    pub fn get_process(self, processes : &[Box<Process>]) -> &Process {
        processes.get(self.0.get()-1).unwrap()
    }
    
    pub fn get_process_mut(self, processes : &mut [Box<Process>]) -> &mut Process {
        processes.get_mut(self.0.get()-1).unwrap()
    }
}

#[derive(PartialEq, Eq)]
pub enum ProcessKind {
    User,
    Kernel,
}

#[derive(Clone, Copy)]
pub struct ElfMemRegion {
    pub start: VirtAddr,
    pub end: VirtAddr, // exclusive
}

pub struct Process {
    pub pid : Pid,
    pub parent : Option<Pid>,
    pub children: Vec<Pid>,
    pub kernel_stack_top : VirtAddr,
    pub page_table_phys : PhysFrame,
    pub state : SchedulerState,
    pub process_kind : ProcessKind, // TODO : remove this ?
    pub saved_regs : Registers,
    pub kernel_context : KernelContext,
    pub fxstate : FxState,
    pub cwd_path : String,
    pub fd_list : Vec<Option<Arc<OpenedFile>>>, // TODO : replace this with a SmallVec ?
    pub free_fd_nb : usize,
    pub heap_start : VirtAddr,
    pub heap_break : VirtAddr,
    pub heap_max : VirtAddr,
    pub elf_regions : Vec<ElfMemRegion>,
}

// TODO : add in the first file descriptors stdin and stderr

pub struct OpenedFile {
    pub inode : Arc<Inode>,
    pub offset : Mutex<usize>,
    pub readable : bool,
    pub writable : bool,
}

impl OpenedFile {
    pub fn new(path : &str, is_readable : bool, is_writable : bool, create_file : bool) -> Result<Arc<OpenedFile>, FileError> {
        let inode = match get_inode(path){
            Ok(i) => i,
            Err(FileError::FileNotFound { path: err_path }) => {
                if create_file {
                    let inode = Inode::new_mem_file();
                    add_inode(path, inode.clone())?;
                    inode
                } else {
                    return Err(FileError::FileNotFound { path: err_path });
                }
            },
            Err(e) => return Err(e),
        };
        let opened_file = OpenedFile { 
            inode, 
            offset: Mutex::new(0), 
            readable: is_readable, 
            writable: is_writable, 
        };
        Ok(Arc::new(opened_file))
    }
}

pub const KERNEL_PROC_STACK_BASE: u64 = 0xffff_8000_0000_0000;

pub const KERNEL_PROC_STACK_SIZE: u64 = 32 * 1024; // 8 pages

pub const KERNEL_PROC_STACK_GUARD_SIZE: u64 = 4096; // 1 page

pub const KERNEL_PROC_STACK_SLOT_SIZE : u64 = KERNEL_PROC_STACK_GUARD_SIZE + KERNEL_PROC_STACK_SIZE;

fn allocate_kernel_stack(new_process_idx : usize, page_table_phys : PhysFrame) -> u64 {
    // stack starts at the end
    let stack_slot_start = KERNEL_PROC_STACK_BASE + new_process_idx as u64 * KERNEL_PROC_STACK_SLOT_SIZE;
    let stack_start = stack_slot_start + KERNEL_PROC_STACK_GUARD_SIZE;
    let stack_end = stack_start + KERNEL_PROC_STACK_SIZE;
    let virt_stack_start = VirtAddr::new(stack_start);
    let virt_stack_end = VirtAddr::new(stack_end-1);
    let kernel_stack_start_page = Page::<Size4KiB>::containing_address(virt_stack_start);
    let kernel_stack_end_page = Page::containing_address(virt_stack_end);
    let page_range = Page::range_inclusive(kernel_stack_start_page, kernel_stack_end_page);
    for page in page_range {
        map_page_at_in(page_table_phys.start_address(), page.start_address(), PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::NO_EXECUTE).unwrap().flush(); // TODO : should I really unwrap ?
    }
    stack_end
}

fn current_rsp() -> VirtAddr {
    let rsp: u64;

    unsafe {
        core::arch::asm!(
            "mov {}, rsp",
            out(reg) rsp,
            options(nomem, nostack, preserves_flags),
        );
    }

    VirtAddr::new(rsp)
}

// TODO : remove debug code ?
fn deallocate_kernel_stack(stack_top : VirtAddr, page_table_frame : PhysFrame){
    let stack_start = stack_top - KERNEL_PROC_STACK_SIZE;

    let start_page =
        Page::<Size4KiB>::from_start_address(stack_start)
            .expect("stack start is not page aligned");

    let end_page =
        Page::<Size4KiB>::from_start_address(stack_top)
            .expect("stack top is not page aligned");

    let rsp = current_rsp();
    let current_stack_page =
        Page::<Size4KiB>::containing_address(rsp);

    serial_println!(
        "free stack {:#x}..{:#x}, rsp={:#x}, active page={:#x}",
        stack_start.as_u64(),
        stack_top.as_u64(),
        rsp.as_u64(),
        current_stack_page.start_address().as_u64(),
    );

    for page in Page::range(start_page, end_page) {
        serial_println!(
            "unmapping stack page {:#x}",
            page.start_address().as_u64()
        );

        assert_ne!(
            page,
            current_stack_page,
            "attempted to unmap page containing current RSP"
        );

        deallocate_virtual_page(page_table_frame, page);

        serial_println!(
            "unmapped stack page {:#x}",
            page.start_address().as_u64()
        );
    }
}

fn deallocate_user_heap(heap_start : VirtAddr, heap_break : VirtAddr, page_table_frame : PhysFrame){
    let start_page = Page::<Size4KiB>::containing_address(VirtAddr::new(heap_start.as_u64()));
    let end_page = Page::<Size4KiB>::containing_address(VirtAddr::new(heap_break.as_u64()-1));
    let page_range= Page::range_inclusive(start_page, end_page);
    for page in page_range {
        deallocate_virtual_page(page_table_frame, page);
    }
}

fn deallocate_user_stack(page_table_frame : PhysFrame){
    let user_stack_end = USER_STACK_TOP;
    let user_stack_start = user_stack_end - USER_STACK_SIZE;
    let start_page = Page::<Size4KiB>::containing_address(VirtAddr::new(user_stack_start as u64));
    let end_page = Page::<Size4KiB>::containing_address(VirtAddr::new((user_stack_end-1) as u64));
    let page_range= Page::range_inclusive(start_page, end_page);
    for page in page_range {
        deallocate_virtual_page(page_table_frame, page);
    }
}

fn deallocate_elf_regions(regions : &[ElfMemRegion], page_table_frame : PhysFrame){
    for &region in regions {
        let start_page = Page::<Size4KiB>::containing_address(region.start);
        let end_page = Page::<Size4KiB>::containing_address(region.end-1);
        let page_range= Page::range_inclusive(start_page, end_page);
        for page in page_range {
            deallocate_virtual_page(page_table_frame, page);
        }
    }
}

// only cleanup what can be immediately
pub fn cleanup_process_mem_soft(process : &Process){
    deallocate_user_heap(process.heap_start, process.heap_break, process.page_table_phys);
    deallocate_user_stack(process.page_table_phys);
    deallocate_elf_regions(&process.elf_regions, process.page_table_phys);
    // TODO
}

// TODO : call when cleaning up zombie processes
pub fn cleanup_process_complete(process : &Process){
    serial_println!("cleanup complete");
    serial_println!("before kernel stack cleanup");
    deallocate_kernel_stack(process.kernel_stack_top, process.page_table_phys);
    deallocate_userspace_page_tables(process.page_table_phys);
    serial_println!("after PML4 cleanup");
}

fn init_fd_list() -> Result<Vec<Option<Arc<OpenedFile>>>, FileError> {
    let mut v = Vec::with_capacity(1);

    // TODO : should I cache the inode instead of searching the path ? could make it more performant, + would prevent a security risk in the future by changing the root mount ?
    v.push(Some(OpenedFile::new("/dev/stdout", false, true, false)?));
    Ok(v)
}


impl Process {
    pub fn add_opened_file(&mut self, file : Arc<OpenedFile>) -> Fd {
        if self.free_fd_nb == 0 {
            let fd = self.fd_list.len();
            self.fd_list.push(Some(file));
            return Fd(fd);
        }
        for (idx, f) in self.fd_list.iter_mut().enumerate(){
            if f.is_none(){
                *f = Some(file);
                self.free_fd_nb -= 1;
                return Fd(idx);
            }
        }
        
        unreachable!();
    }

    pub fn remove_opened_file(&mut self, fd: Fd) -> Option<()> {
        let _ = self.fd_list.get_mut(fd.0)?.take();
        self.free_fd_nb += 1;

        // close all the None at the end (it will keep the allocated part which speeds up the push, and prevent the need to scan the vec for free fd)
        while let Some(None) = self.fd_list.last(){
            self.fd_list.pop();
            self.free_fd_nb -= 1;
        }
        
        Some(())
    }

    pub fn empty_process(cwd_path : String) -> Pid {
        with_scheduler_no_int(|scheduler|{

            let page_table_phys = allocate_userspace_level_4_table();
            
            let parent_pid = scheduler.current_process;

            map_page_phys_at_in(page_table_phys.start_address(), PhysFrame::containing_address(PhysAddr::new(0xb8000)), VirtAddr::new(0xb8000), PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::NO_EXECUTE).unwrap().flush(); // TODO : should I realy unwrap ?
            let new_process_pid = scheduler.add_process(Process { 
                pid: Pid(NonZero::new(usize::MAX).unwrap()), // will be replaced in add_process
                children: Vec::new(),
                parent: parent_pid,
                kernel_stack_top: VirtAddr::new(0), // is replaced just after 
                page_table_phys,
                state: SchedulerState::Loading,
                process_kind: ProcessKind::User,
                saved_regs: Registers::default(),
                kernel_context: KernelContext::default(),
                fxstate: DEFAULT_FXSTATE.get().unwrap().clone(),
                cwd_path,
                fd_list: init_fd_list().unwrap(), // TODO : better error handling (either cache the stdout to not have to search it, or return a result from this fun)
                free_fd_nb: 0,
                heap_start: VirtAddr::new(USER_HEAP_START as u64),
                heap_break: VirtAddr::new(USER_HEAP_START as u64),
                heap_max: VirtAddr::new((USER_HEAP_START + USER_HEAP_SIZE) as u64),
                elf_regions: Vec::new(),
            });

            let new_process_idx = new_process_pid.0.get() - 1;

            let stack_end = allocate_kernel_stack(new_process_idx, page_table_phys);

            new_process_pid.get_process_mut(&mut scheduler.processes).kernel_stack_top = VirtAddr::new(stack_end);

            if let Some(parent_pid) = parent_pid {
                parent_pid.get_process_mut(&mut scheduler.processes).children.push(new_process_pid);
            }

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

        let kernel_stack_end = allocate_kernel_stack(new_process_idx, kernel_page_table);

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

        scheduler_lock.processes.push(Box::new(Process { 
            pid: new_process_pid, 
            children: Vec::new(),
            parent: None,
            kernel_stack_top: VirtAddr::new(kernel_stack_end), 
            page_table_phys: kernel_page_table,
            state: SchedulerState::Ready(ReadyMode::Kernel),
            process_kind: ProcessKind::Kernel,
            saved_regs,
            kernel_context,
            fxstate: DEFAULT_FXSTATE.get().unwrap().clone(),
            cwd_path: String::new(),
            fd_list: Vec::new(),
            free_fd_nb: 0,
            heap_start: VirtAddr::new(0),
            heap_break: VirtAddr::new(0),
            heap_max: VirtAddr::new(0),
            elf_regions: Vec::new(),
        }));
        
    }

    fn write_to_process_stack_bytes(page_table : PhysFrame<Size4KiB>, stack_ptr : &mut u64, bytes : &[u8]){
        unsafe {
            *stack_ptr -= bytes.len() as u64;
            let phys_ptr = translate_addr_in(page_table, VirtAddr::new(*stack_ptr)).unwrap(); // TODO : replace unwrap with real error handling ?
            let real_ptr_addr = PHYSICAL_MEMORY_OFFSET.as_u64() + phys_ptr.as_u64();
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
            Self::write_to_process_stack_u64(page_table, &mut current_stack_ptr, arg_ptr);
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