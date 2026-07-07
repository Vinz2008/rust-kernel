#![no_std]

pub const SYSCALL_EXIT : u64 = 0;
pub const SYSCALL_PRINT : u64 = 1;
pub const SYSCALL_EXEC : u64 = 2;
pub const SYSCALL_GET_CHAR : u64 = 3;
pub const SYSCALL_WAIT_PID : u64 = 4;
pub const SYSCALL_STAT : u64 = 5;
pub const SYSCALL_OPEN : u64 = 6;
pub const SYSCALL_CLOSE : u64 = 7;
pub const SYSCALL_GET_CWD : u64 = 8;

pub const BACKSPACE: char = '\u{0008}';
pub const BACKSPACE_BYTE : u8 = b'\x08';

pub enum StatMode {
    File {
        size : usize,
    },
    Directory,
}

#[repr(C)]
pub struct Stat {
    pub mode : StatMode,
}

pub const READABLE : u64 = 0x1;
pub const WRITABLE : u64 = 0x2;

#[repr(transparent)]
pub struct Fd(pub usize);
