use core::fmt::{self, Write};

use crate::syscall::syscall_print;

struct Writer;

impl fmt::Write for Writer {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let res = syscall_print(s);
        if res.is_none() {
            return Err(fmt::Error);
        }
        Ok(())
    }
}

pub fn _print(args: fmt::Arguments){
    Writer.write_fmt(args).unwrap();
}

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => ($crate::print::_print(format_args!($($arg)*)));
}

#[macro_export]
macro_rules! println {
    () => ($crate::print!("\n"));
    ($($arg:tt)*) => ($crate::print!("{}\n", format_args!($($arg)*)));
}