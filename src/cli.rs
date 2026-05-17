use core::cmp::min;

use spin::Mutex;
use x86_64::instructions::port::Port;

use crate::vga::BUFFER_WIDTH;

pub struct Cursor {
    row_pos : usize,
    col_pos : usize,
}

pub enum CursorMove {
    Left,
    Right,
}

impl Cursor {
    fn move_cursor_at(&mut self, row : usize, col : usize){
        let pos = row * BUFFER_WIDTH + col;
        let mut port1 = Port::new(0x3D4);
        let mut port2 = Port::new(0x3D5);
        unsafe {
            port1.write(0x0F as u8);
            port2.write((pos & 0xFF) as u8);
            port1.write(0x0E);
            port2.write(((pos >> 8) & 0xFF) as u8);
        }
        self.row_pos = row;
        self.col_pos = col;
    }
    pub fn move_cursor(&mut self, cursor_move : CursorMove){
        let (new_row, new_col) = match cursor_move {
            CursorMove::Right => (self.row_pos, min(self.col_pos+1, BUFFER_WIDTH)),
            CursorMove::Left => (self.row_pos, self.col_pos.checked_sub(1).unwrap_or(0))
        };
        self.move_cursor_at(new_row, new_col);

    }
}

pub static CURSOR : Mutex<Cursor> = Mutex::new(Cursor {
    row_pos: 0,
    col_pos: 0,
});