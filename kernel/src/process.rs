use core::{fmt::Debug, num::NonZero, ops::Deref, ptr};

use alloc::{boxed::Box, string::String, sync::Arc, vec::Vec};
use shared_consts::{Fd, RNG_SEED_SIZE, USER_HEAP_SIZE, USER_HEAP_START};
use spin::Mutex;
use x86_64::{PhysAddr, VirtAddr, instructions::interrupts::without_interrupts, registers::{control::Cr3, rflags::RFlags}, structures::paging::{Page, PageTableFlags, PhysFrame, Size4KiB}};

use crate::{allocator::{allocate_userspace_level_4_table, deallocate_userspace_page_tables, deallocate_virtual_page}, fs::{FileError, Inode, add_inode, get_inode}, gdt::GDT, paging::{PHYSICAL_MEMORY_OFFSET, map_page_at_in, map_page_phys_at_in, translate_addr_in}, random::random_bytes, scheduler::{KernelContext, ProcessSlot, ReadyMode, SCHEDULER, Scheduler, SchedulerState, idle_main}, serial_println, sse::{DEFAULT_FXSTATE, FxState}, userspace::{USER_STACK_SIZE, USER_STACK_TOP}, utils::Registers};


#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Pid(shared_consts::Pid);

impl Debug for Pid {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f,  "{:?}", self.0)
    }
}

