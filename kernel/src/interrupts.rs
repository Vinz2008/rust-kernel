use core::{arch::naked_asm, fmt::{self, Write}, sync::atomic::{AtomicBool, AtomicU64, Ordering}};

use pc_keyboard::{DecodedKey, HandleControl, KeyCode, KeyState, PS2Keyboard, ScancodeSet1, layouts};
use spin::Mutex;
use x86_64::{VirtAddr, instructions::port::Port, registers::control::Cr2, structures::idt::{InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode}};
use lazy_static::lazy_static;
use crate::{apic::LOCAL_APIC, backtrace::Backtrace, gdt, pic::PIC_1_OFFSET, process::Pid, ringbuf::RingBuf, scheduler::{SCHEDULER, kill_current_and_schedule, schedule}, serial::SERIAL1, serial_println, utils::{Registers, hlt_loop}, vga::{CursorMove, WRITER}};

#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum InterruptIndex {
    Timer = PIC_1_OFFSET,
    Keyboard,
    Spurious = 0xFF, // only for APIC
}

lazy_static! {
    static ref IDT: InterruptDescriptorTable = {
        let mut idt = InterruptDescriptorTable::new();
        idt.breakpoint.set_handler_fn(breakpoint_handler);
        unsafe {
            idt.double_fault.set_handler_fn(double_fault_handler).set_stack_index(gdt::DOUBLE_FAULT_IST_INDEX);
        }
        idt.page_fault.set_handler_fn(page_fault_handler);
        unsafe {
            idt[InterruptIndex::Timer as u8].set_handler_addr(VirtAddr::new(timer_interrupt_stub as *const () as u64));
        }
        idt[InterruptIndex::Keyboard as u8].set_handler_fn(keyboard_interrupt_handler);

        idt[InterruptIndex::Spurious as u8].set_handler_fn(spurious_interrupt_handler);

        idt
    };
}

pub fn end_of_interrupt(){
    LOCAL_APIC.get().unwrap().lock().end_of_interrupt();
}

pub fn init_idt() {
    IDT.load();
}

extern "x86-interrupt" fn breakpoint_handler(stack_frame: InterruptStackFrame)
{
    if let Some(mut writer_lock) = WRITER.try_lock(){
        let _ = writeln!(writer_lock, "EXCEPTION: BREAKPOINT\n{:#?}", stack_frame);
    }
    hlt_loop();
}

extern "x86-interrupt" fn double_fault_handler(stack_frame: InterruptStackFrame, _error_code: u64) -> !
{
    panic!("EXCEPTION: DOUBLE FAULT\n{:#?}", stack_frame);
}

enum ErrorCause {
    Read,
    Write,
    Execute,
}

struct StackStr<'a> {
    buf: &'a mut [u8],
    len: usize,
}

impl <'a> StackStr<'a> {
    fn new(buf : &'a mut [u8]) -> StackStr<'a> {
        StackStr { buf, len: 0}
    }

    fn as_str(&self) -> &str {
        unsafe {
            str::from_utf8_unchecked(&self.buf[..self.len])
        }
    }
} 

impl Write for StackStr<'_> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let end = self.len.checked_add(s.len()).ok_or(fmt::Error)?;
        if end > self.buf.len(){
            return Err(fmt::Error);
        }

        self.buf[self.len..end].copy_from_slice(s.as_bytes());
        self.len = end;
        Ok(())
    }
}

fn print_segfault_infos<W : Write>(writer : &mut W, current_pid : Pid, error_code: PageFaultErrorCode, accessed_addr : Option<usize>){
    let _ = write!(writer, "segfault of process {} : ", current_pid.0.get());
    
    let present = error_code.contains(PageFaultErrorCode::PROTECTION_VIOLATION);
    let error_cause = if error_code.contains(PageFaultErrorCode::CAUSED_BY_WRITE){
        ErrorCause::Write
    } else if error_code.contains(PageFaultErrorCode::INSTRUCTION_FETCH) {
        ErrorCause::Execute
    } else {
        ErrorCause::Read
    };

    const NO_ADDR_STR : &str = "NO ADDRESS";

    let mut accessed_addr_buf = [0 as u8; 18];
    let mut access_addr_stack_str = StackStr::new(&mut accessed_addr_buf);
    let access_addr_str= match accessed_addr {
        Some(accessed_addr) => {
            if write!(access_addr_stack_str, "{:#x}", accessed_addr).is_ok(){
                access_addr_stack_str.as_str()
            } else {
                NO_ADDR_STR
            }
        },
        None => NO_ADDR_STR,
    };

    let _ = match (present, error_cause){
        (true, ErrorCause::Read) => writeln!(writer, "read from protected memory at {}", access_addr_str),
        (true, ErrorCause::Write) => writeln!(writer, "write to protected memory at {}", access_addr_str),
        (true, ErrorCause::Execute) => writeln!(writer, "execution of protected memory at {}", access_addr_str),
        (false, ErrorCause::Read) => writeln!(writer, "read from unmapped memory at {}", access_addr_str),
        (false, ErrorCause::Write) => writeln!(writer, "write to unmapped memory at {}", access_addr_str),
        (false, ErrorCause::Execute) => writeln!(writer, "execution of unmapped memory at {}", access_addr_str),
    };
}

