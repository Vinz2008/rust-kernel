#![no_std]

// TODO : replace this with an enum with numbers for each variant ?
pub const SYSCALL_EXIT : u64 = 0;
pub const SYSCALL_EXEC : u64 = 1;
pub const SYSCALL_GET_CHAR : u64 = 2;
pub const SYSCALL_WAIT_PID : u64 = 3;
pub const SYSCALL_STAT : u64 = 4;
pub const SYSCALL_OPEN : u64 = 5;
pub const SYSCALL_CLOSE : u64 = 6;
pub const SYSCALL_GET_CWD : u64 = 7;
pub const SYSCALL_GET_DIR_CHILDREN : u64 = 8;
pub const SYSCALL_SBRK : u64 = 9;
pub const SYSCALL_SHUTDOWN : u64 = 10;
pub const SYSCALL_CHANGE_CWD : u64 = 11;
pub const SYSCALL_FSTAT : u64 = 12;
pub const SYSCALL_READ : u64 = 13;
pub const SYSCALL_WRITE : u64 = 14;

pub const BACKSPACE: char = '\u{0008}';
pub const BACKSPACE_BYTE : u8 = b'\x08';

#[repr(C)]
pub enum StatMode {
    File {
        size : usize,
    },
    Directory,
    Device,
}

#[repr(C)]
pub struct Stat {
    pub mode : StatMode,
}

pub const READABLE : u64 = 0x1;
pub const WRITABLE : u64 = 0x2;
pub const CREATE_FILE : u64 = 0x4;

// TODO : add generations to fd (but only on kernel side ? or also on userspace size ?)
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct Fd(pub usize);

pub const DIRENT_FILE : u8 = 1;
pub const DIRENT_DIR : u8 = 2;
pub const DIRENT_DEVICE : u8 = 3;

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

pub const STDOUT_FD : Fd = Fd(0);