impl Deref for Pid {
    type Target = shared_consts::Pid;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Pid {
    pub const fn new(pid_idx : NonZero<u32>, generation : u32) -> Pid {
        Pid(shared_consts::Pid::new(pid_idx, generation))
    }

    pub fn from_raw(raw : u64) -> Option<Pid> {
        Some(Pid(shared_consts::Pid::from_raw(raw)?))
    }

    pub fn get_process(self, processes : &[ProcessSlot]) -> Option<&Process> {
        let slot = processes.get((self.get_idx().get()-1) as usize)?; 
        if slot.generation != self.get_gen() {
            serial_println!("get_process generation mismatch, in slot : {}, for pid {:?} : {}", slot.generation, self, self.get_gen());
            return None;
        }
        Some(slot.proc.as_ref())
    }
    
    // TODO : not unwrap and return Option<&mut Process> ?
    pub fn get_process_mut(self, processes : &mut [ProcessSlot]) -> Option<&mut Process> {
        let slot = processes.get_mut((self.get_idx().get()-1) as usize)?;
        if slot.generation != self.get_gen() {
            serial_println!("get_pget_process_mutrocess generation mismatch, in slot : {}, for pid {:?} : {}", slot.generation, self, self.get_gen());
            return None;
        }
        Some(slot.proc.as_mut())
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

pub struct FdSlot {
    pub generation : u32, // generation 0 is None
    pub opened_file : Option<Arc<OpenedFile>>,
}

impl FdSlot {
    fn new(opened_file : Arc<OpenedFile>) -> FdSlot {
        FdSlot { generation: 1, opened_file: Some(opened_file) }
    }
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
    pub fd_list : Vec<FdSlot>, // TODO : replace this with a SmallVec ?
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

fn allocate_kernel_stack(new_process_idx : u32, page_table_phys : PhysFrame) -> u64 {
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
}

// TODO : call when cleaning up zombie processes (maybe when replacing the pid with a new one, check with a flag if the cleanup process is complete, and if not run this)
pub fn cleanup_process_complete(process : &Process){
    serial_println!("cleanup complete");
    serial_println!("before kernel stack cleanup");
    deallocate_kernel_stack(process.kernel_stack_top, process.page_table_phys);
    deallocate_userspace_page_tables(process.page_table_phys);
    serial_println!("after PML4 cleanup");
}

// used to destroy completely a process because an error occured, for ex a invalid elf file during loading
pub fn destroy_process_because_err(scheduler : &mut Scheduler, new_proc_pid : Pid) -> Option<()> {
    if let Some(parent) = new_proc_pid.get_process(&scheduler.processes)?.parent {
        let child_idx = parent.get_process(&scheduler.processes)?.children.iter().position(|&pid| pid == new_proc_pid).unwrap();
        parent.get_process_mut(&mut scheduler.processes)?.children.swap_remove(child_idx);
    }
    let process = new_proc_pid.get_process(&scheduler.processes)?;
    cleanup_process_mem_soft(process);
    cleanup_process_complete(process);
    scheduler.mark_dead(new_proc_pid);
    Some(())
}

fn init_fd_list() -> Result<Vec<FdSlot>, FileError> {
    // TODO : should I cache the inode instead of searching the path ? could make it more performant, + would prevent a security risk in the future by changing the root mount ?
    let v = alloc::vec![
        FdSlot::new(OpenedFile::new("/dev/stdout", false, true, false)?),
        FdSlot::new(OpenedFile::new("/dev/stderr", false, true, false)?),
        FdSlot::new(OpenedFile::new("/dev/stdin", true, false, false)?)
    ];

    Ok(v)
}


impl Process {
    pub fn add_opened_file(&mut self, file : Arc<OpenedFile>) -> Fd {
        if self.free_fd_nb == 0 {
            let fd_idx = self.fd_list.len() as u32;
            self.fd_list.push(FdSlot::new(file));
            return Fd::new(fd_idx, 1);
        }
        for (idx, f) in self.fd_list.iter_mut().enumerate(){
            if f.opened_file.is_none(){
                f.opened_file = Some(file);
                self.free_fd_nb -= 1;
                f.generation = f.generation.wrapping_add(1);
                return Fd::new(idx as u32, f.generation);
            }
        }
        
        unreachable!();
    }

    pub fn remove_opened_file(&mut self, fd: Fd) -> Option<()> {
        let _ = self.fd_list.get_mut(fd.get_idx() as usize)?.opened_file.take();
        self.free_fd_nb += 1;

        // close all the None at the end (it will keep the allocated part which speeds up the push, and prevent the need to scan the vec for free fd)
        while let Some(None) = self.fd_list.last().map(|slot| &slot.opened_file) {
            self.fd_list.pop();
            self.free_fd_nb -= 1;
        }
        
        Some(())
    }

    pub fn empty_process(cwd_path : String, scheduler : &mut Scheduler) -> Option<Pid> {
        without_interrupts(||{
            let page_table_phys = allocate_userspace_level_4_table();
            
            let parent_pid = scheduler.current_process;

            map_page_phys_at_in(page_table_phys.start_address(), PhysFrame::containing_address(PhysAddr::new(0xb8000)), VirtAddr::new(0xb8000), PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::NO_EXECUTE).unwrap().flush(); // TODO : should I realy unwrap ?
            let new_process_pid = scheduler.add_process(Process { 
                pid: Pid(shared_consts::Pid::new(NonZero::new(u32::MAX).unwrap(), u32::MAX)), // will be replaced in add_process
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

            let new_process_idx = new_process_pid.get_idx().get() - 1;

            let stack_end = allocate_kernel_stack(new_process_idx, page_table_phys);

            new_process_pid.get_process_mut(&mut scheduler.processes)?.kernel_stack_top = VirtAddr::new(stack_end);

            if let Some(parent_pid) = parent_pid {
                parent_pid.get_process_mut(&mut scheduler.processes)?.children.push(new_process_pid);
            }

            Some(new_process_pid)
        })
    }

    pub const IDLE_PROCESS_PID: Pid = Pid::new(NonZero::new(1).unwrap(), 1);

    pub const INIT_PROCESS_PID : Pid = Pid::new(NonZero::new(2).unwrap(), 1);

    pub fn init_idle_process(){
        without_interrupts(||{
            let mut scheduler_lock = SCHEDULER.lock();
            let new_process_idx = scheduler_lock.processes.len() as u32; // TODO : check when adding scheduler if there is too much processes
            let new_process_pid = new_process_idx + 1;
            debug_assert_eq!(new_process_pid, Process::IDLE_PROCESS_PID.get_idx().get());
            let new_process_pid = Pid(shared_consts::Pid::new(NonZero::new(new_process_pid).unwrap(), 1));

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

            let proc_box = Box::new(Process { 
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
            });

            scheduler_lock.processes.push(ProcessSlot { 
                proc: proc_box, 
                generation: 1,
            });
        })
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

    fn init_process_stack<S : AsRef<str>>(&self, stack_top : usize, page_table : PhysFrame<Size4KiB>, args : &[S]) -> usize {
        let mut current_stack_ptr = stack_top as u64;
        let mut args_ptr = Vec::with_capacity(args.len());
        for arg in args.iter() {
            let arg = arg.as_ref();
            Self::write_to_process_stack_bytes(page_table, &mut current_stack_ptr, arg.as_bytes());
            args_ptr.push((current_stack_ptr, arg.len()));
        }

        // TODO : env vars ?

        current_stack_ptr &= !0xf;

        for &(arg_ptr, arg_len) in args_ptr.iter().rev() {
            Self::write_to_process_stack_u64(page_table, &mut current_stack_ptr, arg_ptr);
            Self::write_to_process_stack_u64(page_table, &mut current_stack_ptr, arg_len as u64);
        }


        Self::write_to_process_stack_u64(page_table, &mut current_stack_ptr, args.len() as u64);

        let mut rand_bytes = [0; RNG_SEED_SIZE + 8];
        random_bytes(&mut rand_bytes);
        Self::write_to_process_stack_bytes(page_table, &mut current_stack_ptr, &rand_bytes[..RNG_SEED_SIZE]);

        let stack_canary = u64::from_ne_bytes(rand_bytes[RNG_SEED_SIZE..].try_into().unwrap());
        Self::write_to_process_stack_u64(page_table, &mut current_stack_ptr, stack_canary);

        debug_assert_eq!(current_stack_ptr % 16, 0);
        current_stack_ptr as usize
    }

    pub fn init_process<S : AsRef<str>>(&mut self, entrypoint : usize, args : &[S]){

        let stack_segment = GDT.1.user_data_selector.0 as u64 | 3;
        let code_segment = GDT.1.user_code_selector.0 as u64 | 3;
        let rflags = RFlags::INTERRUPT_FLAG | RFlags::from_bits_truncate(0x2); // 0x2 is for the reserved bit that always need to be 1

        let stack_top = USER_STACK_TOP & !0xf; // 16 bytes align the stack, for syscall and iret
        let rsp = self.init_process_stack(stack_top, self.page_table_phys, args);

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