fn handle_userspace_page_fault(error_code: PageFaultErrorCode, accessed_addr : Option<usize>) -> ! {
    let current_pid = SCHEDULER.lock().current_process.unwrap();
    if let Some(mut writer_lock) = WRITER.try_lock(){
        print_segfault_infos(&mut *writer_lock, current_pid, error_code, accessed_addr);
    }
    if let Some(mut serial_lock) = SERIAL1.try_lock() {
        print_segfault_infos(&mut *serial_lock, current_pid, error_code, accessed_addr);
    }
    // TODO: instead send a signal ? and then print the segfault infos only if the signal is not handled
    kill_current_and_schedule(139);
}


extern "x86-interrupt" fn page_fault_handler(stack_frame: InterruptStackFrame, error_code: PageFaultErrorCode){
    let accessed_addr = Cr2::read().ok().map(|addr| addr.as_u64() as usize);
    let cs = stack_frame.code_segment.0 as u64;

    if is_from_userspace(cs) {
        handle_userspace_page_fault(error_code, accessed_addr)
    }

    if let Some(mut writer_lock) = WRITER.try_lock(){
        let _ = writeln!(writer_lock, "EXCEPTION: PAGE FAULT");
        let _ = writeln!(writer_lock, "Accessed Address: {:#x?}", accessed_addr);
        let _ = writeln!(writer_lock, "Error Code: {:?}", error_code);
        let _ = writeln!(writer_lock, "{:#?}", stack_frame);
    }
    if let Some(mut serial_lock) = SERIAL1.try_lock() {
        let backtrace = Backtrace::new();
        let _ = writeln!(serial_lock, "backtrace page fault, accessed addr {:#x?}, backtrace : {}", accessed_addr, backtrace);
    }
    hlt_loop();
}


#[unsafe(naked)]
pub unsafe extern "C" fn timer_interrupt_stub() -> ! {
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
        handler = sym timer_interrupt_handler,
    )
}

static TICKS: AtomicU64 = AtomicU64::new(0);
const TICKS_EACH_SCHEDULE: u64 = 1; // TODO : if will not change this (so it stays at 1), remove the tick.is_multiple_of check ?

fn is_from_userspace(cs : u64) -> bool {
    (cs & 0b11) == 3
}

pub fn ticks() -> u64 {
    TICKS.load(Ordering::Relaxed)
}

fn timer_interrupt_handler(regs : &mut Registers){
    let tick = TICKS.fetch_add(1, Ordering::Relaxed) + 1;

    let should_schedule = tick.is_multiple_of(TICKS_EACH_SCHEDULE) && is_from_userspace(regs.cs); // TODO : make the kernel preemptible (need to remove the is_userspace, but need to add enable_prempt disable_preempt sections, need to think about ikt)

    end_of_interrupt();

    if should_schedule {
        // timer in user code
        schedule(regs);
    }
}



// TODO : make the layout dynamic (use AnyLayout enum ?)
lazy_static! {
    static ref KEYBOARD: Mutex<PS2Keyboard<layouts::Azerty, ScancodeSet1>> =
        Mutex::new(PS2Keyboard::new(ScancodeSet1::new(), layouts::Azerty, HandleControl::Ignore));
}

const DELETE: char = '\u{007f}';

pub static KEYBOARD_RINGBUF : Mutex<RingBuf<char, 512>> = Mutex::new(RingBuf::new());

static CTRL_DOWN: AtomicBool = AtomicBool::new(false);

extern "x86-interrupt" fn keyboard_interrupt_handler(_stack_frame: InterruptStackFrame) {
    let mut keyboard = KEYBOARD.lock();
    let mut port = Port::new(0x60);
    let scancode : u8 = unsafe { port.read() };

    if let Ok(Some(key_event)) = keyboard.add_byte(scancode) {
        if let Some(key) = keyboard.process_keyevent(key_event.clone()) {
            match key {
                DecodedKey::Unicode(c) => {
                    serial_println!("keyboard: pushing {:?}", c);
                    if c == 'c' && CTRL_DOWN.load(Ordering::Relaxed) {
                        // TODO : handle the foreground process in the case (add the concept of foreground process, which is a global pid that is stopped on ctrl c, so set the new process as the foreground process, then after it exiting, set as the shell process as the foreground process, but after adding signals, add a SIGINT handler to not kill the shell when doing ctrl c)
                    }
                    KEYBOARD_RINGBUF.lock().push(c);
                    serial_println!("keyboard: waking waiter");
                    SCHEDULER.lock().new_char();
                    //print!("{}", c);
                },
                DecodedKey::RawKey(key) => {
                    match key {
                        // TODO  shift, ctrl, etc
                        KeyCode::ArrowLeft => {
                            //CLI_CONTEXT.lock().cursor.move_cursor(CursorMove::Left);
                            WRITER.lock().move_cursor(CursorMove::Left);
                        }
                        KeyCode::ArrowRight => {
                            //CLI_CONTEXT.lock().cursor.move_cursor(CursorMove::Right);
                            WRITER.lock().move_cursor(CursorMove::Right);
                        },
                        KeyCode::LShift => {}, // Do nothing, because pc-keyboard already does the shift for the chars
                        KeyCode::LControl => {
                            CTRL_DOWN.store(
                                key_event.state == KeyState::Down,
                                Ordering::Relaxed,
                            );
                        }
                        _ => serial_println!("{:?}", key),
                    }
                },
            }
        }
    }

    end_of_interrupt();
}

extern "x86-interrupt" fn spurious_interrupt_handler(_stack_frame: InterruptStackFrame) {
}