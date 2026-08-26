use core::{arch::{asm, naked_asm}, ops::{ControlFlow, Deref, DerefMut}, ptr};

use alloc::{borrow::ToOwned, slice, str, string::String, vec::Vec};
use shared_consts::{Arg, CREATE_FILE, DirChild, Fd, READABLE, SHUTDOWN_REBOOT, SYSCALL_CHANGE_CWD, SYSCALL_CLOSE, SYSCALL_EXEC, SYSCALL_EXIT, SYSCALL_FSTAT, SYSCALL_GET_CWD, SYSCALL_GET_DIR_CHILDREN, SYSCALL_GET_RANDOM, SYSCALL_OPEN, SYSCALL_READ, SYSCALL_SBRK, SYSCALL_SHUTDOWN, SYSCALL_STAT, SYSCALL_WAIT_PID, SYSCALL_WRITE, Stat, StatMode, WRITABLE};
use x86_64::{VirtAddr, align_up, structures::paging::{OffsetPageTable, Page, PageTableFlags, Size4KiB, mapper::MapToError}};

use crate::{allocator::serial_print_allocs_deallocs, elf::load_elf, fs::{FileError, canonicalize_path, file_stat, get_inode, process_close_file, process_fstat, process_get_dir_children, process_open_file, process_read, process_write}, paging::{PHYSICAL_MEMORY_OFFSET, active_level_4_table, get_page_flags_in, map_page_at_in, translate_addr_in}, power::{reboot, shutdown}, process::{Pid, Process, cleanup_process_complete, destroy_process_because_err}, random::random_bytes, scheduler::{ReadyMode, SCHEDULER, SchedulerState, WaitReason, kill_current_and_schedule, schedule, with_scheduler_no_int}, security::spectre_fence, serial_println, utils::Registers};


const USER_CS: u64 = 0x23;
const USER_SS: u64 = 0x1b;

// TODO : after adding multiple cpu support, make this per cpu (gs infos, then use the user_rsp and kernel_rsp with the gs reg)
#[unsafe(no_mangle)]
pub static mut SYSCALL_USER_RSP: u64 = 0;

#[unsafe(no_mangle)]
pub static mut SYSCALL_KERNEL_RSP: u64 = 0;

// if return address is "canonical" (what does it mean ?) use sysret instead of iretq, because it is a lot faster (see https://kernel-internals.org/arch/x86/syscall-entry/ and https://kernel-internals.org/arch/x86/war-stories/)

#[unsafe(naked)]
pub unsafe extern "C" fn syscall_instr_entry(){
    naked_asm!(
        "
        mov qword ptr [rip + {user_rsp}], rsp
        mov rsp, qword ptr [rip + {kernel_rsp}]

        push {user_ss}
        push qword ptr [rip + {user_rsp}]
        push r11
        push {user_cs}
        push rcx
        
        push rax
        push rbx
        push rcx
        push rdx
        push rsi
        push rdi
        push rbp
        push r8
        push r9
        push r10
        push r11
        push r12
        push r13
        push r14
        push r15

        mov rdi, rsp
        call {handler}

        pop r15
        pop r14
        pop r13
        pop r12
        pop r11
        pop r10
        pop r9
        pop r8
        pop rbp
        pop rdi
        pop rsi
        pop rdx
        pop rcx
        pop rbx
        pop rax
        iretq
        ",
        user_cs = const USER_CS,
        user_ss = const USER_SS,
        handler = sym syscall_interrupt_handler,
        user_rsp = sym SYSCALL_USER_RSP,
        kernel_rsp = sym SYSCALL_KERNEL_RSP,
    )
}

#[repr(transparent)]
struct SyscallRegs(Registers);

