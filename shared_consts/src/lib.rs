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
pub const SYSCALL_GET_DIR_CHILDREN : u64 = 9;
pub const SYSCALL_SBRK : u64 = 10;
pub const SYSCALL_SHUTDOWN : u64 = 11;
pub const SYSCALL_CHANGE_CWD : u64 = 12;

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

#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct Fd(pub usize);

pub const DIRENT_FILE : u8 = 1;
pub const DIRENT_DIR : u8 = 2;

// TOO : make this variable length like linux_dirent in linux
#[repr(C)]
#[derive(Clone, Copy)]
pub struct DirChild {
    pub kind : u8,
    pub name_len : u8,   
    pub name : [u8; PATH_NAME_MAX],
}

pub const PATH_MAX : usize = 4096; // TODO : add dynamic memory in userspace to use this less
pub const PATH_NAME_MAX : usize = 256;

pub const USER_HEAP_START : usize = 0x0000_0000_4000_0000;
pub const USER_HEAP_SIZE : usize = 1024 * 1024 * 1024; // 1 GiB

// last bit is for success or failure
pub const SHUTDOWN_SUCCESS : u64 = 0x1;
pub const SHUTDOWN_FAILURE : u64 = 0x0;

#[repr(C)]
pub struct Arg {
    pub len : usize,
    pub ptr : *const u8,
}