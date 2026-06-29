use core::fmt::Write;

use pc_keyboard::{DecodedKey, HandleControl, KeyCode, PS2Keyboard, ScancodeSet1, layouts};
use spin::Mutex;
use x86_64::{PrivilegeLevel, VirtAddr, instructions::{interrupts, port::Port}, registers::control::Cr2, structures::idt::{InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode}};
use lazy_static::lazy_static;
use crate::{backtrace::Backtrace, cli::{CLI_CONTEXT, CursorMove}, gdt, pic::{PIC_1_OFFSET, PICS}, print, println, serial::SERIAL1, serial_println, syscall::syscall_interrupt_stub, utils::hlt_loop, vga::WRITER};

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
        idt[InterruptIndex::Timer as u8].set_handler_fn(timer_interrupt_handler);
        idt[InterruptIndex::Keyboard as u8].set_handler_fn(keyboard_interrupt_handler);

        // TODO : add also syscall instruction support
        unsafe {
            idt[InterruptIndex::Syscall as u8].set_handler_addr(VirtAddr::new(syscall_interrupt_stub as *const () as u64)).set_privilege_level(PrivilegeLevel::Ring3);
        }

        idt
    };
}

pub fn init_idt() {
    IDT.load();
}

extern "x86-interrupt" fn breakpoint_handler(stack_frame: InterruptStackFrame)
{
    println!("EXCEPTION: BREAKPOINT\n{:#?}", stack_frame);
}

extern "x86-interrupt" fn double_fault_handler(stack_frame: InterruptStackFrame, _error_code: u64) -> !
{
    panic!("EXCEPTION: DOUBLE FAULT\n{:#?}", stack_frame);
}

extern "x86-interrupt" fn page_fault_handler(stack_frame: InterruptStackFrame, error_code: PageFaultErrorCode){
    println!("EXCEPTION: PAGE FAULT");
    println!("Accessed Address: {:?}", Cr2::read());
    println!("Error Code: {:?}", error_code);
    println!("{:#?}", stack_frame);
    if let Some(mut serial_lock) = SERIAL1.try_lock() {
        let backtrace = Backtrace::new();
        interrupts::without_interrupts(|| serial_lock.write_fmt(format_args!("backtrace page fault {}", backtrace)).unwrap());
    }
    hlt_loop();
}


extern "x86-interrupt" fn timer_interrupt_handler(
    _stack_frame: InterruptStackFrame)
{
    //print!(".");
    unsafe {
        PICS.lock()
            .notify_end_of_interrupt(InterruptIndex::Timer as u8);
    }
}



// TODO : make the layout dynamic (use AnyLayout enum ?)
lazy_static! {
    static ref KEYBOARD: Mutex<PS2Keyboard<layouts::Azerty, ScancodeSet1>> =
        Mutex::new(PS2Keyboard::new(ScancodeSet1::new(), layouts::Azerty, HandleControl::Ignore));
}

const DELETE: char = '\u{007f}';
const BACKSPACE: char = '\u{0008}';

extern "x86-interrupt" fn keyboard_interrupt_handler(_stack_frame: InterruptStackFrame) {
    let mut keyboard = KEYBOARD.lock();
    let mut port = Port::new(0x60);
    let scancode : u8 = unsafe { port.read() };

    if let Ok(Some(key_event)) = keyboard.add_byte(scancode) {
        if let Some(key) = keyboard.process_keyevent(key_event) {
            match key {
                DecodedKey::Unicode(c) => {
                    match c {
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
                    }
                },
                DecodedKey::RawKey(key) => {
                    match key {
                        // TODO  shift, ctrl, etc
                        KeyCode::ArrowLeft => {
                            CLI_CONTEXT.lock().cursor.move_cursor(CursorMove::Left);
                        }
                        KeyCode::ArrowRight => {
                            CLI_CONTEXT.lock().cursor.move_cursor(CursorMove::Right);
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