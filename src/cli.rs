use core::cmp::min;

use arrayvec::ArrayString;
use lazy_static::lazy_static;
use spin::Mutex;
use x86_64::instructions::port::Port;

use crate::{print, println, serial_println, vga::{BUFFER_WIDTH, ColorCode, WRITER}};

pub struct CliContext {
    cli_line : ArrayString<100>,
    pub cursor : Cursor,
}

impl CliContext {
    pub fn add_char(&mut self, c : char){
        self.cli_line.push(c);
    }

    pub fn launch_cmd_cli(&mut self){
        println!();
        let mut argv = self.cli_line.split_ascii_whitespace();
        match argv.next().unwrap() {
            "clear" => {
                WRITER.lock().reset();
            },
            "echo" => {
                let mut writer_lock = WRITER.lock();
                let mut is_first = true;
                for arg in argv {
                    if is_first {
                        is_first = false;
                    } else {
                        writer_lock.write_byte(b' ');
                    }
                    writer_lock.write_string(arg);
                }
                writer_lock.write_byte(b'\n');
            }
            cmd => println!("Unknown command {} !!", cmd),
        }
        print!(">");
        (self.cursor.row_pos, self.cursor.col_pos) = {
            let writer_lock = WRITER.lock();
            (writer_lock.get_row(), writer_lock.get_col())
        };
        self.cursor.update_cursor();
        self.cli_line.clear();
    }
}

pub struct Cursor {
    row_pos : usize,
    col_pos : usize,
}

pub enum CursorMove {
    Left,
    Right,
}

impl Cursor {
    fn update_cursor(&self){
        let pos = self.row_pos * BUFFER_WIDTH + self.col_pos;
        let mut port1 = Port::new(0x3D4);
        let mut port2 = Port::new(0x3D5);
        unsafe {
            port1.write(0x0F as u8);
            port2.write((pos & 0xFF) as u8);
            port1.write(0x0E);
            port2.write(((pos >> 8) & 0xFF) as u8);
        }
        self.handle_cursor_color();
    }   
    fn handle_cursor_color(&self){
        let mut writer_lock = WRITER.lock();
        let mut c = writer_lock.buffer.read_char(self.row_pos, self.col_pos);
        c.color_code = writer_lock.get_color();
        writer_lock.buffer.write_char(self.row_pos, self.col_pos, c);
    }

    fn move_cursor_at(&mut self, row : usize, col : usize){
        self.row_pos = row;
        self.col_pos = col;
        self.update_cursor();
    }

    pub fn move_cursor(&mut self, cursor_move : CursorMove){
        let (new_row, new_col) = match cursor_move {
            CursorMove::Right => (self.row_pos, min(self.col_pos+1, BUFFER_WIDTH)),
            CursorMove::Left => (self.row_pos, self.col_pos.checked_sub(1).unwrap_or(0))
        };
        self.move_cursor_at(new_row, new_col);

    }
}

lazy_static! {
    pub static ref CLI_CONTEXT : Mutex<CliContext> = {
        let cli_context = CliContext {
            cursor: Cursor { row_pos: WRITER.lock().get_row(), col_pos: 0 },
            cli_line: ArrayString::new_const(),
        };
        Mutex::new(cli_context)
    };
}


pub fn init_cli(){
    print!(">");
    let mut cursor_lock = CLI_CONTEXT.lock();
    cursor_lock.cursor.col_pos += 1;
    cursor_lock.cursor.update_cursor();
}