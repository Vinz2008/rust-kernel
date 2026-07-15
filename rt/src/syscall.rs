use core::{hint::unreachable_unchecked, mem::MaybeUninit};

use alloc::vec::Vec;
use arrayvec::ArrayString;
use shared_consts::{Arg, DirChild, Fd, PATH_MAX, SYSCALL_CHANGE_CWD, SYSCALL_CLOSE, SYSCALL_EXEC, SYSCALL_EXIT, SYSCALL_GET_CHAR, SYSCALL_GET_CWD, SYSCALL_GET_DIR_CHILDREN, SYSCALL_OPEN, SYSCALL_PRINT, SYSCALL_SBRK, SYSCALL_SHUTDOWN, SYSCALL_STAT, SYSCALL_WAIT_PID, Stat};

pub unsafe fn syscall0(syscall_nb : u64) -> u64 {
    let ret : u64;
    unsafe {
        core::arch::asm!(
            "int 0x80",
            inlateout("rax") syscall_nb => ret,
            options(nostack)
        );
    }
    ret
}

pub unsafe fn syscall1(syscall_nb : u64, arg1 : u64) -> u64 {
    let ret : u64;
    unsafe {
        core::arch::asm!(
            "int 0x80",
            inlateout("rax") syscall_nb => ret,
            in("rdi") arg1,
            options(nostack)
        );
    }
    ret
}

pub unsafe fn syscall2(syscall_nb : u64, arg1 : u64, arg2 : u64) -> u64 {
    let ret : u64;
    unsafe {
        core::arch::asm!(
            "int 0x80",
            inlateout("rax") syscall_nb => ret,
            in("rdi") arg1,
            in("rsi") arg2,
            options(nostack)
        );
    }
    ret
}

pub unsafe fn syscall3(syscall_nb : u64, arg1 : u64, arg2 : u64, arg3 : u64) -> u64 {
    let ret : u64;
    unsafe {
        core::arch::asm!(
            "int 0x80",
            inlateout("rax") syscall_nb => ret,
            in("rdi") arg1,
            in("rsi") arg2,
            in("rdx") arg3,
            options(nostack)
        );
    }
    ret
}


pub unsafe fn syscall4(syscall_nb : u64, arg1 : u64, arg2 : u64, arg3 : u64, arg4 : u64) -> u64 {
    let ret : u64;
    unsafe {
        core::arch::asm!(
            "int 0x80",
            inlateout("rax") syscall_nb => ret,
            in("rdi") arg1,
            in("rsi") arg2,
            in("rdx") arg3,
            in("r10") arg4,
            options(nostack)
        );
    }
    ret
}

pub fn syscall_exit(status : i32) -> ! {
    unsafe { 
        syscall1(SYSCALL_EXIT, status as u64);
        unreachable_unchecked()
    }
}

fn str_to_ptr_and_len(s : &str) -> (u64, u64) {
    (s.as_ptr() as u64, s.len() as u64)
}

pub fn syscall_print(message : &str) -> Option<()> {
    let (message_ptr, message_len) = str_to_ptr_and_len(message);
    unsafe {
        let res = syscall2(SYSCALL_PRINT, message_ptr, message_len);
        if res == u64::MAX {
            return None;
        }
        Some(())
    }
}

pub fn syscall_exec(path : &str, args : &[&str]) -> u64 {
    let (path_ptr, path_len) = str_to_ptr_and_len(path);
    let args_vec = args.iter().map(|arg| Arg { len: arg.len(), ptr: arg.as_ptr() }).collect::<Vec<_>>();
    let args_ptr = args_vec.as_ptr() as u64;
    let args_len = args.len() as u64;
    unsafe {
        syscall4(SYSCALL_EXEC, path_ptr, path_len, args_ptr, args_len)
    }
}

pub fn syscall_get_char() -> char {
    unsafe {
        let c_u32 = syscall0(SYSCALL_GET_CHAR).try_into().unwrap();
        char::from_u32_unchecked(c_u32) 
    }
}

pub fn syscall_wait_pid(pid : u64){
    unsafe {
        syscall1(SYSCALL_WAIT_PID, pid); 
    }
}

pub fn syscall_stat(path : &str) -> Option<Stat> {
    let (path_ptr, path_len) = str_to_ptr_and_len(path);
    let mut stat = MaybeUninit::uninit();
    let ret = unsafe {
        syscall3(SYSCALL_STAT, path_ptr, path_len, stat.as_mut_ptr() as u64)
    };
    match ret {
        u64::MAX => None,
        _ => unsafe { Some(stat.assume_init()) }
    }
}

pub fn syscall_open(path : &str, mode : u64) -> Option<Fd> {
    let (path_ptr, path_len) = str_to_ptr_and_len(path);
    let ret = unsafe {
        syscall3(SYSCALL_OPEN, path_ptr, path_len, mode)
    };
    match ret {
        u64::MAX => None,
        _ => Some(Fd(ret as usize))
    }
}

pub fn syscall_close(fd : Fd) -> Option<()> {
    let fd = fd.0 as u64;
    let ret = unsafe {
        syscall1(SYSCALL_CLOSE, fd)
    };
    
    match ret {
        u64::MAX => None,
        _ => Some(()),
    }
}

pub fn syscall_get_cwd() -> Option<ArrayString<PATH_MAX>> {
    let mut ret = ArrayString::new();
    let ret_ptr = ret.as_ptr() as u64;
    let ret_len = ret.capacity() as u64;
    let ret_syscall = unsafe {
        syscall2(SYSCALL_GET_CWD, ret_ptr, ret_len)
    };
    match ret_syscall {
        u64::MAX => None,
        len => {
            unsafe {
                ret.set_len(len as usize);
            }
            Some(ret)
        },
    }
}

pub fn syscall_get_dir_children(fd : Fd, children : &mut [DirChild]) -> Option<usize> {
    let fd = fd.0 as u64;
    let children_ptr = children.as_mut_ptr() as u64;
    let children_len = children.len() as u64;
    
    let ret = unsafe {
        syscall3(SYSCALL_GET_DIR_CHILDREN, fd, children_ptr, children_len)
    };

    match ret {
        u64::MAX => None,
        ret => {
            Some(ret as usize)
        }
    }
}

pub fn syscall_sbrk(increment : u64) -> Option<u64> {
    let ret = unsafe {
        syscall1(SYSCALL_SBRK, increment)
    };
    match ret {
        u64::MAX => None,
        ret => Some(ret),
    }
}

pub fn syscall_shutdown(flags : u64) -> ! {
    unsafe {
        syscall1(SYSCALL_SHUTDOWN, flags);
    }
    panic!("shutdown syscall failed")
}

pub fn syscall_change_cwd(dir : &str) -> Option<()> {
    let (dir_ptr, dir_len) = str_to_ptr_and_len(dir);
    let ret = unsafe {
        syscall2(SYSCALL_CHANGE_CWD, dir_ptr, dir_len)
    };
    match ret {
        u64::MAX => None,
        _ => Some(())
    }
}