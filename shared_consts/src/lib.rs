#![no_std]

pub const SYSCALL_EXIT : u64 = 0;
pub const SYSCALL_PRINT : u64 = 1;
pub const SYSCALL_EXEC : u64 = 2;
pub const SYSCALL_GET_CHAR : u64 = 3;
pub const SYSCALL_WAIT_PID : u64 = 4;

pub const BACKSPACE: char = '\u{0008}';
pub const BACKSPACE_BYTE : u8 = b'\x08';