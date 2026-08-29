use shared_consts::STDIN_FD;

use crate::syscall::syscall_read;

pub struct Reader;

impl Reader {
    pub fn read_byte() -> u8 {
        // TODO : optimize this ?
        // TODO : also should I improve it for utf8 support ? (or should it return u8 instead ?)
        let mut buf = [0; 1];
        syscall_read(STDIN_FD, &mut buf);
        buf[0]
    }
}