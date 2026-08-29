use core::sync::atomic::{AtomicBool, Ordering};
use pc_keyboard::{DecodedKey, HandleControl, KeyCode, KeyState, PS2Keyboard, ScancodeSet1, layouts};
use lazy_static::lazy_static;
use spin::Mutex;
use x86_64::{instructions::port::Port, structures::idt::InterruptStackFrame};

use crate::{interrupts::end_of_interrupt, ringbuf::RingBuf, scheduler::SCHEDULER, serial_println, vga::{CursorMove, WRITER}};

// TODO : make the layout dynamic (use AnyLayout enum ?)
lazy_static! {
    static ref KEYBOARD: Mutex<PS2Keyboard<layouts::Azerty, ScancodeSet1>> =
        Mutex::new(PS2Keyboard::new(ScancodeSet1::new(), layouts::Azerty, HandleControl::Ignore));
}

// TODO : increase size to 1024 ?)
pub static KEYBOARD_RINGBUF : Mutex<RingBuf<u8, 512>> = Mutex::new(RingBuf::new());

static CTRL_DOWN: AtomicBool = AtomicBool::new(false);

const DELETE: char = '\u{007f}';

pub extern "x86-interrupt" fn keyboard_interrupt_handler(_stack_frame: InterruptStackFrame) {
    unsafe {
        core::arch::asm!("clac", options(nostack));
    }
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
                    let mut buf = [0; char::MAX_LEN_UTF8];
                    let encoded = c.encode_utf8(&mut buf);
                    {
                        let mut ringbuf = KEYBOARD_RINGBUF.lock();
                        ringbuf.extend(encoded.bytes());
                    }
                    
                    
                    serial_println!("keyboard: waking waiter");
                    SCHEDULER.lock().new_char();
                },
                DecodedKey::RawKey(key) => {
                    let sequence = match key {
                        // TODO  shift, ctrl, etc
                        KeyCode::ArrowLeft => {
                            Some("\x1B[D")
                        }
                        KeyCode::ArrowRight => {
                            Some("\x1B[C")
                            //WRITER.lock().move_cursor(CursorMove::Right);
                        },
                        KeyCode::ArrowDown => {
                            Some("\x1B[B")
                        },
                        KeyCode::ArrowUp => {
                            Some("\x1B[A")
                        },
                        KeyCode::LShift => {
                            // Do nothing, because pc-keyboard already does the shift for the chars
                            None
                        },
                        KeyCode::LControl => {
                            CTRL_DOWN.store(
                                key_event.state == KeyState::Down,
                                Ordering::Relaxed,
                            );
                            None
                        }
                        _ => {
                            serial_println!("{:?}", key);
                            None
                        },
                    };
                    if let Some(sequence) = sequence {
                        KEYBOARD_RINGBUF.lock().extend(sequence.bytes());
                        SCHEDULER.lock().new_char();
                    }
                },
            }
        }
    }

    end_of_interrupt();
}