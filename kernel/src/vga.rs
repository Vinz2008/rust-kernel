use core::{fmt, ptr};
use lazy_static::lazy_static;
use spin::Mutex;
use x86_64::instructions::{interrupts, port::Port};

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

    fn set_foreground(&mut self, foreground: Color){
        self.0 = (self.0 & 0xF0) | foreground as u8;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct ScreenChar {
    pub ascii_character: u8,
    pub color_code: ColorCode,
}

pub const BUFFER_HEIGHT: usize = 25;
pub const BUFFER_WIDTH: usize = 80;

const DEFAULT_COLOR : ColorCode = ColorCode::new(Color::Yellow, Color::Black);

const EMPTY_CHAR : ScreenChar = ScreenChar { 
    ascii_character: b' ', 
    color_code: DEFAULT_COLOR,
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
            unsafe {
                ptr::write_volatile(line as *mut _, [EMPTY_CHAR; BUFFER_WIDTH]);
            }
        }
    }
    fn clear_row(&mut self, row: usize) {
        unsafe {
            ptr::write_volatile(&mut self.chars[row] as *mut _, [EMPTY_CHAR; BUFFER_WIDTH]);
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

enum AnsiState {
    Normal,
    Escape,
    CSI,
}

pub struct Writer {
    column_pos: usize,
    row_pos : usize,
    color_code: ColorCode,
    pub buffer: &'static mut Buffer,
    ansi_state : AnsiState, // for escape codes
    // CSI = control sequence introducer = ESC [ = 0x1B 0x5B
    csi_buf : [u8; 16],
    csi_len : usize,

    cursor_row_pos : usize,
    cursor_col_pos : usize,
}

pub enum CursorMove {
    Left,
    Right,
}


// TODO : simplify this, for example with the cursor handling, moving the most possible to the userspace

impl Writer {
    fn write_byte(&mut self , byte: u8){
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
        //self.move_cursor(CursorMove::Right);
        self.sync_cursor_to_print_pos();
        self.column_pos += 1;

        if self.column_pos >= BUFFER_WIDTH {
            self.new_line();
        } else {
            self.sync_cursor_to_print_pos();
        }
    }

    pub fn write_ansi_byte(&mut self, byte: u8) {
        match self.ansi_state {
            AnsiState::Normal => {
                match byte {
                    b'\n' => self.new_line(),
                    0x20..=0x7e => self.write_byte(byte),
                    0x1B => {
                        self.ansi_state = AnsiState::Escape;
                    }
                    _ => self.write_byte(0xfe),
                }
            },
            AnsiState::Escape => {
                match byte {
                    b'[' => {
                        self.ansi_state = AnsiState::CSI;
                        self.csi_len = 0;
                    }
                    _ => self.ansi_state = AnsiState::Normal,
                }
            }
            AnsiState::CSI => {
                if self.csi_len < self.csi_buf.len() {
                    self.csi_buf[self.csi_len] = byte;
                    self.csi_len += 1;
                } else {
                    self.ansi_state = AnsiState::Normal;
                    return;
                }
                
                if 0x40 <= byte && byte <= 0x7e { 
                    // is final byte, so csi finished
                    self.execute_escape_code();
                    self.ansi_state = AnsiState::Normal;
                    self.csi_len = 0;
                }
            }
        }
        
    }

    pub fn write_string(&mut self, s: &str) {

        for byte in s.bytes() {
            self.write_ansi_byte(byte);
        }
    }

    fn execute_escape_code(&mut self){
        match &self.csi_buf[..self.csi_len] {
            b"H" => todo!(), // TODO : cursor home, moves the cursor to row 0, col 0
            b"2J" => self.reset(),
            b"0m" => self.color_code = DEFAULT_COLOR,
            b"30m" => self.color_code.set_foreground(Color::Black),
            b"31m" => self.color_code.set_foreground(Color::Red),
            b"32m" => self.color_code.set_foreground(Color::Green),
            b"33m" => self.color_code.set_foreground(Color::Yellow),
            b"34m" => self.color_code.set_foreground(Color::Blue),
            b"35m" => self.color_code.set_foreground(Color::Magenta),
            b"36m" => self.color_code.set_foreground(Color::Cyan),
            b"37m" => self.color_code.set_foreground(Color::White),
            _ => {}
        }
    }

    fn new_line(&mut self) {
        if self.row_pos >= BUFFER_HEIGHT - 1 {
            self.buffer.shift_lines();
        } else {
            self.row_pos += 1;
            //self.cursor_row_pos = self.cursor_row_pos + 1;
        }
        //self.cursor_col_pos = 0;
        //self.update_cursor();

        self.column_pos = 0;
        self.sync_cursor_to_print_pos();
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

    fn sync_cursor_to_print_pos(&mut self) {
        self.cursor_row_pos = self.row_pos;
        self.cursor_col_pos = self.column_pos;
        self.update_cursor();
    }

    fn update_cursor(&self){
        let pos = self.cursor_row_pos * BUFFER_WIDTH + self.cursor_col_pos;
        let mut port1 = Port::<u8>::new(0x3D4);
        let mut port2 = Port::<u8>::new(0x3D5);
        unsafe {
            port1.write(0x0F);
            port2.write((pos & 0xFF) as u8);
            port1.write(0x0E);
            port2.write(((pos >> 8) & 0xFF) as u8);
        }
        //self.handle_cursor_color();
    }

    fn move_cursor_at(&mut self, row : usize, col : usize){
        self.cursor_row_pos = row;
        self.cursor_col_pos = col;
        self.update_cursor();
    }

    pub fn move_cursor_by(&mut self, cursor_move : CursorMove, count : usize){
        let pos = self.cursor_row_pos * BUFFER_WIDTH + self.cursor_col_pos;
        let max_pos = BUFFER_HEIGHT * BUFFER_WIDTH - 1;
        let new_pos = match cursor_move {
            CursorMove::Right => pos.saturating_add(count).min(max_pos),
            CursorMove::Left => pos.saturating_sub(count),
        };
        let new_row = new_pos / BUFFER_WIDTH;
        let new_col = new_pos % BUFFER_WIDTH;
        self.move_cursor_at(new_row, new_col);
    }

    pub fn move_cursor(&mut self, cursor_move : CursorMove){
        self.move_cursor_by(cursor_move, 1);
    }

    pub fn reset(&mut self){
        self.buffer.clear();
        self.column_pos = 0;
        self.row_pos = 0;
        self.sync_cursor_to_print_pos();
    }
}

impl fmt::Write for Writer {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.write_string(s);
        Ok(())
    }
}

lazy_static! {
    pub static ref WRITER: Mutex<Writer> = {
        let mut writer = Writer {
            column_pos: 0,
            row_pos: 0,
            color_code: ColorCode::new(Color::Yellow, Color::Black),
            buffer: unsafe { &mut *(0xb8000 as *mut Buffer) },
            ansi_state:AnsiState::Normal,
            csi_buf: [0; 16],
            csi_len: 0,
            cursor_col_pos: 0,
            cursor_row_pos: 0,
        };
        writer.reset();
        Mutex::new(writer)
    };
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