use shared_consts::STDIN_FD;

use crate::syscall::syscall_read;

pub struct Reader;

// TODO

impl Reader {
    pub fn read_char() -> char {
        // TODO : optimize this ?
        // TODO : also should I improve it for utf8 support ?
        let mut buf = [0; 1];
        syscall_read(STDIN_FD, &mut buf);
        char::from_u32(buf[0] as u32).unwrap()
    }
}