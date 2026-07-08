use core::{arch::naked_asm, ops::{ControlFlow, Deref, DerefMut}};

use alloc::{slice, str};
use shared_consts::{DirChild, Fd, READABLE, SYSCALL_CLOSE, SYSCALL_EXEC, SYSCALL_EXIT, SYSCALL_GET_CHAR, SYSCALL_GET_CWD, SYSCALL_OPEN, SYSCALL_PRINT, SYSCALL_STAT, SYSCALL_WAIT_PID, SYSCALL_GET_DIR_CHILDREN, Stat, WRITABLE};
use x86_64::{VirtAddr, instructions::interrupts, structures::paging::{OffsetPageTable, Page, PageTableFlags, Size4KiB}};

use crate::{allocator::get_page_flags_in, elf::load_elf, fs::{process_close_file, process_get_dir_children, process_open_file}, initrd::{file_stat, get_file_content}, interrupts::KEYBOARD_RINGBUF, paging::{PHYSICAL_MEMORY_OFFSET, active_level_4_table}, print, process::{Pid, Process}, scheduler::{SCHEDULER, SchedulerState, kill_current_and_schedule, schedule, with_scheduler_no_int}, serial_println, utils::Registers};

#[unsafe(naked)]
pub unsafe extern "C" fn syscall_interrupt_stub() -> ! {
    naked_asm!(
        "
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

        cld

        mov rdi, rsp # put in rdi the stack pointer to have as arg the reg struct
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
        handler = sym syscall_interrupt_handler,
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

// TODO : add a doc with prototypes/list of args for each syscall

fn syscall_interrupt_handler(regs : &mut SyscallRegs){
    let sycall_nb = regs.rax;
    //serial_println!("syscall rax number : {}", sycall_nb);
    let ret = match sycall_nb {
        SYSCALL_EXIT => syscall_exit(regs),
        SYSCALL_PRINT => syscall_print(regs).map(|_| 0), // TODO : change these syscalls ?
        SYSCALL_EXEC => syscall_exec(regs),
        SYSCALL_GET_CHAR => syscall_get_char(regs),
        SYSCALL_WAIT_PID => syscall_wait_pid(regs).map(|_| 0),
        SYSCALL_STAT => syscall_stat(regs).map(|_| 0),
        SYSCALL_OPEN => syscall_open(regs).map(|fd| fd.0 as u64),
        SYSCALL_CLOSE => syscall_close(regs).map(|_| 0),
        SYSCALL_GET_CWD => syscall_get_cwd(regs),
        SYSCALL_GET_DIR_CHILDREN => syscall_get_dir_children(regs),
        _ => None,
    }.unwrap_or(u64::MAX);
    regs.rax = ret;
}

fn syscall_exit(regs : &mut SyscallRegs) -> ! {
    let exit_code = regs.get_arg(1);
    kill_current_and_schedule(exit_code as i32)
}


// TODO : look at all the memory regions, and also a check to have kernel memory forbidden (for ex memory > 0xXXXXX)
fn check_ptr(ptr : usize, len : usize, is_write : bool) -> bool {
    let end = match ptr.checked_add(len){
        Some(end) => end,
        None => return false,
    };
    let start_page = Page::<Size4KiB>::containing_address(VirtAddr::new(ptr as u64));
    let end_page = Page::<Size4KiB>::containing_address(VirtAddr::new((end-1) as u64));

    let page_table = unsafe { active_level_4_table() };
    let phys_offset = *PHYSICAL_MEMORY_OFFSET.get().unwrap();
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

fn create_str<'a>(str_ptr : *const u8, str_len : usize) -> Option<&'a str> {
    if !check_ptr(str_ptr as usize, str_len, false) {
        return None;
    }
    let slice = unsafe { slice::from_raw_parts(str_ptr, str_len) };
    let s = match str::from_utf8(slice){
        Ok(s) => s,
        Err(_) => return None,
    };
    Some(s)
}

fn create_buf<'a, T>(buf_ptr : *mut T, buf_len : usize) -> Option<&'a mut [T]> {
    if !check_ptr(buf_ptr as usize, buf_len * size_of::<T>(), true){
        return None;
    }
    let slice = unsafe { slice::from_raw_parts_mut(buf_ptr, buf_len) };
    Some(slice)
}

fn syscall_print(regs : &mut SyscallRegs) -> Option<()> {
    let message_ptr = regs.get_arg(1) as *const u8;
    
    let message_len = regs.get_arg(2);

    let s = create_str(message_ptr, message_len as usize)?;

    print!("{}", s);
    Some(())
}

fn syscall_exec(regs : &mut SyscallRegs) -> Option<u64> {
    serial_println!("start exe");
    
    let path_ptr = regs.get_arg(1) as *const u8;
    let path_len = regs.get_arg(2);

    let path = create_str(path_ptr, path_len as usize)?;
    

    // TODO : merge this with the init executing, by having an run_exe function in userspace.rs
    let file_content = get_file_content(path).ok()?;

    let new_proc_pid = interrupts::without_interrupts(|| {
        let new_proc_pid = Process::empty_process();
        let mut scheduler_lock = SCHEDULER.lock();
        let process = new_proc_pid.get_process(&scheduler_lock.processes);

        let elf = load_elf(file_content, process);
        let entrypoint = elf.ehdr.e_entry as usize;
        new_proc_pid.get_process_mut(&mut scheduler_lock.processes).init_process(entrypoint);
        scheduler_lock.make_runnable(new_proc_pid);
        new_proc_pid
    });

    Some(new_proc_pid.0.get() as u64)

    //switch_to_userspace(entrypoint, USER_STACK_TOP, kernel_stack_top, user_page_table)
}

fn syscall_get_char(regs : &mut SyscallRegs) -> Option<u64> {
    loop {
        let control_flow = interrupts::without_interrupts(|| {
            serial_println!("get_char: trying pop");
            let c = KEYBOARD_RINGBUF.lock().pop();
            if let Some(c) = c {
                return ControlFlow::Break(c);
            }

            let mut scheduler_lock = SCHEDULER.lock();
            let current_pid = scheduler_lock.current_process.unwrap();
            serial_println!("get_char: current pid {:?}", current_pid);
            current_pid.get_process_mut(&mut scheduler_lock.processes).state = SchedulerState::WaitKeyboard;
            scheduler_lock.processes_waiting_keyboard.push_back(current_pid);
            ControlFlow::Continue(())
        });

        if let ControlFlow::Break(c) = control_flow {
            serial_println!("get_char: got {:?}", c);
            return Some(c as u64);
        }
        
        schedule(regs);
        serial_println!("get_char: resumed after schedule");
    }
}

fn syscall_wait_pid(regs : &mut SyscallRegs) -> Option<()> {
    let waited_pid = unsafe { Pid::new_unchecked(regs.get_arg(1) as usize) }?;

    serial_println!("waiting for pid {}", waited_pid.0.get());
    
    let control_flow = with_scheduler_no_int(|scheduler|{
        let current_pid = scheduler.current_process.unwrap();

        if !current_pid.get_process(&scheduler.processes).children.contains(&waited_pid) {
            // not a children
            return ControlFlow::Break(None);
        }

        if let SchedulerState::Zombie(exit_code) = waited_pid.get_process(&scheduler.processes).state {
            regs.rax = exit_code as u64;
            waited_pid.get_process_mut(&mut scheduler.processes).state = SchedulerState::Dead;
            current_pid.get_process_mut(&mut scheduler.processes).children.retain(|&pid| pid != waited_pid); 
            return ControlFlow::Break(Some(()));
        }

        current_pid.get_process_mut(&mut scheduler.processes).state = SchedulerState::WaitPid(waited_pid);
        ControlFlow::Continue(())
    });

    if let ControlFlow::Break(res) = control_flow {
        return res;
    }

    schedule(regs);

    Some(())
}

fn syscall_stat(regs : &mut SyscallRegs) -> Option<()>{
    let path_ptr = regs.get_arg(1) as *const u8;
    let path_len = regs.get_arg(2) as usize;
    let stat_ptr = regs.get_arg(3) as *mut Stat;

    if !check_ptr(stat_ptr as usize, size_of::<Stat>(), true){
        return None;
    }

    let path_str = create_str(path_ptr, path_len)?;

    let stat = file_stat(path_str).ok()?;

    unsafe {
        *stat_ptr = stat;
    }

    Some(())
}

fn syscall_open(regs : &mut SyscallRegs) -> Option<Fd> {
    let path_ptr = regs.get_arg(1) as *const u8;
    let path_len = regs.get_arg(2) as usize;
    let mode = regs.get_arg(3);
    let path = create_str(path_ptr, path_len)?;
    let is_readable = (mode & READABLE) != 0;
    let is_writable = (mode & WRITABLE) != 0;
    process_open_file(path, is_readable, is_writable)
}

fn syscall_close(regs : &mut SyscallRegs) -> Option<()> {
    let fd = regs.get_arg(1);
    let fd = Fd(fd as usize);
    process_close_file(fd)
}

fn syscall_get_cwd(regs : &mut SyscallRegs) -> Option<u64> {
    let cwd_buf = regs.get_arg(1) as *mut u8;
    let cwd_len = regs.get_arg(2) as usize;
    let cwd_buf = create_buf(cwd_buf, cwd_len)?;

    with_scheduler_no_int(|scheduler|{
        let cwd = &scheduler.current_process.unwrap().get_process(&scheduler.processes).cwd_path;
        serial_println!("cwd in syscall  : {}", cwd);
        serial_println!("cwd.len() > cwd_len : {} > {}", cwd.len(), cwd_len);
        if cwd.len() > cwd_len {
            return None
        }
        cwd_buf[..cwd.len()].copy_from_slice(cwd.as_bytes());
        Some(cwd.len() as u64)
    })
}

fn syscall_get_dir_children(regs : &mut SyscallRegs) -> Option<u64> {
    let fd = regs.get_arg(1);
    let children_ptr = regs.get_arg(2) as *mut DirChild;
    let children_len = regs.get_arg(3) as usize;
    let fd = Fd(fd as usize);
    let children_buf = create_buf(children_ptr, children_len)?;
    
    
    process_get_dir_children(fd, children_buf).ok().map(|nb| nb as u64)
}