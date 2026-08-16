use core::fmt::{self, Write};

use shared_consts::STDOUT_FD;
use spin::Mutex; // TODO : have a real mutex implementation using the OS, then remove the spinning mutex

use crate::syscall::{syscall_write};


const STDIO_BUF_SIZE: usize = 4096;
struct Writer {
    buf : [u8; STDIO_BUF_SIZE],
    pos : usize,
}

static WRITER : Mutex<Writer> = Mutex::new(Writer { buf: [0; STDIO_BUF_SIZE], pos: 0, });


impl Writer {
    fn flush(&mut self) -> fmt::Result {
        if self.pos == 0 {
            return Ok(());
        }
        let data = &self.buf[..self.pos];
        syscall_write(STDOUT_FD, data).ok_or(fmt::Error)?;
        self.pos = 0;
        Ok(())
    }
}

impl fmt::Write for Writer {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let bytes = s.as_bytes();
        let bytes_len = bytes.len();
        if bytes_len >= STDIO_BUF_SIZE {
            self.flush()?;
            syscall_write(STDOUT_FD, bytes).ok_or(fmt::Error)?;
            return Ok(());
        }

        if self.pos + bytes_len > STDIO_BUF_SIZE {
            self.flush()?;
        }
        self.buf[self.pos..self.pos+bytes_len].copy_from_slice(bytes);
        self.pos += bytes_len;

        if bytes.contains(&b'\n'){
            self.flush()?;
        }
        Ok(())
    }
}

pub fn flush_stdout() -> fmt::Result {
    WRITER.lock().flush()
}

pub fn _print(args: fmt::Arguments) -> fmt::Result {
    WRITER.lock().write_fmt(args)
}

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => ($crate::print::_print(format_args!($($arg)*)).unwrap());
}

#[macro_export]
macro_rules! println {
    () => ($crate::print!("\n"));
    ($($arg:tt)*) => ($crate::print!("{}\n", format_args!($($arg)*)));
}