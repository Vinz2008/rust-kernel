#![no_std]

pub const SYSCALL_EXIT : u64 = 0;
pub const SYSCALL_PRINT : u64 = 1;
pub const SYSCALL_EXEC : u64 = 2;
pub const SYSCALL_GET_CHAR : u64 = 3;