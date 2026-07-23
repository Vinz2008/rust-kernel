use core::{arch::naked_asm, fmt::Write, sync::atomic::{AtomicU64, Ordering}};

use pc_keyboard::{DecodedKey, HandleControl, KeyCode, PS2Keyboard, ScancodeSet1, layouts};
use spin::Mutex;
use x86_64::{PrivilegeLevel, VirtAddr, instructions::port::Port, registers::control::Cr2, structures::idt::{InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode}};
use lazy_static::lazy_static;
use crate::{backtrace::Backtrace, gdt, pic::{PIC_1_OFFSET, PICS}, ringbuf::RingBuf, scheduler::{SCHEDULER, kill_current_and_schedule, schedule}, serial::SERIAL1, serial_println, syscall::syscall_interrupt_stub, utils::{Registers, hlt_loop}, vga::{CursorMove, WRITER}};

#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum InterruptIndex {
    Timer = PIC_1_OFFSET,
    Keyboard,
    Syscall = 0x80,
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

        unsafe {
            idt[InterruptIndex::Syscall as u8].set_handler_addr(VirtAddr::new(syscall_interrupt_stub as *const () as u64)).set_privilege_level(PrivilegeLevel::Ring3).disable_interrupts(false);
        }

        idt
    };
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

fn is_from_userspace(cs : u64) -> bool {
    (cs & 0b11) == 3
}

fn handle_userspace_page_fault() -> ! {
    let current_pid = SCHEDULER.lock().current_process.unwrap();
    if let Some(mut writer_lock) = WRITER.try_lock(){
        let _ = writeln!(writer_lock, "segfault of process {}", current_pid.0.get());
    }
    kill_current_and_schedule(139);
}


extern "x86-interrupt" fn page_fault_handler(stack_frame: InterruptStackFrame, error_code: PageFaultErrorCode){
    let cs = stack_frame.code_segment.0 as u64;

    if is_from_userspace(cs) {
        handle_userspace_page_fault()
    }

    if let Some(mut writer_lock) = WRITER.try_lock(){
        let _ = writeln!(writer_lock, "EXCEPTION: PAGE FAULT");
        let _ = writeln!(writer_lock, "Accessed Address: {:?}", Cr2::read());
        let _ = writeln!(writer_lock, "Error Code: {:?}", error_code);
        let _ = writeln!(writer_lock, "{:#?}", stack_frame);
    }
    if let Some(mut serial_lock) = SERIAL1.try_lock() {
        let backtrace = Backtrace::new();
        serial_lock.write_fmt(format_args!("backtrace page fault {}", backtrace)).unwrap();
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
const TICKS_EACH_SCHEDULE: u64 = 10; // TODO : reprogram pic to 100 Hz

fn timer_interrupt_handler(regs : &mut Registers){
    //print!(".");
    unsafe {
        PICS.lock()
            .notify_end_of_interrupt(InterruptIndex::Timer as u8);
    }

    let tick = TICKS.fetch_add(1, Ordering::Relaxed);

    if tick.is_multiple_of(TICKS_EACH_SCHEDULE) && is_from_userspace(regs.cs) {
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

// TODO : should I replace the ringbuf with a VecDeque (that would remove the size limit but would allocate dynamic memory)
pub static KEYBOARD_RINGBUF : Mutex<RingBuf<char, 512>> = Mutex::new(RingBuf::new());

extern "x86-interrupt" fn keyboard_interrupt_handler(_stack_frame: InterruptStackFrame) {
    let mut keyboard = KEYBOARD.lock();
    let mut port = Port::new(0x60);
    let scancode : u8 = unsafe { port.read() };

    if let Ok(Some(key_event)) = keyboard.add_byte(scancode) {
        if let Some(key) = keyboard.process_keyevent(key_event) {
            match key {
                DecodedKey::Unicode(c) => {
                    /*match c {
                        '\n' => {
                            CLI_CONTEXT.lock().launch_cmd_cli();
                        }
                        DELETE | BACKSPACE => {
                            WRITER.lock().remove_last_char();
                            CLI_CONTEXT.lock().cursor.move_cursor(CursorMove::Left);
                        },
                        _ => {
                            print!("{}", c);
                            let mut cli_context_lock = CLI_CONTEXT.lock();
                            cli_context_lock.add_char(c);
                            cli_context_lock.cursor.move_cursor(CursorMove::Right);
                        },
                    }*/
                    serial_println!("keyboard: pushing {:?}", c);
                    
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
                        _ => serial_println!("{:?}", key),
                    }
                },
            }
        }
    }

    unsafe {
        PICS.lock()
            .notify_end_of_interrupt(InterruptIndex::Keyboard as u8);
    }
}