impl Deref for SyscallRegs {
    type Target = Registers;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for SyscallRegs {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl SyscallRegs {
    fn get_arg(&self, nb : u8) -> u64 {
        match nb {
            1 => self.rdi,
            2 => self.rsi,
            3 => self.rdx,
            4 => self.r10,
            5 => self.r8,
            6 => self.r9,
            _ => unreachable!(), // coding error
        }
    }
}

fn mark_current_as_user(){
    with_scheduler_no_int(|scheduler|{
        let pid = scheduler.current_process.unwrap();
        let process = pid.get_process_mut(&mut scheduler.processes);

        process.state = SchedulerState::Ready(ReadyMode::User);
    })
}

// TODO : add a doc with prototypes/list of args for each syscall

fn syscall_interrupt_handler(regs : &mut SyscallRegs){
    let sycall_nb = regs.rax;
    //serial_println!("syscall rax number : {}", sycall_nb);
    let ret = match sycall_nb {
        SYSCALL_EXIT => syscall_exit(regs),
        SYSCALL_EXEC => syscall_exec(regs),
        SYSCALL_WAIT_PID => syscall_wait_pid(regs).map(|_| 0),
        SYSCALL_STAT => syscall_stat(regs).map(|_| 0),
        SYSCALL_OPEN => syscall_open(regs).map(|fd| fd.into_raw()),
        SYSCALL_CLOSE => syscall_close(regs).map(|_| 0),
        SYSCALL_GET_CWD => syscall_get_cwd(regs),
        SYSCALL_GET_DIR_CHILDREN => syscall_get_dir_children(regs),
        SYSCALL_SBRK => syscall_sbrk(regs),
        SYSCALL_SHUTDOWN => syscall_shutdown(regs),
        SYSCALL_CHANGE_CWD => syscall_change_cwd(regs).map(|_| 0),
        SYSCALL_FSTAT => syscall_fstat(regs).map(|_| 0),
        SYSCALL_READ => syscall_read(regs),
        SYSCALL_WRITE => syscall_write(regs),
        SYSCALL_GET_RANDOM => syscall_get_random(regs).map(|_| 0),
        _ => None,
    }.unwrap_or(u64::MAX);
    regs.rax = ret;
    mark_current_as_user();
}

fn syscall_exit(regs : &mut SyscallRegs) -> ! {
    let exit_code = regs.get_arg(1);
    kill_current_and_schedule(exit_code as i32)
}


// TODO : look at all the memory regions, and also a check to have kernel memory forbidden (for ex memory > 0xXXXXX) (or do the reverse, put only the user accessible range ? and not too low, for ex NULL ?)
fn check_ptr(ptr : usize, len : usize, is_write : bool) -> bool {
    let end = match ptr.checked_add(len){
        Some(end) => end,
        None => return false,
    };
    if len == 0 {
        return true;
    }
    let start_page = Page::<Size4KiB>::containing_address(VirtAddr::new(ptr as u64));
    let end_page = Page::<Size4KiB>::containing_address(VirtAddr::new((end-1) as u64));

    let page_table = unsafe { active_level_4_table() };
    let phys_offset = PHYSICAL_MEMORY_OFFSET;
    let mut mapper = unsafe { OffsetPageTable::new(page_table, phys_offset) };


    for page in Page::range_inclusive(start_page, end_page){ 
        let flags = get_page_flags_in(&mut mapper, page.start_address());
        match flags {
            Some(flags) => {
                if !flags.contains(PageTableFlags::USER_ACCESSIBLE){
                    return false;
                }
                if is_write && !flags.contains(PageTableFlags::WRITABLE){
                    return false;
                }
            },
            None => return false,
        }
    }
    true
}

// TODO : to make SMAP useful, randomize the physmap (if it is not, attacker that controls a ptr in the kernel could just pass the virt user address + CONST_PHYS_OFF instead of the virt user address)

#[must_use = "the guard must be kept alive for its Drop implementation"]
struct SmapGuard(());

impl SmapGuard {
    #[inline]
    fn new() -> SmapGuard {
        unsafe {
            asm!("stac", options(nostack));
        }
        SmapGuard(())
    }
}

impl Drop for SmapGuard {
    #[inline]
    fn drop(&mut self) {
        unsafe {
            asm!("clac", options(nostack));
        }
    }
}

fn copy_buf_from_user<T : Copy>(user_ptr : *const T, buf_len : usize) -> Option<Vec<T>> {
    let _smap_guard = SmapGuard::new();
    if !check_ptr(user_ptr as usize, buf_len * size_of::<T>(), false){
        return None;
    }
    spectre_fence();
    let buf = unsafe { slice::from_raw_parts(user_ptr, buf_len) };
    let vec = buf.to_vec();
    Some(vec)
}

fn copy_str_from_user(user_ptr : *const u8, str_len : usize) -> Option<String> {
    let _smap_guard = SmapGuard::new();
    if !check_ptr(user_ptr as usize, str_len, false) {
        return None;
    }
    spectre_fence();
    let slice = unsafe { slice::from_raw_parts(user_ptr, str_len) };
    let str = str::from_utf8(slice).ok()?;
    let str = str.to_owned();
    Some(str)
}

// invariant : need to pass a valid ptr (that was already checked)
unsafe fn store_to_user<T : Copy>(user_ptr : *mut T, val : T){
    debug_assert!(check_ptr(user_ptr as usize, size_of::<T>(), true));
    let _smap_guard = SmapGuard::new();
    unsafe {
        *user_ptr = val;
    }
}

struct UserBuf<T : Copy> {
    ptr : *mut T,
    len : usize,
}

impl<T : Copy> UserBuf<T>{
    fn new(user_ptr : *mut T, buf_len : usize) -> Option<UserBuf<T>> {
        if !check_ptr(user_ptr as usize, buf_len * size_of::<T>(), true){
            return None;
        }
        let user_buf = UserBuf { 
            ptr: user_ptr, 
            len: buf_len 
        };
        Some(user_buf)
    }
}

// TODO : make this a method to UserBuf ?
fn copy_buf_to_user<T : Copy>(user_buf : UserBuf<T>, data : &[T]) {
    debug_assert!(user_buf.len >= data.len());
    let _smap_guard = SmapGuard::new();
    let buf = unsafe { slice::from_raw_parts_mut(user_buf.ptr, user_buf.len) };
    buf[..data.len()].copy_from_slice(data);
}

fn syscall_exec(regs : &mut SyscallRegs) -> Option<u64> {
    serial_println!("start exe");

    serial_print_allocs_deallocs("before exec");
    
    let path_ptr = regs.get_arg(1) as *const u8;
    let path_len = regs.get_arg(2);

    let args_ptr = regs.get_arg(3) as *const Arg;
    let args_len = regs.get_arg(4);

    let path = copy_str_from_user(path_ptr, path_len as usize)?;

    let args = copy_buf_from_user(args_ptr, args_len as usize)?;

    let args_strings = args.into_iter().map(|arg| copy_str_from_user(arg.ptr, arg.len)).collect::<Option<Vec<_>>>()?; // TODO : optimize this ?

    // TODO : merge this block with_scheduler_no_int with the one next ?
    let canonicalized_path = with_scheduler_no_int(|scheduler|{
        let current_cwd = &scheduler.current_process.unwrap().get_process(&scheduler.processes).cwd_path;
        canonicalize_path(&path, current_cwd)
    })?;


    // TODO : merge this with the init executing, by having an run_exe function in userspace.rs
    let inode = get_inode(&canonicalized_path).ok()?;
    let file_content = inode.read_entire_file_in_mem().ok()?;

    let new_proc_pid = with_scheduler_no_int(|scheduler| {
        let current_cwd_path = {
            let current_proc = scheduler.current_process.unwrap().get_process(&scheduler.processes);
            let current_cwd_path = current_proc.cwd_path.clone();
            current_cwd_path
        };
        let new_proc_pid = Process::empty_process(current_cwd_path, scheduler);
        let process = new_proc_pid.get_process_mut(&mut scheduler.processes);

        let elf = match load_elf(&file_content, process).ok(){  // TODO : in case like this in syscalls, instead of destroying the error and returning a non specific error to syscall, return the error (change abi ? how would it work ? maybe have a ptr, with a certain memory allocated that is the maximum size that can be used as an used for the error, use an enum and sizeof on it ?)
            Some(elf) => elf,
            None => {
                destroy_process_because_err(scheduler, new_proc_pid);
                return None;
            },
        };
        let entrypoint = elf.ehdr.e_entry as usize;
        new_proc_pid.get_process_mut(&mut scheduler.processes).init_process(entrypoint, &args_strings);
        scheduler.make_runnable(new_proc_pid);
        Some(new_proc_pid)
    })?;

    serial_print_allocs_deallocs("after exec");

    Some(new_proc_pid.0.get() as u64)
}

fn syscall_wait_pid(regs : &mut SyscallRegs) -> Option<()> {
    let waited_pid = unsafe { Pid::new_unchecked(regs.get_arg(1) as usize) }?;

    serial_println!("waiting for pid {}", waited_pid.0.get());

    loop {
        let control_flow = with_scheduler_no_int(|scheduler|{
            if scheduler.processes.len() < waited_pid.0.get() {
                return ControlFlow::Break(None);
            }
            
            let current_pid = scheduler.current_process.unwrap();

            if !current_pid.get_process(&scheduler.processes).children.contains(&waited_pid) {
                // not a children
                return ControlFlow::Break(None);
            }

            if let SchedulerState::Zombie(exit_code) = waited_pid.get_process(&scheduler.processes).state {
                regs.rax = exit_code as u64;
                scheduler.mark_dead(waited_pid);
                current_pid.get_process_mut(&mut scheduler.processes).children.retain(|&pid| pid != waited_pid);
                cleanup_process_complete(waited_pid.get_process(&scheduler.processes));
                serial_print_allocs_deallocs("after zombie complete cleanup");
                return ControlFlow::Break(Some(()));
            }

            current_pid.get_process_mut(&mut scheduler.processes).state = SchedulerState::Wait(WaitReason::WaitPid(waited_pid));
            ControlFlow::Continue(())
        });

        if let ControlFlow::Break(res) = control_flow {
            serial_println!("before return waitpid");
            return res;
        }
        schedule(regs);
    }
}

fn syscall_stat(regs : &mut SyscallRegs) -> Option<()>{
    let path_ptr = regs.get_arg(1) as *const u8;
    let path_len = regs.get_arg(2) as usize;
    let stat_ptr = regs.get_arg(3) as *mut Stat;

    if !check_ptr(stat_ptr as usize, size_of::<Stat>(), true){
        return None;
    }

    let path_str = copy_str_from_user(path_ptr, path_len)?;

    let stat = file_stat(&path_str).ok()?;

    unsafe {
        store_to_user(stat_ptr, stat);
    }

    Some(())
}

fn syscall_open(regs : &mut SyscallRegs) -> Option<Fd> {
    let path_ptr = regs.get_arg(1) as *const u8;
    let path_len = regs.get_arg(2) as usize;
    let mode = regs.get_arg(3);
    let path = copy_str_from_user(path_ptr, path_len)?;
    // TODO : use the bitflags crate instead ?
    let is_readable = (mode & READABLE) != 0;
    let is_writable = (mode & WRITABLE) != 0;
    let create_file = (mode & CREATE_FILE) != 0;
    serial_println!("syscall open of path {}", path);
    process_open_file(&path, is_readable, is_writable, create_file)
}

fn syscall_close(regs : &mut SyscallRegs) -> Option<()> {
    let fd = regs.get_arg(1);
    let fd = Fd::from_raw(fd);
    process_close_file(fd)
}

fn syscall_get_cwd(regs : &mut SyscallRegs) -> Option<u64> {
    let cwd_buf = regs.get_arg(1) as *mut u8;
    let cwd_len = regs.get_arg(2) as usize;

    let cwd_user_buf = UserBuf::new(cwd_buf, cwd_len)?;
    let mut cwd_buf = alloc::vec![0; cwd_len];

    let cwd_len = with_scheduler_no_int(|scheduler|{
        let cwd = &scheduler.current_process.unwrap().get_process(&scheduler.processes).cwd_path;
        serial_println!("cwd in syscall  : {}", cwd);
        serial_println!("cwd.len() > cwd_len : {} > {}", cwd.len(), cwd_len);
        if cwd.len() > cwd_len {
            return None;
        }
        cwd_buf[..cwd.len()].copy_from_slice(cwd.as_bytes());
        Some(cwd.len())
    })?;

    copy_buf_to_user(cwd_user_buf, &cwd_buf);

    Some(cwd_len as u64)
}

fn syscall_get_dir_children(regs : &mut SyscallRegs) -> Option<u64> {
    let fd = regs.get_arg(1);
    let children_ptr = regs.get_arg(2) as *mut DirChild;
    let children_len = regs.get_arg(3) as usize;
    let fd = Fd::from_raw(fd);

    let children_user_buf = UserBuf::new(children_ptr, children_len)?;
    let mut children_buf = alloc::vec![DirChild::zeroed(); children_len];
    let children_nb = process_get_dir_children(fd, &mut children_buf).ok()?;
    copy_buf_to_user(children_user_buf, &children_buf);
    Some(children_nb as u64)
}

fn syscall_sbrk(regs : &mut SyscallRegs) -> Option<u64> {
    let increment = regs.get_arg(1); // TODO : make it i64, and handle shrinking
    let (page_table_phys, current_break, new_break) = with_scheduler_no_int(|scheduler|{
        let current_proc = scheduler.current_process.unwrap().get_process_mut(&mut scheduler.processes);
        let current_break = current_proc.heap_break.as_u64();
        let new_break = current_break.checked_add(increment)?;
        if new_break > current_proc.heap_max.as_u64() || new_break < current_proc.heap_start.as_u64() {
            return None;
        }
        let page_table_phys = current_proc.page_table_phys;
        current_proc.heap_break = VirtAddr::new(new_break);
        Some((page_table_phys, current_break, new_break))
    })?;

    let map_start  = align_up(current_break, 4096);
    let map_end = align_up(new_break, 4096);
    
    if increment > 0 && map_start < map_end {    
        let start_page = Page::<Size4KiB>::containing_address(VirtAddr::new(map_start));
        let end_page = Page::<Size4KiB>::containing_address(VirtAddr::new(map_end-1));
        let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE | PageTableFlags::NO_EXECUTE;
        for page in Page::range_inclusive(start_page, end_page){
            match map_page_at_in(page_table_phys.start_address(), page.start_address(), flags){
                Ok(flush) => {
                    flush.flush();
                    // TODO : why not make it also return the phys frame to not have to translate the addr after
                    let page_phys = unsafe { translate_addr_in(page_table_phys, page.start_address()) }.unwrap();
                    let page_phys_virt = PHYSICAL_MEMORY_OFFSET + page_phys.as_u64();
                    unsafe {
                        ptr::write_bytes(page_phys_virt.as_mut_ptr::<u8>(), 0, page.size() as usize);
                    }
                },
                Err(MapToError::PageAlreadyMapped(_)) => {},
                Err(e) => panic!("error when mapping user heap pages in sbrk : {:?}", e),
            }
        }
    }
    
    Some(current_break)
    
}

fn syscall_shutdown(regs : &mut SyscallRegs) -> ! {
    let flags = regs.get_arg(1);
    if (flags & SHUTDOWN_REBOOT) != 0 {
        reboot()
    } else {
        shutdown(flags)
    }
}

fn syscall_change_cwd(regs : &mut SyscallRegs) -> Option<()> {
    let path_ptr = regs.get_arg(1) as *const u8;
    let path_len = regs.get_arg(2) as usize;

    let path = copy_str_from_user(path_ptr, path_len)?;

    with_scheduler_no_int(|scheduler|{
        let canonicalized_path = {
            let current_cwd = &scheduler.current_process.unwrap().get_process(&scheduler.processes).cwd_path;
            canonicalize_path(&path, current_cwd)?.into_owned()
        };
        match file_stat(&canonicalized_path) {
            Ok(Stat { mode: StatMode::Directory }) => {},
            _ => return None,
        }
        scheduler.current_process.unwrap().get_process_mut(&mut scheduler.processes).cwd_path = canonicalized_path;
        Some(())
    })
}

fn syscall_fstat(regs : &mut SyscallRegs) -> Option<()>{
    let fd = regs.get_arg(1);
    let fd = Fd::from_raw(fd);
    let stat_ptr = regs.get_arg(2) as *mut Stat;

    if !check_ptr(stat_ptr as usize, size_of::<Stat>(), true){
        return None;
    }

    let stat = process_fstat(fd).ok()?;

    unsafe {
        store_to_user(stat_ptr, stat);
    }

    Some(())
}

// TODO : make the fs async first (and then add synchronous wrappers in userspace)

// TODO : return in the trait for devices (then I will use this for traits for any file) an enum with either the count, or pending (with a wait handle ?), would help transitionning to async
// helper function for syscall_read
fn read_retry(fd : Fd, buf : &mut [u8], regs : &mut SyscallRegs) -> Result<usize, FileError> {
    loop {
        let read = process_read(fd, buf);
        if let Err(FileError::NoDataYet) = read {
            schedule(regs);
            continue;
        } 
        return read;
    }
}

fn syscall_read(regs : &mut SyscallRegs) -> Option<u64> {
    let fd = regs.get_arg(1);
    let buf = regs.get_arg(2) as *mut u8;
    let buf_size = regs.get_arg(3) as usize;

    let fd = Fd::from_raw(fd);

    serial_println!(
        "syscall_read entered: fd={:?}, ptr={:#x}, len={}",
        fd,
        buf as usize,
        buf_size,
    );

    let user_buf = UserBuf::new(buf, buf_size)?;
    let mut buf = alloc::vec![0; buf_size];

    let read = read_retry(fd, &mut buf, regs).ok()?;    
    
    copy_buf_to_user(user_buf, &buf[..read]);

    Some(read as u64)
}

fn syscall_write(regs : &mut SyscallRegs) -> Option<u64> {
    let fd = regs.get_arg(1);
    let buf = regs.get_arg(2) as *const u8;
    let buf_size = regs.get_arg(3) as usize;

    let fd = Fd::from_raw(fd);

    serial_println!(
        "syscall_write entered: fd={:?}, ptr={:#x}, len={}",
        fd,
        buf as usize,
        buf_size,
    );

    let buf = copy_buf_from_user(buf, buf_size)?;

    let written = process_write(fd, &buf).ok()? as u64;

    Some(written)
}

fn syscall_get_random(regs : &mut SyscallRegs) -> Option<()> {
    let buf_ptr = regs.get_arg(1) as *mut u8;
    let buf_len = regs.get_arg(2) as usize;
    let user_buf = UserBuf::new(buf_ptr, buf_len)?;
    let mut buf = alloc::vec![0; buf_len]; // TODO : in those cases, maybe use a smallvec (depending on the case ?) to not always allocate on the heap, especially when most of the time the buf is small

    random_bytes(&mut buf);

    copy_buf_to_user(user_buf, &buf);

    Some(())
}