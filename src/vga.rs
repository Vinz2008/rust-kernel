use core::{fmt, ptr};
use lazy_static::lazy_static;
use spin::Mutex;
use x86_64::instructions::interrupts;

use crate::cli::CLI_CONTEXT;

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Color {
    Black = 0,
    Blue = 1,
    Green = 2,
    Cyan = 3,
    Red = 4,
    Magenta = 5,
    Brown = 6,
    LightGray = 7,
    DarkGray = 8,
    LightBlue = 9,
    LightGreen = 10,
    LightCyan = 11,
    LightRed = 12,
    Pink = 13,
    Yellow = 14,
    White = 15,
}


#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct ColorCode(u8);

impl ColorCode {
    pub const fn new(foreground: Color, background: Color) -> ColorCode {
        ColorCode((background as u8) << 4 | (foreground as u8))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct ScreenChar {
    pub ascii_character: u8,
    pub color_code: ColorCode,
}

const BUFFER_HEIGHT: usize = 25;
pub const BUFFER_WIDTH: usize = 80;

const EMPTY_CHAR : ScreenChar = ScreenChar { 
    ascii_character: b' ', 
    color_code: ColorCode::new(Color::Black, Color::Black),
};

#[repr(transparent)]
pub struct Buffer {
    chars: [[ScreenChar; BUFFER_WIDTH]; BUFFER_HEIGHT],
}

impl Buffer {
    pub fn read_char(&self, row : usize, col : usize) -> ScreenChar {
        unsafe {
            ptr::read_volatile(&self.chars[row][col] as *const _)
        }
    }
    pub fn write_char(&mut self, row : usize, col : usize, c : ScreenChar){
        unsafe {
            ptr::write_volatile(&mut self.chars[row][col] as *mut _, c);
        }
    }
    pub fn clear(&mut self){
        for line in &mut self.chars {
            for col in line {
                unsafe {
                    ptr::write_volatile(col as *mut _, EMPTY_CHAR);
                }
            }
        }
    }
    fn clear_row(&mut self, row: usize) {
        for col in 0..BUFFER_WIDTH {
            self.write_char(row, col, EMPTY_CHAR);
        }
    }
    fn shift_lines(&mut self){
        for line_idx in 0..self.chars.len()-1 {
            unsafe {
                ptr::write_volatile(&mut self.chars[line_idx], self.chars[line_idx+1]);
            }
        }
        self.clear_row(BUFFER_HEIGHT-1);
    }
}

pub struct Writer {
    column_pos: usize,
    row_pos : usize,
    color_code: ColorCode,
    pub buffer: &'static mut Buffer,
}


impl Writer {

    pub fn write_byte(&mut self, byte: u8) {
        match byte {
            b'\n' => self.new_line(),
            byte => {
                if self.column_pos >= BUFFER_WIDTH {
                    self.new_line();
                }

                let row = self.row_pos;
                let col = self.column_pos;

                let color_code = self.color_code;
                self.buffer.write_char(row, col, ScreenChar {
                    ascii_character: byte,
                    color_code,
                });
                self.column_pos += 1;
            }
        }
    }

    pub fn write_string(&mut self, s: &str) {
        for byte in s.bytes() {
            match byte {
                // printable ASCII byte or newline
                0x20..=0x7e | b'\n' => self.write_byte(byte),
                // not part of printable ASCII range
                _ => self.write_byte(0xfe),
            }

        }
    }

    fn new_line(&mut self) {
        if self.row_pos >= BUFFER_HEIGHT - 1 {
            self.buffer.shift_lines();
        } else {
            self.row_pos += 1;
        }
        self.column_pos = 0;
    }    

    pub fn get_row(&self) -> usize {
        self.row_pos
    }

    pub fn get_col(&self) -> usize {
        self.column_pos
    }

    pub fn get_color(&self) -> ColorCode {
        self.color_code
    }

    pub fn remove_last_char(&mut self){
        if self.column_pos > 0 {
            self.column_pos -= 1;
            self.buffer.write_char(self.row_pos, self.column_pos, EMPTY_CHAR);
        }
    }

    pub fn reset(&mut self){
        self.buffer.clear();
        self.column_pos = 0;
        self.row_pos = 0;
    }
}

impl fmt::Write for Writer {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.write_string(s);
        Ok(())
    }
}

lazy_static! {
    pub static ref WRITER: Mutex<Writer> = Mutex::new(Writer {
        column_pos: 0,
        row_pos: 0,
        color_code: ColorCode::new(Color::Yellow, Color::Black),
        buffer: unsafe { &mut *(0xb8000 as *mut Buffer) },
    });
}



#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => ($crate::vga::_print(format_args!($($arg)*)));
}

#[macro_export]
macro_rules! println {
    () => ($crate::print!("\n"));
    ($($arg:tt)*) => ($crate::print!("{}\n", format_args!($($arg)*)));
}

#[doc(hidden)]
pub fn _print(args: fmt::Arguments) {
    use core::fmt::Write;
    interrupts::without_interrupts(|| {
        WRITER.lock().write_fmt(args).unwrap();
    });
}



#[test_case]
fn test_println_simple() {
    println!("test_println_simple output");
}

#[test_case]
fn test_println_many() {
    for _ in 0..200 {
        println!("test_println_many output");
    }
}


#[test_case]
fn test_println_output() {
    let s = "Some test string that fits on a single line";
    println!("{}", s);
    for (i, c) in s.chars().enumerate() {
        let screen_char = WRITER.lock().buffer.read_char(BUFFER_HEIGHT - 2, i);
        assert_eq!(char::from(screen_char.ascii_character), c);
    }
}