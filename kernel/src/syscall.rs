use core::{arch::naked_asm, ops::{ControlFlow, Deref, DerefMut}};

use alloc::{slice, str};
use syscall_nbs::{SYSCALL_EXEC, SYSCALL_EXIT, SYSCALL_GET_CHAR, SYSCALL_PRINT, SYSCALL_WAIT_PID};
use x86_64::{VirtAddr, instructions::interrupts, structures::paging::{OffsetPageTable, Page, PageTableFlags, Size4KiB}};

use crate::{allocator::get_page_flags_in, elf::load_elf, initrd::initrd_get_file_content, interrupts::{KEYBOARD_RINGBUF}, paging::{PHYSICAL_MEMORY_OFFSET, active_level_4_table}, print, process::{Pid, Process}, scheduler::{ReadyMode, SCHEDULER, Scheduler, SchedulerState, schedule}, serial_println, userspace::USER_STACK_TOP, utils::Registers};

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

fn syscall_interrupt_handler(regs : &mut SyscallRegs){
    let sycall_nb = regs.rax;
    //serial_println!("syscall rax number : {}", sycall_nb);
    let ret = match sycall_nb {
        SYSCALL_EXIT => syscall_exit(regs).map(|_| 0 as u64),
        SYSCALL_PRINT => syscall_print(regs).map(|_| 0 as u64), // TODO : change these syscalls ?
        SYSCALL_EXEC => syscall_exec(regs),
        SYSCALL_GET_CHAR => syscall_get_char(regs),
        SYSCALL_WAIT_PID => syscall_wait_pid(regs).map(|_| 0),
        _ => None,
    }.unwrap_or(u64::MAX);
    regs.rax = ret;
}

fn syscall_exit(regs : &mut SyscallRegs) -> Option<()> {
    
    interrupts::without_interrupts(||{
        let mut scheduler_lock = SCHEDULER.lock();
        let current_process_pid = scheduler_lock.current_process.unwrap();
        serial_println!("current_process_pid : {:?}", current_process_pid);
        if current_process_pid.0.get() == 1 {
            panic!("tried to exit init");
        }
        let exit_code = regs.get_arg(1);
        
        let current_proc = current_process_pid.get_process_mut(&mut scheduler_lock.processes);
        current_proc.state = SchedulerState::Zombie(exit_code as i32);

        let parent_pid = current_process_pid.get_process(&scheduler_lock.processes).parent;
        
        if let Some(parent_pid) = parent_pid {
            let parent = parent_pid.get_process_mut(&mut scheduler_lock.processes);
            if parent.state == SchedulerState::WaitPid(current_process_pid) {
                //parent.state = SchedulerState::Ready(ReadyMode::Kernel);
                scheduler_lock.make_runnable_kernel(parent_pid);
            }
        }
    });
    
    
    schedule(regs);
    Some(())
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
    let file_content = initrd_get_file_content(path);

    let new_proc_pid = interrupts::without_interrupts(||{
        let new_proc_pid = Process::empty_process();
        let mut scheduler_lock = SCHEDULER.lock();
        let process = new_proc_pid.get_process(&scheduler_lock.processes);

        //unsafe { Cr3::write(process.page_table_phys, Cr3Flags::empty()) };
        //*CURRENT_PROCESS.lock().deref_mut() = Some(new_proc_pid);

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
    // TODO : use scheduler for that (BlockedOnKeyboard state) instead of busy wait
    loop {
        serial_println!("get_char: trying pop");
        
        let c = interrupts::without_interrupts(|| {
            KEYBOARD_RINGBUF.lock().pop()
        });
        if let Some(c) = c {
            serial_println!("get_char: got {:?}", c);
            return Some(c as u64) 
        }

        interrupts::without_interrupts(||{
            // TODO : add without_interrupts each time the scheduler lock is taken ? (like with the keyboard)
            let mut scheduler_lock = SCHEDULER.lock();
            let current_pid = scheduler_lock.current_process.unwrap();
            serial_println!("get_char: current pid {:?}", current_pid);
            current_pid.get_process_mut(&mut scheduler_lock.processes).state = SchedulerState::WaitKeyboard;
            scheduler_lock.processes_waiting_keyboard.push_back(current_pid);
        });

        schedule(regs);
        serial_println!("get_char: resumed after schedule");
    }
}

fn syscall_wait_pid(regs : &mut SyscallRegs) -> Option<()> {
    let waited_pid = unsafe { Pid::new_unchecked(regs.get_arg(1) as usize) }?;

    serial_println!("waiting for pid {}", waited_pid.0.get());
    
    // TODO : how could I ad without_interrupts because of the return ?
    let control_flow = interrupts::without_interrupts(||{
        let mut scheduler_lock = SCHEDULER.lock();
        let current_pid = scheduler_lock.current_process.unwrap();

        if !current_pid.get_process(&scheduler_lock.processes).children.contains(&waited_pid) {
            // not a children
            return ControlFlow::Break(None);
        }

        if let SchedulerState::Zombie(exit_code) = waited_pid.get_process(&scheduler_lock.processes).state {
            regs.rax = exit_code as u64;
            waited_pid.get_process_mut(&mut scheduler_lock.processes).state = SchedulerState::Dead;
            current_pid.get_process_mut(&mut scheduler_lock.processes).children.retain(|&pid| pid != waited_pid); 
            return ControlFlow::Break(Some(()));
        }

        current_pid.get_process_mut(&mut scheduler_lock.processes).state = SchedulerState::WaitPid(waited_pid);
        ControlFlow::Continue(())
    });

    if let ControlFlow::Break(res) = control_flow {
        return res;
    }

    schedule(regs);

    Some(())
}